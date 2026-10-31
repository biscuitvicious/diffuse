//! The golden acceptance — TWO legs over the frozen corpus
//! (`tests/golden/diffuse-2026-08/wire.json`), both walking the wire-contract
//! output (PLAN.md §2) and asserting its invariants:
//!
//!   1. **lib leg** — embed the engine, run the full pipeline directly.
//!   2. **bin leg** — spawn the built `diffuse` binary, feed it the same input,
//!      decode its token-stream JSON, run the SAME walk.
//!
//! Both legs must match byte-for-byte — the bin leg pins the wire contract
//! to the engine (a schema change breaks the bin leg even if the engine's
//! semantics are unchanged).
//!
//! The walk asserts the contract is self-consistent over the real corpus:
//! the row-kind/line-number rules, the spans in bounds, the runs
//! concatenating to the display text, the header counts matching a walk of
//! the rows (incl. the mods' absorbed tails), and the two legs'
//! byte-identical canonical JSON.
//!
//! The RENDERED golden (the TUI's colored scrolldown) is walked by the
//! extension's `test/golden.test.ts` — it needs the renderer, which stays
//! in TS. This test proves the engine + the contract; the extension proves
//! the render; they meet at Phase 4's equivalence gate
//! (`REVIEW_BACKEND=rust` renders the byte-identical golden).

use std::{
    collections::HashMap,
    env,
    fs::{self},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::{Arc, LazyLock, Mutex, Once},
    thread,
    time::Instant,
};

use serde::{Deserialize, Serialize};

use diffuse::{
    core::{
        apply::apply_hunk_rows,
        lcs::char_len,
        merge::{RunKind, display_text_with_profile, is_wholesale_mod},
        parse::{Row, RowGroup, RowKind},
        pipeline::{PipelineOptions, process_single_diff_file},
        wire::{MovSide, OutputFile, PipelineOutput},
    },
    render::{
        DiffuseTheme, default_theme, encode_sgr, render_pipeline_output, render_raw_pipeline_output,
    },
    syntax::{profile::LangProfile, registry::lang_profile},
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    diff: Arc<String>,
    new_content: HashMap<String, Arc<String>>,
    /// The OLD file contents, harvested alongside the diff (the apply-oracle
    /// (PLAN.md §4 Phase 6) uses it to prove `apply(diff, old) == new`
    /// mechanically — the diff is a semantic program, not just a render).
    old_content: HashMap<String, Arc<String>>,
    #[serde(skip)]
    notes_file: Option<PathBuf>,
    #[serde(skip)]
    notes_db: Option<diffuse::core::notes::NotesDB>,
    #[serde(skip)]
    repo_key: Option<String>,
    #[serde(skip)]
    rev_key: Option<String>,
}

fn golden_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden"))
}

/// The language profile a file's rows were built with — mirrors
/// `pipeline::process_single_diff_file` (notes render as markdown; anything
/// else keys off the path extension).
fn row_lang_profile(f: &OutputFile) -> Option<&'static LangProfile> {
    if f.status == "note" {
        lang_profile("md")
    } else {
        let ext = f.path.rsplit('.').next().unwrap_or("");
        lang_profile(ext)
    }
}

fn is_golden_initialized_at(path: &Path) -> bool {
    path.join("diffuse-2026-08").join("meta.json").is_file()
}

fn is_golden_initialized() -> bool {
    is_golden_initialized_at(&golden_dir())
}

fn write_file_plain(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;
    Ok(())
}

fn write_gz(path: &Path, data: &[u8]) -> io::Result<()> {
    let gz_path = if path.extension().is_some_and(|ext| ext == "gz") {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.gz", path.display()))
    };
    if let Some(parent) = gz_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(&gz_path)?;
    let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
    Write::write_all(&mut encoder, data)?;
    encoder.finish()?;
    if path.exists() && path != gz_path {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

static FIXTURE_CACHE: LazyLock<Mutex<HashMap<String, Arc<Fixture>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

type RenderCacheMap = HashMap<(String, &'static str), Arc<String>>;
static EXPECTED_RENDER_CACHE: LazyLock<Mutex<RenderCacheMap>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize, Serialize)]
struct MetaFileRecord {
    path: String,
    #[serde(default)]
    old: Option<String>,
    #[serde(default)]
    new: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct CorpusMeta {
    #[serde(default)]
    revision: String,
    #[serde(default)]
    repo_key: Option<String>,
    #[serde(default)]
    rev_key: Option<String>,
    #[serde(default)]
    files: Vec<MetaFileRecord>,
}

fn load_fixture(name: &str) -> Arc<Fixture> {
    {
        let cache = FIXTURE_CACHE.lock().unwrap();
        if let Some(f) = cache.get(name) {
            return Arc::clone(f);
        }
    }
    let corpus_dir = golden_dir().join(name);
    let patch_path = corpus_dir.join("diff.patch");
    let diff = fs::read_to_string(&patch_path)
        .unwrap_or_else(|e| panic!("failed reading diff.patch for {name}: {e}"));
    let meta_path = corpus_dir.join("meta.json");
    let meta_raw = fs::read_to_string(&meta_path)
        .unwrap_or_else(|e| panic!("failed reading meta.json for {name}: {e}"));
    let meta: CorpusMeta = serde_json::from_str(&meta_raw)
        .unwrap_or_else(|e| panic!("failed parsing meta.json for {name}: {e}"));
    let mut new_content = HashMap::new();
    let mut old_content = HashMap::new();
    let files_dir = corpus_dir.join("files");

    use rayon::prelude::*;
    let records = meta.files;
    let loaded: Vec<(String, Option<String>, Option<String>)> = records
        .into_par_iter()
        .map(|record| {
            let new_str = record.new.as_ref().map(|new_file| {
                let new_path = files_dir.join(new_file);
                fs::read_to_string(&new_path)
                    .unwrap_or_else(|e| panic!("failed reading {}: {e}", new_path.display()))
            });
            let old_str = record.old.as_ref().map(|old_file| {
                let old_path = files_dir.join(old_file);
                fs::read_to_string(&old_path)
                    .unwrap_or_else(|e| panic!("failed reading {}: {e}", old_path.display()))
            });
            (record.path, new_str, old_str)
        })
        .collect();

    for (path, new_str, old_str) in loaded {
        if let Some(new_c) = new_str {
            new_content.insert(path.clone(), Arc::new(new_c));
        }
        if let Some(old_c) = old_str {
            old_content.insert(path, Arc::new(old_c));
        }
    }

    let notes_json_path = corpus_dir.join("notes.json");
    let (notes_file, notes_db, repo_key, rev_key) = if notes_json_path.is_file() {
        let db = diffuse::core::notes::load_notes(&notes_json_path);
        let rk = meta
            .repo_key
            .clone()
            .unwrap_or_else(|| "diffuse".to_string());
        let rvk = meta.rev_key.clone().or_else(|| {
            meta.revision
                .split_once("..")
                .map(|(b, _)| b.trim().to_string())
                .filter(|s| !s.is_empty())
        });
        (Some(notes_json_path), Some(db), Some(rk), rvk)
    } else {
        (None, None, None, None)
    };

    let f = Fixture {
        diff: Arc::new(diff),
        new_content,
        old_content,
        notes_file,
        notes_db,
        repo_key,
        rev_key,
    };
    let arc = Arc::new(f);
    let mut cache = FIXTURE_CACHE.lock().unwrap();
    cache.insert(name.to_string(), Arc::clone(&arc));
    arc
}

fn expected_render_path(name: &str, flavor: &str) -> PathBuf {
    golden_dir()
        .join(name)
        .join("outputs")
        .join(format!("render.{flavor}.txt"))
}

fn expected_markdown_path(name: &str) -> PathBuf {
    golden_dir().join(name).join("outputs").join("markdown.md")
}

fn load_expected_render(name: &str, flavor: &'static str) -> Arc<String> {
    {
        let cache = EXPECTED_RENDER_CACHE.lock().unwrap();
        if let Some(r) = cache.get(&(name.to_string(), flavor)) {
            return Arc::clone(r);
        }
    }
    let render_path = expected_render_path(name, flavor);
    let expected = if render_path.exists() {
        fs::read_to_string(&render_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected {flavor} render output for {name} from {}: {e}",
                render_path.display()
            )
        })
    } else {
        let gz_path = golden_dir().join(format!("{name}.render.{flavor}.txt.gz"));
        if gz_path.exists() {
            let bytes = fs::read(&gz_path).unwrap_or_else(|e| {
                panic!(
                    "Failed to read expected {flavor} render output for {name} from {} or {}: {e}",
                    render_path.display(),
                    gz_path.display()
                )
            });
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut s = String::new();
            Read::read_to_string(&mut decoder, &mut s).unwrap();
            s
        } else {
            panic!(
                "Failed to read expected {flavor} render output for {name} from {} (file not found). Use GEN_GOLDEN=1 or UPDATE_RENDER=1 to create/update.",
                render_path.display()
            );
        }
    };
    let arc = Arc::new(expected);
    let mut cache = EXPECTED_RENDER_CACHE.lock().unwrap();
    cache.insert((name.to_string(), flavor), Arc::clone(&arc));
    arc
}

/// The walk: every invariant over the whole corpus. A failure increments
/// `failures` and prints the exact row; the caller asserts zero at the end.
/// Round-trip invariants, checked on live output for every corpus:
/// - old + diff = old snapshot, new − diff = new snapshot: every patch hunk
///   range renders (no drops), and every numbered row matches its snapshot line
///   (no inventions, no misnumbering). Output legitimately contains extra
///   context beyond hunk boundaries (expanded from the snapshots), so the check
///   is required-presence plus snapshot-equality, never exact row/patch
///   equality.
/// - moved-in multiplicity per hunk: moved-in tokens ⊆ struck tokens (fires on
///   double-claimed occurrences, e.g. two old brackets rendered as four context
///   brackets).
fn assert_diff_invariants(
    patch: &str,
    out: &PipelineOutput,
    corpus: &str,
    old_snap: &std::collections::HashMap<String, std::sync::Arc<String>>,
    new_snap: &std::collections::HashMap<String, std::sync::Arc<String>>,
) {
    use std::collections::HashMap;

    // Parse the patch into per-file required changed-line numbers: every
    // `-` line's old number and every `+` line's new number, tracked with
    // running counters reset at each hunk header. Context lines are
    // intentionally NOT required — the engine trims and expands context
    // windows freely; only changed lines must render.
    struct FileReq {
        display: String,
        old_path: String,
        req_old: Vec<(u32, String)>,
        req_new: Vec<(u32, String)>,
        cur_old: u32,
        cur_new: u32,
        in_hunk: bool,
    }
    let mut files: Vec<FileReq> = Vec::new();
    // Split on `diff --git` (not `+++`) so renames and deletions (`+++
    // /dev/null`) also open entries; display path prefers the new side.
    // Paths may contain unquoted spaces; the ` b/` separator between the
    // two sides is unambiguous.
    let split_paths = |line: &str| {
        let rest = line.strip_prefix("diff --git ")?;
        let cut = rest.find(" b/")?;
        let (a, b) = rest.split_at(cut);
        let strip = |p: &str| {
            p.strip_prefix("a/")
                .or_else(|| p.strip_prefix("b/"))
                .or_else(|| p.strip_prefix("/dev/null").map(|_| ""))
                .unwrap_or(p)
                .to_string()
        };
        Some((strip(a), strip(&b[1..])))
    };
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            if let Some((old_p, new_p)) = split_paths(line) {
                let display = if new_p.is_empty() {
                    old_p.clone()
                } else {
                    new_p.clone()
                };
                files.push(FileReq {
                    display,
                    old_path: old_p,
                    req_old: Vec::new(),
                    req_new: Vec::new(),
                    cur_old: 0,
                    cur_new: 0,
                    in_hunk: false,
                });
            }
        } else if line.strip_prefix("@@ ").is_some() {
            // Reset running counters from the hunk header; counts may be
            // omitted (= 1). Zero-length sides (`-0,0`) reset to 0 and
            // record nothing.
            if let Some(f) = files.last_mut() {
                let h = &line[3..];
                let mut sides = h.split(' ').filter_map(|w| {
                    if let Some((a, b)) = w.split_once(',') {
                        let a = a.trim_start_matches(['-', '+']).parse::<u32>().ok()?;
                        let b = b.trim_end_matches('@').parse::<u32>().ok();
                        Some((a, b.unwrap_or(1)))
                    } else {
                        let a = w.trim_start_matches(['-', '+']).parse::<u32>().ok()?;
                        Some((a, 1))
                    }
                });
                if let (Some((os, _)), Some((ns, _))) = (sides.next(), sides.next()) {
                    f.cur_old = os;
                    f.cur_new = ns;
                    f.in_hunk = true;
                }
            }
        } else if let Some(f) = files.last_mut() {
            if !f.in_hunk {
                continue;
            }
            match line.as_bytes().first() {
                Some(b' ') => {
                    f.cur_old += 1;
                    f.cur_new += 1;
                }
                Some(b'-') => {
                    f.req_old.push((f.cur_old, line[1..].to_string()));
                    f.cur_old += 1;
                }
                Some(b'+') => {
                    f.req_new.push((f.cur_new, line[1..].to_string()));
                    f.cur_new += 1;
                }
                _ => {}
            }
        }
    }
    // Skip files without changes (mode changes, binary notices): the engine
    // emits no rows for them.
    let files: Vec<FileReq> = files
        .into_iter()
        .filter(|f| !f.req_old.is_empty() || !f.req_new.is_empty())
        .collect();
    assert_eq!(
        files.len(),
        out.files.len(),
        "[{corpus}] patch files with changes must match output files"
    );
    for (fi, (freq, of)) in files.iter().zip(out.files.iter()).enumerate() {
        let (path, old_path, req_old, req_new) =
            (&freq.display, &freq.old_path, &freq.req_old, &freq.req_new);
        let old_lines: Option<Vec<&str>> = old_snap
            .get(old_path.as_str())
            .or_else(|| old_snap.get(path.as_str()))
            .map(|c| c.lines().collect());
        let new_lines: Option<Vec<&str>> = new_snap
            .get(path.as_str())
            .or_else(|| new_snap.get(old_path.as_str()))
            .map(|c| c.lines().collect());
        let strip_ws = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        // Record an old-side line, firing on contradictory duplicates
        // (same number, different text — overlapping hunks may repeat a
        // row, but never with different content).
        let mut seen_old: HashMap<u32, String> = HashMap::new();
        let mut seen_new: HashMap<u32, String> = HashMap::new();
        let mut record_old = |n: u32, text: String| {
            if let Some(prev) = seen_old.get(&n) {
                assert!(
                    prev == &text,
                    "[{corpus}] {path} (file {fi}): old line {n} renders twice with different text: {prev:?} vs {text:?}"
                );
            } else {
                seen_old.insert(n, text);
            }
        };
        let mut record_new = |n: u32, text: String| {
            if let Some(prev) = seen_new.get(&n) {
                assert!(
                    prev == &text,
                    "[{corpus}] {path} (file {fi}): new line {n} renders twice with different text: {prev:?} vs {text:?}"
                );
            } else {
                seen_new.insert(n, text);
            }
        };
        for h in &of.hunks {
            for r in &h.rows {
                match r.kind {
                    RowKind::Ctx | RowKind::Del => {
                        if let Some(n) = r.old_line {
                            record_old(n, r.old_text());
                        }
                        if matches!(r.kind, RowKind::Ctx)
                            && let Some(n) = r.new_line
                        {
                            record_new(n, r.new_text());
                        }
                    }
                    RowKind::Mod => {
                        // A Collapse anchor's old_text spans its whole
                        // block (possibly reflow-joined): match it against
                        // the snapshot window starting at its first line,
                        // whitespace-insensitively (joins insert spaces).
                        if let Some(n) = r.old_line {
                            let ot = r.old_text();
                            // Window source: snapshot lines when present,
                            // else the patch's own del lines (renamed files
                            // ship only new content). Either way the match
                            // is whitespace-insensitive: Collapse joins
                            // insert spaces.
                            let src: Vec<String> = match &old_lines {
                                Some(snap) => {
                                    let mut v = Vec::new();
                                    let mut m = n;
                                    while v.len() <= 64 {
                                        match snap.get(m as usize - 1) {
                                            Some(l) => v.push(l.to_string()),
                                            None => break,
                                        }
                                        m += 1;
                                    }
                                    v
                                }
                                None => req_old
                                    .iter()
                                    .skip_while(|(k, _)| *k != n)
                                    .map(|(_, t)| t.clone())
                                    .collect(),
                            };
                            let want = strip_ws(&ot);
                            // How many leading source lines the
                            // old_text covers: smallest k with equal
                            // stripped content.
                            let mut acc = String::new();
                            let mut take = 0usize;
                            for l in &src {
                                acc.extend(l.chars().filter(|c| !c.is_whitespace()));
                                take += 1;
                                if acc == want {
                                    break;
                                }
                                if acc.len() > want.len() || take > 64 {
                                    take = usize::MAX;
                                    break;
                                }
                            }
                            if take == usize::MAX || acc != want {
                                panic!(
                                    "[{corpus}] {path} (file {fi}): mod old line {n} old_text {ot:?} matches no source window"
                                );
                            }
                            // Mark covered numbers: snapshot positions when
                            // available, else the patch's own del numbers.
                            if old_lines.is_some() {
                                for k in n..n + take as u32 {
                                    if let Some(snap) = &old_lines {
                                        record_old(k, snap[(k as usize) - 1].to_string());
                                    }
                                }
                            } else {
                                let mut marked = 0;
                                for (k, t) in req_old.iter() {
                                    if *k < n {
                                        continue;
                                    }
                                    if marked >= take {
                                        break;
                                    }
                                    record_old(*k, t.clone());
                                    marked += 1;
                                }
                            }
                        }
                        if let Some(n) = r.new_line {
                            record_new(n, r.new_text());
                        }
                    }
                    RowKind::Add => {
                        if let Some(n) = r.new_line {
                            record_new(n, r.new_text());
                        }
                    }
                    RowKind::Mov => {
                        if r.mov_side == Some(MovSide::Src) {
                            if let Some(n) = r.old_line {
                                record_old(n, r.old_text());
                            }
                        } else if let Some(n) = r.new_line {
                            record_new(n, r.new_text());
                        }
                    }
                    _ => {}
                }
            }
        }
        for (n, _) in req_old {
            assert!(
                seen_old.contains_key(n),
                "[{corpus}] {path} (file {fi}): old line {n} never renders"
            );
        }
        for (n, _) in req_new {
            assert!(
                seen_new.contains_key(n),
                "[{corpus}] {path} (file {fi}): new line {n} never renders"
            );
        }
        // Snapshot equality for single-line rows (joined Mod blocks are
        // covered by the window match above).
        for h in &of.hunks {
            for r in &h.rows {
                if let Some(n) = r.old_line
                    && matches!(r.kind, RowKind::Ctx | RowKind::Del)
                    && let Some(snap) = &old_lines
                    && let Some(exp) = snap.get(n as usize - 1)
                    && r.old_text() != *exp
                {
                    panic!(
                        "[{corpus}] {path} (file {fi}): old line {n} renders as {:?}, snapshot has {exp:?}",
                        r.old_text()
                    );
                }
                if let Some(n) = r.new_line
                    && matches!(r.kind, RowKind::Ctx | RowKind::Add)
                    && let Some(snap) = &new_lines
                    && let Some(exp) = snap.get(n as usize - 1)
                    && r.new_text() != *exp
                {
                    panic!(
                        "[{corpus}] {path} (file {fi}): new line {n} renders as {:?}, snapshot has {exp:?}",
                        r.new_text()
                    );
                }
            }
        }
        // Moved-in multiplicity per hunk. Whitespace tokens are split
        // into single spaces: occurrence supply is positional, so an aux
        // indent may legitimately consume part of a wider struck indent;
        // exact-string token counting would false-fire there.
        // Non-whitespace tokens stay whole (single-char delimiters
        // included — those are the double-claim signal).
        for (hi, oh) in of.hunks.iter().enumerate() {
            let mut struck: HashMap<String, usize> = HashMap::new();
            let mut moved_in: HashMap<String, usize> = HashMap::new();
            let count = |map: &mut HashMap<String, usize>, tok: &str| {
                if diffuse::core::lcs::token_is_ws(tok) {
                    for _ in tok.chars() {
                        *map.entry(" ".to_string()).or_default() += 1;
                    }
                } else {
                    *map.entry(tok.to_string()).or_default() += 1;
                }
            };
            // Merge adjacent same-kind runs first: the wire layer
            // splits runs for syntax coloring (e.g. lifetime `'` from
            // `static`) without changing kinds; tokenizing the fragments
            // would manufacture mismatches against whole-token supply.
            let mut merged: Vec<(RunKind, String)> = Vec::new();
            for r in &oh.rows {
                for run in &r.runs {
                    if let Some((k, t)) = merged.last_mut()
                        && *k == run.kind
                    {
                        t.push_str(&run.text);
                        continue;
                    }
                    merged.push((run.kind, run.text.to_string()));
                }
            }
            for (kind, text) in &merged {
                {
                    let toks = diffuse::core::lcs::tokenize_line(text).collect::<Vec<_>>();
                    match kind {
                        RunKind::Del | RunKind::MovedSrc => {
                            for t in toks {
                                count(&mut struck, t);
                            }
                        }
                        RunKind::MovedTgt => {
                            for t in toks {
                                count(&mut moved_in, t);
                            }
                        }
                        _ => {}
                    }
                }
            }
            for (tok, n) in &moved_in {
                let have = struck.get(tok).copied().unwrap_or(0);
                assert!(
                    have >= *n,
                    "[{corpus}] {path} hunk {hi} (file {fi}): moved-in {tok:?} ×{n} exceeds struck ×{have}"
                );
            }
        }
    }
}
fn walk_and_assert(out: &PipelineOutput, corpus: &str, num_files: usize) {
    let mut failures = 0usize;
    macro_rules! check {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                failures += 1;
                eprintln!("FAIL [{}]: {}", corpus, format_args!($($arg)*));
            }
        };
    }
    check!(
        out.files.len() == num_files,
        "corpus parses into exactly {num_files} files, got {}",
        out.files.len()
    );
    check!(
        out.validate_invariants().is_ok(),
        "{}: output wire invariants failed: {:?}",
        corpus,
        out.validate_invariants().err()
    );
    for f in &out.files {
        check!(!f.path.is_empty(), "a file has an empty path");
        // The header counts must equal a re-walk of the rows
        // Each visual row contributes once. A moved pair is one relocation —
        // only the target side counts as +1 mov; the source side is absorbed.
        let mut adds = 0usize;
        let mut dels = 0usize;
        let mut mods = 0usize;
        let mut movs = 0usize;
        for h in &f.hunks {
            // Aux row texts in this hunk, for the moved-out tripwire below.
            let aux_texts: Vec<String> = h
                .rows
                .iter()
                .filter(|r| r.is_aux())
                .map(|r| {
                    let mut s = r.text();
                    let ot = r.old_text();
                    if !ot.is_empty() {
                        s.push(' ');
                        s.push_str(&ot);
                    }
                    s
                })
                .collect();
            // Anchor deletion texts in this hunk, for the moved-in mirror.
            let anchor_dels: Vec<String> = h
                .rows
                .iter()
                .filter(|r| matches!(r.group, Some(RowGroup::Anchor)))
                .map(|r| r.old_text())
                .filter(|t| !t.is_empty())
                .collect();
            for r in &h.rows {
                match r.kind {
                    RowKind::Mov => {
                        if r.mov_side != Some(MovSide::Src) {
                            movs += 1;
                        }
                    }
                    RowKind::Add => adds += 1,
                    RowKind::Del => dels += 1,
                    RowKind::Mod => mods += 1,
                    _ => {}
                }
                // The line-number rules per kind.
                match r.kind {
                    RowKind::Add => check!(
                        r.new_line.is_some() && r.old_line.is_none(),
                        "{}: add row has newLine {:?} and oldLine {:?}",
                        f.path,
                        r.new_line,
                        r.old_line
                    ),
                    RowKind::Del => check!(
                        r.old_line.is_some() && r.new_line.is_none(),
                        "{}: del row has newLine {:?} and oldLine {:?}",
                        f.path,
                        r.new_line,
                        r.old_line
                    ),
                    RowKind::Mov => {
                        if r.mov_side == Some(MovSide::Src) {
                            check!(
                                r.old_line.is_some() && r.new_line.is_none(),
                                "{}: mov src row has newLine {:?} and oldLine {:?}",
                                f.path,
                                r.new_line,
                                r.old_line
                            );
                        } else {
                            check!(
                                r.new_line.is_some() && r.old_line.is_none(),
                                "{}: mov tgt row has newLine {:?} and oldLine {:?}",
                                f.path,
                                r.new_line,
                                r.old_line
                            );
                        }
                    }
                    RowKind::Mod => {
                        check!(
                            r.new_line.is_some() && r.old_line.is_some(),
                            "{}: mod row has newLine {:?} and oldLine {:?}",
                            f.path,
                            r.new_line,
                            r.old_line
                        );
                    }
                    RowKind::Ctx => {
                        check!(
                            r.new_line.is_some() && r.old_line.is_some(),
                            "{}: ctx row has newLine {:?} and oldLine {:?}",
                            f.path,
                            r.new_line,
                            r.old_line
                        );
                    }
                    _ => {}
                }
                // Spans in bounds (char offsets into their own text).
                let spans = r.spans();
                let text_chars = char_len(&r.new_text());
                for &(s, e) in &spans.add {
                    check!(
                        s <= e && e <= text_chars,
                        "{}: add span {s}..{e} beyond text len {text_chars}",
                        f.path
                    );
                }
                let ot = r.old_text();
                let del_chars = char_len(&ot);
                for &(s, e) in &spans.del {
                    check!(
                        s <= e && e <= del_chars,
                        "{}: del span {s}..{e} beyond oldText len {del_chars}",
                        f.path
                    );
                }
                // The runs concatenate to the display text; del runs carry
                // no color (the strike is the renderer's), common/add do.
                // The display text derives from the row's language profile —
                // the same one the pipeline used to build the row's spans.
                let concat: String = r.runs.iter().map(|x| x.text.as_ref()).collect();
                let expected = display_text_with_profile(&Row::from(r), row_lang_profile(f));
                check!(
                    concat == expected,
                    "{}:{}: runs concatenate to the display text ({} chars vs {})",
                    f.path,
                    r.new_line.unwrap_or(0),
                    concat.chars().count(),
                    expected.chars().count()
                );
                // The DISK-EXACT invariant: removing the struck (del-like)
                // content from the render must leave exactly the row's
                // new-side line — A mod or add row's non-del-like runs
                // concatenate to the new line (the old fragments), and
                // this walk locks it. Moved-out runs are old-side like
                // struck runs (see `RunKind::is_del_like`).
                if r.kind == RowKind::Mod || (r.kind == RowKind::Add && r.is_aux()) {
                    let kept: String = r
                        .runs
                        .iter()
                        .filter(|x| !x.kind.is_del_like())
                        .map(|x| x.text.as_ref())
                        .collect();
                    check!(
                        kept == r.new_text(),
                        "{}:{}: the strike-removed (forward) render differs from the new line",
                        f.path,
                        r.new_line.unwrap_or(0)
                    );
                }
                if r.kind == RowKind::Mod {
                    let rev: String = r
                        .runs
                        .iter()
                        .filter(|x| !x.kind.is_add_like())
                        .map(|x| x.text.as_ref())
                        .collect();
                    let old_text = r.old_text();
                    check!(
                        rev == old_text,
                        "{}:{}: the add-removed (reverse) render differs from the old line. Expected: {:?}, Got: {:?}",
                        f.path,
                        r.old_line.unwrap_or(0),
                        old_text,
                        rev
                    );
                }
                for run in &r.runs {
                    match run.kind {
                        // Moved-out runs ride the del channel (no color/sgr,
                        // like struck runs); only renderers tell them apart.
                        RunKind::Del | RunKind::MovedSrc => check!(
                            run.color.is_default() && run.sgr.is_none(),
                            "{}: a del run carries no color/sgr",
                            f.path
                        ),
                        RunKind::Add | RunKind::Ctx | RunKind::MovedTgt => {
                            if let Some(s) = run.sgr {
                                check!(
                                    matches!(s, 1 | 3 | 4 | 9 | 14),
                                    "{}: invalid sgr {s} (md emphasis)",
                                    f.path
                                );
                            }
                        }
                    }
                    // Tripwire: moved-out text must survive in a same-hunk
                    // aux row — otherwise the engine hid a genuine deletion
                    // behind the moved mark. Token granularity: the moved
                    // ranges come from word alignment, so shared tokens must
                    // survive while edge punctuation (whose occurrence
                    // identity across a reflow is ambiguous) may not match
                    // verbatim.
                    if run.kind == RunKind::MovedSrc && !run.text.is_empty() {
                        let missing: Vec<&str> = run
                            .text
                            .split(|c: char| !c.is_alphanumeric())
                            .filter(|t| !t.is_empty())
                            .filter(|t| !aux_texts.iter().any(|a| a.contains(t)))
                            .collect();
                        check!(
                            missing.is_empty(),
                            "{}:{}: moved-out tokens survive nowhere in-hunk: {:?} (run {:?})",
                            f.path,
                            r.old_line.unwrap_or(0),
                            missing,
                            run.text
                        );
                    }
                    // Mirror: moved-in target tokens must come from a
                    // same-hunk anchor's old text.
                    if run.kind == RunKind::MovedTgt && !run.text.is_empty() {
                        let missing: Vec<&str> = run
                            .text
                            .split(|c: char| !c.is_alphanumeric())
                            .filter(|t| !t.is_empty())
                            .filter(|t| !anchor_dels.iter().any(|a| a.contains(t)))
                            .collect();
                        check!(
                            missing.is_empty(),
                            "{}:{}: moved-in tokens come from nowhere in-hunk: {:?} (run {:?})",
                            f.path,
                            r.new_line.unwrap_or(0),
                            missing,
                            run.text
                        );
                    }
                }
                if r.kind == RowKind::Mod {
                    check!(
                        r.wholesale == is_wholesale_mod(&Row::from(r)),
                        "{}: the wire wholesale flag matches the engine",
                        f.path
                    );
                }
            }
        }
        check!(
            f.counts.adds == adds
                && f.counts.dels == dels
                && f.counts.mods == mods
                && f.counts.movs == movs,
            "{}: header counts {:?} != walked adds {adds} dels {dels} mods {mods} movs {movs}",
            f.path,
            f.counts
        );
        // The fence regions: sorted, non-overlapping, in the file's bounds,
        // and the language is a known profile or the plain style.
        for (i, fence) in f.fences.iter().enumerate() {
            check!(
                fence.start <= fence.end && fence.end <= f.new_line_count as u32,
                "{}: fence {i} out of bounds [{},{}]",
                f.path,
                fence.start,
                fence.end
            );
            if i > 0 {
                check!(
                    f.fences[i - 1].end + 1 < fence.start,
                    "{}: fences overlap (region {i})",
                    f.path
                );
            }
            // The lang is the NORMALIZED profile key the engine emits on
            // the wire (e.g. `rs`, `py`, `md` — never a raw doctest tag like
            // `rust,no_run`; the engine's `fence_lang_ext` resolves those
            // before sending). An empty lang = the plain "code" style.
            // Pin the contract: the lang either resolves to a profile or is
            // empty — a non-empty unknown is a normalization regression.
            assert!(
                fence.lang.is_empty() || lang_profile(&fence.lang).is_some(),
                "{}: fence {i} lang `{}` is not a normalized profile key (got a raw doctest tag?)",
                f.path,
                fence.lang
            );
            let _ = &fence.lang;
        }

        // 1. Verify reconstructed content completeness (only when present — lib_leg
        //    populates these in-memory; bin_leg decodes from wire JSON which omits them
        //    as of the slim format, so we skip these checks when the vecs are empty).
        if !f.new_content.is_empty() && f.status != "deleted" && f.new_line_count > 0 {
            check!(
                f.new_content.len() == f.new_line_count,
                "{}: new_content len {} != new_line_count {}",
                f.path,
                f.new_content.len(),
                f.new_line_count
            );
        }
        if !f.old_content.is_empty() && f.status != "added" && f.old_line_count > 0 {
            check!(
                f.old_content.len() == f.old_line_count,
                "{}: old_content len {} != old_line_count {}",
                f.path,
                f.old_content.len(),
                f.old_line_count
            );
        }

        // 2. Verify visible rows completeness and accuracy (only when present)
        if !f.rows.is_empty() {
            let mut last_new_line: Option<u32> = None;
            for (ri, r) in f.rows.iter().enumerate() {
                if let Some(nl) = r.new_line {
                    if let Some(prev) = last_new_line
                        && nl == prev + 1
                    {
                        // Sequential
                    }
                    last_new_line = Some(nl);
                    if !f.new_content.is_empty() {
                        let expected_text = f
                            .new_content
                            .get((nl as usize) - 1)
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        check!(
                            r.new_text() == expected_text,
                            "{}:{}: visible row {} text {:?} != new_content text {:?}",
                            f.path,
                            nl,
                            ri,
                            r.new_text(),
                            expected_text
                        );
                    }
                } else if let Some(ol) = r.old_line
                    && !f.old_content.is_empty()
                {
                    let expected_text = f
                        .old_content
                        .get((ol as usize) - 1)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    check!(
                        r.old_text() == expected_text,
                        "{}:{}: visible old row {} text {:?} != old_content text {:?}",
                        f.path,
                        ol,
                        ri,
                        r.old_text(),
                        expected_text
                    );
                }

                if r.kind == RowKind::Ctx {
                    check!(
                        r.new_line.is_some() || r.old_line.is_some(),
                        "{}: visible ctx row {} missing line number",
                        f.path,
                        ri
                    );
                    let concat: String = r.runs.iter().map(|x| x.text.as_ref()).collect();
                    check!(
                        concat == r.new_text(),
                        "{}: visible ctx row {} runs concat {:?} != text {:?}",
                        f.path,
                        ri,
                        concat,
                        r.new_text()
                    );
                }
            }
        }
    }
    assert_eq!(
        failures, 0,
        "{failures} golden-walk failure(s) — see the FAIL lines above"
    );
}

const MAX_WIRE_DIFF_FAILURES: usize = 10;

fn print_wire_diff(name: &str, actual: &PipelineOutput, expected: &PipelineOutput) {
    let mut failures: Vec<String> = Vec::new();

    if actual.files.len() != expected.files.len() {
        failures.push(format!(
            "File count mismatch: actual={}, expected={}",
            actual.files.len(),
            expected.files.len()
        ));
    }

    'files: for (fi, (af, ef)) in actual.files.iter().zip(expected.files.iter()).enumerate() {
        if af.path != ef.path {
            failures.push(format!(
                "[file {fi}] Path mismatch: actual={:?}, expected={:?}",
                af.path, ef.path
            ));
            if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                break 'files;
            }
        }
        if af.status != ef.status
            || af.new_line_count != ef.new_line_count
            || af.old_line_count != ef.old_line_count
        {
            failures.push(format!(
                "[file {fi} {:?}] Metadata mismatch: status actual={:?} expected={:?}, \
                 new_line_count actual={} expected={}, old_line_count actual={} expected={}",
                af.path,
                af.status,
                ef.status,
                af.new_line_count,
                ef.new_line_count,
                af.old_line_count,
                ef.old_line_count
            ));
            if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                break 'files;
            }
        }
        if af.hunks.len() != ef.hunks.len() {
            failures.push(format!(
                "[file {fi} {:?}] Hunk count mismatch: actual={}, expected={}",
                af.path,
                af.hunks.len(),
                ef.hunks.len()
            ));
            if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                break 'files;
            }
        }
        'hunks: for (hi, (ah, eh)) in af.hunks.iter().zip(ef.hunks.iter()).enumerate() {
            if ah.rows.len() != eh.rows.len() {
                failures.push(format!(
                    "[file {fi} {:?} hunk {hi}] Row count mismatch: actual={}, expected={}",
                    af.path,
                    ah.rows.len(),
                    eh.rows.len()
                ));
                if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                    break 'files;
                }
            }
            for (ri, (ar, er)) in ah.rows.iter().zip(eh.rows.iter()).enumerate() {
                if ar != er {
                    let mut msg = format!("[file {fi} {:?} hunk {hi} row {ri}] mismatch:", af.path);
                    if ar.kind != er.kind
                        || ar.new_line != er.new_line
                        || ar.old_line != er.old_line
                        || ar.new_text() != er.new_text()
                    {
                        msg.push_str(&format!(
                            "\n    actual:   kind={:?}, nl={:?}, ol={:?}, new_text={:?}",
                            ar.kind,
                            ar.new_line,
                            ar.old_line,
                            ar.new_text()
                        ));
                        msg.push_str(&format!(
                            "\n    expected: kind={:?}, nl={:?}, ol={:?}, new_text={:?}",
                            er.kind,
                            er.new_line,
                            er.old_line,
                            er.new_text()
                        ));
                    }
                    if ar.old_text() != er.old_text() {
                        msg.push_str(&format!(
                            "\n    old_text actual:   {:?}\n    old_text expected: {:?}",
                            ar.old_text(),
                            er.old_text()
                        ));
                    }
                    if ar.spans() != er.spans() {
                        msg.push_str(&format!(
                            "\n    spans actual:   {:?}\n    spans expected: {:?}",
                            ar.spans(),
                            er.spans()
                        ));
                    }
                    if ar.runs != er.runs {
                        msg.push_str(&format!(
                            "\n    runs actual:   {:?}\n    runs expected: {:?}",
                            ar.runs, er.runs
                        ));
                    }
                    if ar.wholesale != er.wholesale {
                        msg.push_str(&format!(
                            "\n    flags actual:   wholesale={}\n    flags expected: wholesale={}",
                            ar.wholesale, er.wholesale
                        ));
                    }
                    failures.push(msg);
                    if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                        break 'hunks;
                    }
                }
            }
        }
        if af.fences != ef.fences {
            failures.push(format!(
                "[file {fi} {:?}] fences mismatch: actual={:?}, expected={:?}",
                af.path, af.fences, ef.fences
            ));
            if failures.len() >= MAX_WIRE_DIFF_FAILURES {
                break 'files;
            }
        }
    }

    let total = failures.len();
    let shown = total.min(MAX_WIRE_DIFF_FAILURES);
    eprintln!("\n----- WIRE DIFF [{name}] — {total} failure(s), showing first {shown} -----");
    for f in failures.iter().take(MAX_WIRE_DIFF_FAILURES) {
        eprintln!("  {f}");
    }
    if total > MAX_WIRE_DIFF_FAILURES {
        eprintln!(
            "  ... and {} more (increase MAX_WIRE_DIFF_FAILURES to see all)",
            total - MAX_WIRE_DIFF_FAILURES
        );
    }
    eprintln!("----- END WIRE DIFF -----\n");
}

static EXPECTED_WIRE_CACHE: LazyLock<Mutex<HashMap<String, Arc<PipelineOutput>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn load_expected_wire(name: &str) -> Arc<PipelineOutput> {
    {
        let cache = EXPECTED_WIRE_CACHE.lock().unwrap();
        if let Some(out) = cache.get(name) {
            return Arc::clone(out);
        }
    }
    let wire_jsonl_path = expected_wire_jsonl_path(name);
    let gz_wire_jsonl_path = golden_dir().join(format!("{name}.wire.jsonl.gz"));
    let files = if wire_jsonl_path.exists() {
        let file = fs::File::open(&wire_jsonl_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected wire JSONL for {name} from {}: {e}",
                wire_jsonl_path.display()
            )
        });
        diffuse::core::wire::jsonl::decode_jsonl_reader(BufReader::new(file))
            .unwrap_or_else(|e| panic!("[{name}] failed to parse expected wire JSONL: {e}"))
    } else {
        let gz_bytes = fs::read(&gz_wire_jsonl_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected wire JSONL for {name} from {} or {}: {e}",
                wire_jsonl_path.display(),
                gz_wire_jsonl_path.display()
            )
        });
        let decoder = flate2::read::GzDecoder::new(&gz_bytes[..]);
        diffuse::core::wire::jsonl::decode_jsonl_reader(BufReader::new(decoder))
            .unwrap_or_else(|e| panic!("[{name}] failed to parse expected wire JSONL gz: {e}"))
    };
    let parsed = PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    };
    let arc = Arc::new(parsed);
    let mut cache = EXPECTED_WIRE_CACHE.lock().unwrap();
    cache.insert(name.to_string(), Arc::clone(&arc));
    arc
}

fn wire_files_match(a: &OutputFile, b: &OutputFile) -> bool {
    a.path == b.path
        && a.status == b.status
        && a.new_line_count == b.new_line_count
        && a.old_line_count == b.old_line_count
        && a.hunks == b.hunks
        && a.counts == b.counts
        && a.fences == b.fences
        && a.notes == b.notes
}

fn wire_outputs_match(a: &PipelineOutput, b: &PipelineOutput) -> bool {
    a.branch == b.branch
        && a.untracked_skipped == b.untracked_skipped
        && a.files.len() == b.files.len()
        && a.files
            .iter()
            .zip(&b.files)
            .all(|(fa, fb)| wire_files_match(fa, fb))
}

fn expected_wire_jsonl_path(name: &str) -> PathBuf {
    golden_dir().join(name).join("outputs").join("wire.jsonl")
}

fn assert_wire_json_matches(output: &PipelineOutput, name: &str) {
    let wire_jsonl_path = expected_wire_jsonl_path(name);
    let gz_wire_jsonl_path = golden_dir().join(format!("{name}.wire.jsonl.gz"));

    if env::var("GEN_GOLDEN").is_ok() || env::var("UPDATE_WIRE").is_ok() {
        let mut jsonl_buf = Vec::new();
        for file in &output.files {
            diffuse::core::wire::jsonl::emit_file_jsonl(file, &mut jsonl_buf)
                .expect("emit_file_jsonl");
        }

        if gz_wire_jsonl_path.exists() {
            write_gz(&gz_wire_jsonl_path, &jsonl_buf).unwrap_or_else(|e| {
                panic!("failed to write {}: {e}", gz_wire_jsonl_path.display())
            });
        } else {
            write_file_plain(&wire_jsonl_path, &jsonl_buf)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", wire_jsonl_path.display()));
        }
        return;
    }
    let expected = load_expected_wire(name);
    if !wire_outputs_match(output, expected.as_ref()) {
        print_wire_diff(name, output, expected.as_ref());
        panic!(
            "[{name}] wire JSONL output deviates from expected static golden ({}). See wire diff above. Use GEN_GOLDEN=1 or UPDATE_WIRE=1 to update.",
            wire_jsonl_path.display()
        );
    }
}

fn print_render_diff(name: &str, actual: &str, expected: &str) {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    for (idx, (exp, act)) in expected_lines.iter().zip(actual_lines.iter()).enumerate() {
        if exp != act {
            eprintln!(
                "\nFirst mismatch at line {}:\nEXP: {exp}\nACT: {act}\n",
                idx + 1
            );
            break;
        }
    }
    eprintln!("\n----- RENDER DIFF [{name}] (first 30 lines) -----");
    let diff = difflib::unified_diff(
        &expected_lines,
        &actual_lines,
        &format!("{name}.render.txt (expected)"),
        &format!("{name}.render.txt (actual)"),
        "",
        "",
        3,
    );
    for (i, line) in diff.into_iter().enumerate() {
        if i >= 30 {
            eprintln!("... [diff truncated at 30 lines] ...");
            break;
        }
        eprintln!("{line}");
    }
    eprintln!("----- END RENDER DIFF -----\n");
}

fn assert_render_flavor_matches(actual_encoded: &str, name: &str, flavor: &'static str) {
    let render_path = expected_render_path(name, flavor);
    let gz_render_path = golden_dir().join(format!("{name}.render.{flavor}.txt.gz"));

    if env::var("GEN_GOLDEN").is_ok()
        || env::var("UPDATE_RENDER").is_ok()
        || (flavor == "raw" && env::var("UPDATE_RAW").is_ok())
    {
        if gz_render_path.exists() {
            write_gz(&gz_render_path, actual_encoded.as_bytes())
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", gz_render_path.display()));
        } else {
            write_file_plain(&render_path, actual_encoded.as_bytes())
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", render_path.display()));
        }
        return;
    }

    let expected_encoded = load_expected_render(name, flavor);
    if actual_encoded != *expected_encoded.as_ref() {
        print_render_diff(
            &format!("{name}.render.{flavor}"),
            actual_encoded,
            expected_encoded.as_ref(),
        );
        panic!(
            "[{name}] {flavor} render output deviates from expected static golden ({}). See render diff above. Use GEN_GOLDEN=1 or UPDATE_RENDER=1 to update.",
            render_path.display()
        );
    }
}

fn assert_render_matches(output: &PipelineOutput, name: &str) {
    let dark_theme = default_theme();
    let actual_dark_raw = render_pipeline_output(output, &dark_theme, true);
    let actual_dark_encoded = encode_sgr(&actual_dark_raw);
    assert_render_flavor_matches(&actual_dark_encoded, name, "dark");

    let light_theme = DiffuseTheme::LIGHT;
    let actual_light_raw = render_pipeline_output(output, &light_theme, true);
    let actual_light_encoded = encode_sgr(&actual_light_raw);
    assert_render_flavor_matches(&actual_light_encoded, name, "light");

    let raw_theme = DiffuseTheme::DEFAULT;
    let actual_raw = render_raw_pipeline_output(output, &raw_theme);
    let actual_raw_encoded = encode_sgr(&actual_raw);
    assert_render_flavor_matches(&actual_raw_encoded, name, "raw");
}

fn print_markdown_diff(name: &str, actual: &str, expected: &str) {
    eprintln!("\n----- MARKDOWN DIFF [{name}] (first 30 lines) -----");
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let diff = difflib::unified_diff(
        &expected_lines,
        &actual_lines,
        &format!("{name}.markdown.md (expected)"),
        &format!("{name}.markdown.md (actual)"),
        "",
        "",
        3,
    );
    for (i, line) in diff.into_iter().enumerate() {
        if i >= 30 {
            eprintln!("... [diff truncated at 30 lines] ...");
            break;
        }
        eprintln!("{line}");
    }
    eprintln!("----- END MARKDOWN DIFF -----\n");
}

fn assert_markdown_matches(output: &PipelineOutput, name: &str) {
    let opts = diffuse::render::MarkdownOptions::default();
    let actual_md = diffuse::render::render_markdown_output(output, &opts);
    let md_path = expected_markdown_path(name);
    let gz_md_path = golden_dir().join(format!("{name}.markdown.md.gz"));

    if env::var("GEN_GOLDEN").is_ok()
        || env::var("UPDATE_MARKDOWN").is_ok()
        || env::var("UPDATE_MD").is_ok()
    {
        if gz_md_path.exists() {
            write_gz(&gz_md_path, actual_md.as_bytes())
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", gz_md_path.display()));
        } else {
            write_file_plain(&md_path, actual_md.as_bytes())
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", md_path.display()));
        }
        return;
    }

    let expected_md = if md_path.exists() {
        fs::read_to_string(&md_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected markdown output for {name} from {}: {e}. Use GEN_GOLDEN=1 or UPDATE_MARKDOWN=1 to create/update.",
                md_path.display()
            )
        })
    } else {
        let bytes = fs::read(&gz_md_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected markdown output for {name} from {} or {}: {e}.",
                md_path.display(),
                gz_md_path.display()
            )
        });
        let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut s = String::new();
        Read::read_to_string(&mut decoder, &mut s).unwrap();
        s
    };

    if actual_md != expected_md {
        print_markdown_diff(name, &actual_md, &expected_md);
        panic!(
            "[{name}] markdown output deviates from expected static golden ({}). See markdown diff above. Use GEN_GOLDEN=1 or UPDATE_MARKDOWN=1 to update.",
            md_path.display()
        );
    }
}

fn run_bin_markdown_leg(name: &str) {
    let fixture = load_fixture(name);
    let input = serde_json::to_vec(fixture.as_ref()).expect("serialize fixture");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_diffuse"));
    cmd.arg("-m").arg("--color=never");
    if let Some(ref notes_path) = fixture.notes_file {
        cmd.arg("--notes-file").arg(notes_path);
    }
    if let Some(ref repo_key) = fixture.repo_key {
        cmd.arg("--repo-key").arg(repo_key);
    }
    if let Some(ref rev_key) = fixture.rev_key {
        cmd.arg("--rev-key").arg(rev_key);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the diffuse binary in markdown mode");
    {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&input)
            .expect("write the fixture");
    }
    let out = child.wait_with_output().expect("wait for the bin");
    assert!(
        out.status.success(),
        "[{name}] the bin exited non-zero in markdown mode: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin_md = String::from_utf8_lossy(&out.stdout).into_owned();
    let md_path = expected_markdown_path(name);

    if env::var("GEN_GOLDEN").is_ok()
        || env::var("UPDATE_MARKDOWN").is_ok()
        || env::var("UPDATE_MD").is_ok()
    {
        return;
    }

    let expected_md = if md_path.exists() {
        fs::read_to_string(&md_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected markdown output for {name} from {}: {e}. Use GEN_GOLDEN=1 or UPDATE_MARKDOWN=1 to create/update.",
                md_path.display()
            )
        })
    } else {
        let gz_path = golden_dir().join(format!("{name}.markdown.md.gz"));
        let bytes = fs::read(&gz_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read expected markdown output for {name} from {} or {}: {e}.",
                md_path.display(),
                gz_path.display()
            )
        });
        let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut s = String::new();
        Read::read_to_string(&mut decoder, &mut s).unwrap();
        s
    };

    if bin_md != expected_md {
        print_markdown_diff(name, &bin_md, &expected_md);
        panic!(
            "[{name}] bin markdown output deviates from expected static golden ({}). See markdown diff above.",
            md_path.display()
        );
    }
}

fn run_bin_raw_leg(name: &str) {
    if env::var("GEN_GOLDEN").is_ok()
        || env::var("UPDATE_RENDER").is_ok()
        || env::var("UPDATE_RAW").is_ok()
    {
        return;
    }
    let fixture = load_fixture(name);
    let input = serde_json::to_vec(fixture.as_ref()).expect("serialize fixture");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_diffuse"));
    cmd.arg("--raw").arg("--color=always");
    if let Some(ref notes_path) = fixture.notes_file {
        cmd.arg("--notes-file").arg(notes_path);
    }
    if let Some(ref repo_key) = fixture.repo_key {
        cmd.arg("--repo-key").arg(repo_key);
    }
    if let Some(ref rev_key) = fixture.rev_key {
        cmd.arg("--rev-key").arg(rev_key);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the diffuse binary in --raw mode");

    let mut stdin = child.stdin.take().expect("stdin");
    let writer_thread = thread::spawn(move || {
        let _ = stdin.write_all(&input);
        let _ = stdin.flush();
        drop(stdin);
    });

    let out = child.wait_with_output().expect("wait for the bin");
    let _ = writer_thread.join();
    assert!(
        out.status.success(),
        "[{name}] the bin exited non-zero in raw mode: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin_raw = String::from_utf8_lossy(&out.stdout);
    let actual_encoded = encode_sgr(&bin_raw);
    let raw_path = expected_render_path(name, "raw");
    let expected_encoded = load_expected_render(name, "raw");

    if actual_encoded != *expected_encoded.as_ref() {
        print_render_diff(
            &format!("{name}.render.raw (bin)"),
            &actual_encoded,
            expected_encoded.as_ref(),
        );
        panic!(
            "[{name}] bin raw output deviates from expected static golden ({}). See render diff above.",
            raw_path.display()
        );
    }
}

fn init_test_logging() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = diffuse::core::logging::init_tracing_subscriber();
    });
}

fn is_generating() -> bool {
    ["GEN_GOLDEN", "UPDATE_WIRE", "UPDATE_RENDER", "UPDATE_RAW"]
        .iter()
        .any(|key| env::var(key).is_ok())
}

/// Run the pipeline one file at a time so a panic names the offending path.
/// Used only as a diagnostic when normal golden generation panics; the
/// normal path exercises the streaming pipeline.
fn run_pipeline_for_generation(fixture: &Fixture, opts: PipelineOptions) -> PipelineOutput {
    let mut new_content = fixture.new_content.clone();
    let mut old_content = fixture.old_content.clone();
    let mut files = Vec::new();
    diffuse::core::parse::parse_diff_for_each(&fixture.diff, |f| {
        let path = f.path.clone();
        let new_c = new_content.remove(&path);
        let old_lookup = f.old_path.clone().unwrap_or_else(|| path.clone());
        let old_c = old_content
            .remove(&old_lookup)
            .or_else(|| old_content.remove(&path));
        let diff_fallback = Some(Arc::clone(&fixture.diff));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_single_diff_file(
                f,
                new_c.as_deref().map(|s| s.as_str()),
                old_c.as_deref().map(|s| s.as_str()),
                diff_fallback,
                &opts,
            )
        }));
        match result {
            Ok(file) => files.push(file),
            Err(payload) => {
                eprintln!("PANIC while generating file {path:?}");
                std::panic::resume_unwind(payload);
            }
        }
    });
    PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

fn run_lib_leg(name: &str, num_files: usize) -> (Arc<Fixture>, PipelineOutput) {
    init_test_logging();
    let t0 = Instant::now();
    let fixture = load_fixture(name);
    let _t_load = t0.elapsed();
    let new_content = if fixture.new_content.is_empty() {
        None
    } else {
        Some(fixture.new_content.clone())
    };
    let old_content = if fixture.old_content.is_empty() {
        None
    } else {
        Some(fixture.old_content.clone())
    };
    let mut opts = diffuse::core::pipeline::PipelineOptions::default();
    if let Some(ref db) = fixture.notes_db {
        opts = opts.with_notes(
            Some(db.clone()),
            fixture.repo_key.clone(),
            fixture.rev_key.clone(),
        );
    }
    let output = if env::var("GEN_GOLDEN_FILE_DIAG").is_ok() {
        run_pipeline_for_generation(&fixture, opts)
    } else if is_generating() {
        let normal_opts = opts.clone();
        let normal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            diffuse::core::pipeline::run_pipeline_arc(
                Arc::clone(&fixture.diff),
                new_content,
                old_content,
                normal_opts,
            )
        }));
        match normal {
            Ok(output) => output,
            Err(payload) => {
                eprintln!("Golden generation panicked for corpus {name:?}; locating the file...");
                // Re-run one file at a time to name the offending path. The
                // diagnostic walk panics on the first bad file after printing
                // its path, so silence the duplicate hook output and resume
                // the original panic below.
                let previous_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(|_| {}));
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_pipeline_for_generation(&fixture, opts)
                }));
                std::panic::set_hook(previous_hook);
                std::panic::resume_unwind(payload)
            }
        }
    } else {
        diffuse::core::pipeline::run_pipeline_arc(
            Arc::clone(&fixture.diff),
            new_content,
            old_content,
            opts,
        )
    };
    assert_wire_json_matches(&output, name);
    walk_and_assert(&output, name, num_files);
    assert_diff_invariants(
        &fixture.diff,
        &output,
        name,
        &fixture.old_content,
        &fixture.new_content,
    );
    (fixture, output)
}

fn run_bin_leg(name: &str, num_files: usize) {
    if env::var("GEN_GOLDEN").is_ok() || env::var("UPDATE_WIRE").is_ok() {
        // Regenerate the wire golden from the binary's actual output so that
        // the golden always reflects what the compiled binary emits, not the
        // library leg.  Bailing out here (as before) left the golden written
        // by the library leg, which diverges whenever the two differ.
        let fixture = load_fixture(name);
        let input = serde_json::to_vec(fixture.as_ref()).expect("serialize fixture");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_diffuse"));
        cmd.arg("--jsonl");
        if let Some(ref notes_path) = fixture.notes_file {
            cmd.arg("--notes-file").arg(notes_path);
        }
        if let Some(ref repo_key) = fixture.repo_key {
            cmd.arg("--repo-key").arg(repo_key);
        }
        if let Some(ref rev_key) = fixture.rev_key {
            cmd.arg("--rev-key").arg(rev_key);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the diffuse binary in --jsonl mode (GEN_GOLDEN)");

        let mut stdin = child.stdin.take().expect("stdin");
        let writer_thread = thread::spawn(move || {
            let _ = stdin.write_all(&input);
            let _ = stdin.flush();
            drop(stdin);
        });

        let out = child
            .wait_with_output()
            .expect("wait for binary (GEN_GOLDEN)");
        let _ = writer_thread.join();
        assert!(
            out.status.success(),
            "[{name}] the bin exited non-zero during GEN_GOLDEN: {:?}",
            out.status
        );

        let wire_jsonl_path = expected_wire_jsonl_path(name);
        let gz_wire_jsonl_path = golden_dir().join(format!("{name}.wire.jsonl.gz"));
        if gz_wire_jsonl_path.exists() {
            write_gz(&gz_wire_jsonl_path, &out.stdout).unwrap_or_else(|e| {
                panic!("failed to write {}: {e}", gz_wire_jsonl_path.display())
            });
        } else {
            write_file_plain(&wire_jsonl_path, &out.stdout)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", wire_jsonl_path.display()));
        }
        return;
    }
    let fixture = load_fixture(name);
    let input = serde_json::to_vec(fixture.as_ref()).expect("serialize fixture");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_diffuse"));
    cmd.arg("--jsonl");
    if let Some(ref notes_path) = fixture.notes_file {
        cmd.arg("--notes-file").arg(notes_path);
    }
    if let Some(ref repo_key) = fixture.repo_key {
        cmd.arg("--repo-key").arg(repo_key);
    }
    if let Some(ref rev_key) = fixture.rev_key {
        cmd.arg("--rev-key").arg(rev_key);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the diffuse binary in --jsonl mode");

    let mut stdin = child.stdin.take().expect("stdin");
    let writer_thread = thread::spawn(move || {
        let _ = stdin.write_all(&input);
        let _ = stdin.flush();
        drop(stdin);
    });

    let wire_jsonl_path = expected_wire_jsonl_path(name);
    let gz_wire_jsonl_path = golden_dir().join(format!("{name}.wire.jsonl.gz"));

    let expected_reader: Box<dyn BufRead> = if wire_jsonl_path.exists() {
        let expected_file = fs::File::open(&wire_jsonl_path).unwrap_or_else(|e| {
            panic!(
                "[{name}] failed to open expected wire.jsonl at {}: {e}",
                wire_jsonl_path.display()
            )
        });
        Box::new(BufReader::new(expected_file))
    } else {
        let gz_file = fs::File::open(&gz_wire_jsonl_path).unwrap_or_else(|e| {
            panic!(
                "[{name}] failed to open expected wire.jsonl at {} or {}: {e}",
                wire_jsonl_path.display(),
                gz_wire_jsonl_path.display()
            )
        });
        let decoder = flate2::read::GzDecoder::new(gz_file);
        Box::new(BufReader::new(decoder))
    };
    let mut expected_reader = expected_reader;
    let mut actual_reader = BufReader::new(child.stdout.take().expect("stdout"));

    let mut failures = Vec::new();
    let mut line_idx = 0;
    let mut expected_line = String::new();
    let mut actual_line = String::new();
    let mut records = Vec::new();

    loop {
        expected_line.clear();
        actual_line.clear();
        let exp_len = expected_reader.read_line(&mut expected_line).unwrap_or(0);
        let act_len = actual_reader.read_line(&mut actual_line).unwrap_or(0);

        if exp_len == 0 && act_len == 0 {
            break;
        }

        line_idx += 1;

        // Record at most MAX_WIRE_DIFF_FAILURES mismatches, but always drain
        // both streams to EOF: the child blocks writing once the stdout pipe
        // fills, so an early break here would deadlock the `child.wait()`
        // below while the child still has output in flight.
        let recording = failures.len() < MAX_WIRE_DIFF_FAILURES;

        if exp_len == 0 {
            if recording {
                failures.push(format!(
                    "Line {line_idx}: unexpected extra output from binary: {}",
                    actual_line.trim()
                ));
            }
            continue;
        }
        if act_len == 0 {
            if recording {
                failures.push(format!(
                    "Line {line_idx}: unexpected EOF from binary, expected: {}",
                    expected_line.trim()
                ));
            }
            continue;
        }

        let exp_trimmed = expected_line.trim();
        let act_trimmed = actual_line.trim();

        if exp_trimmed != act_trimmed && recording {
            failures.push(format!(
                "Line {line_idx} mismatch:\n  actual:   {act_trimmed}\n  expected: {exp_trimmed}"
            ));
        }

        if let Ok(rec) = serde_json::from_str::<diffuse::core::wire::jsonl::WireRecord>(act_trimmed)
        {
            records.push(rec);
        }
    }

    let _ = writer_thread.join();
    let status = child.wait().expect("wait for child");
    assert!(
        status.success(),
        "[{name}] the bin exited non-zero with status {status:?}"
    );

    if !failures.is_empty() {
        eprintln!(
            "\n----- WIRE JSONL DIFF [{name}] — showing first {} mismatch(es) -----",
            failures.len()
        );
        for f in &failures {
            eprintln!("  {f}");
        }
        eprintln!("----- END WIRE JSONL DIFF -----\n");
        panic!("[{name}] wire JSONL output deviates from expected static golden. See diff above.");
    }

    let decoded_files =
        diffuse::core::wire::jsonl::decode_jsonl_records(records).expect("decode records");
    let decoded = PipelineOutput {
        files: decoded_files,
        branch: String::new(),
        untracked_skipped: vec![],
    };
    walk_and_assert(&decoded, name, num_files);
    let bin_fixture = load_fixture(name);
    assert_diff_invariants(
        &bin_fixture.diff,
        &decoded,
        name,
        &bin_fixture.old_content,
        &bin_fixture.new_content,
    );
}

/// Slice a single path's portion out of a multi-file diff: from its
/// `diff --git a/<p> b/<p>` header through the next `diff --git` or the end.
fn file_diff_slice(diff: &str, path: &str) -> String {
    let mut si = None;
    let b_suffix = format!(" b/{path}");
    let a_prefix = format!("diff --git a/{path} ");
    for line in diff.split('\n') {
        if line.starts_with("diff --git ")
            && (line.ends_with(&b_suffix) || line.starts_with(&a_prefix))
        {
            let offset = (line.as_ptr() as usize).saturating_sub(diff.as_ptr() as usize);
            si = Some(offset);
            break;
        }
    }
    let si = match si {
        Some(i) => i,
        None => return String::new(),
    };
    let marker_len = 11; // "diff --git "
    let ni = diff[si + marker_len..]
        .find("\ndiff --git ")
        .map(|j| si + marker_len + j + 1)
        .unwrap_or(diff.len());
    diff[si..ni].to_string()
}

fn run_oracle_for_corpus(name: &str, fixture: &Fixture, wire: &PipelineOutput) {
    for (path, new) in &fixture.new_content {
        let Some(old) = fixture.old_content.get(path) else {
            continue;
        };
        let slice = file_diff_slice(&fixture.diff, path);
        assert!(!slice.is_empty(), "{name}: no diff slice for {path}");
        let old_vec: Vec<String> = old.split('\n').map(String::from).collect();
        // The validity predicate is apply(resurface(tape), old) == new: a
        // whitespace-insensitive tape suppresses whitespace-only changes
        // (its context rows carry one side's text), so the tape applies
        // cleanly only after the resurface pass — a no-op for
        // default-generated tapes.
        let mut resurfaced = diffuse::core::parse::parse_diff(&slice);
        let old_refs: Vec<&str> = old_vec.iter().map(String::as_str).collect();
        let new_refs: Vec<&str> = new.split('\n').collect();
        for f in resurfaced.iter_mut() {
            diffuse::core::resurface::resurface_hunks(&mut f.hunks, &old_refs, &new_refs);
        }
        let hunks: Vec<diffuse::core::parse::Hunk> = resurfaced
            .iter()
            .flat_map(|f| f.hunks.iter().cloned())
            .collect();
        let applied = apply_hunk_rows(&hunks, &old_vec);
        let expected: Vec<String> = new.split('\n').map(String::from).collect();
        assert_eq!(
            applied, expected,
            "[{name}:{path}] apply(resurface(file_diff), old) != new — the fixture's diff is not a valid transformation"
        );
    }

    assert_eq!(
        wire.reverse().reverse(),
        *wire,
        "[{name}] wire.reverse().reverse() == wire"
    );

    if name.starts_with("opt-") {
        let wire_rev = wire.reverse();
        let reconstructed = wire.reconstruct();
        let reconstructed_rev = wire_rev.reconstruct();

        for (path, new) in &fixture.new_content {
            let Some(old_str) = fixture.old_content.get(path) else {
                continue;
            };
            let old_vec: Vec<String> = old_str.split('\n').map(String::from).collect();
            let expected: Vec<String> = new.split('\n').map(String::from).collect();

            if let Some(f_reconstructed) = reconstructed.iter().find(|f| f.path == *path) {
                let hunks = if !f_reconstructed.raw_hunks.is_empty() {
                    &f_reconstructed.raw_hunks
                } else {
                    &f_reconstructed.hunks
                };
                let applied_forward = apply_hunk_rows(hunks, &old_vec);
                assert_eq!(
                    applied_forward, expected,
                    "[{name}:{path}] apply(old, wire.reconstruct()) != new"
                );
            }

            if let Some(f_rev_reconstructed) = reconstructed_rev.iter().find(|f| f.path == *path) {
                let hunks_rev = if !f_rev_reconstructed.raw_hunks.is_empty() {
                    &f_rev_reconstructed.raw_hunks
                } else {
                    &f_rev_reconstructed.hunks
                };
                let applied_backward = apply_hunk_rows(hunks_rev, &expected);
                assert_eq!(
                    applied_backward, old_vec,
                    "[{name}:{path}] apply(new, wire.reverse().reconstruct()) != old"
                );
            }
        }
    }
}

fn run_acceptance_test_impl(is_inited: bool, name: &str, num_files: usize) -> bool {
    if !is_inited {
        eprintln!("tests/golden submodule is not initialized; skipping {name} golden_acceptance");
        return false;
    }
    let (fixture, output) = run_lib_leg(name, num_files);
    assert_render_matches(&output, name);
    assert_markdown_matches(&output, name);
    run_oracle_for_corpus(name, &fixture, &output);
    true
}

fn run_acceptance_test(name: &str, num_files: usize) {
    run_acceptance_test_impl(is_golden_initialized(), name, num_files);
}

fn run_binary_test_impl(is_inited: bool, name: &str, num_files: usize) -> bool {
    if !is_inited {
        eprintln!("tests/golden submodule is not initialized; skipping {name} bin_leg");
        return false;
    }
    run_bin_leg(name, num_files);
    run_bin_markdown_leg(name);
    run_bin_raw_leg(name);
    true
}

fn run_binary_test(name: &str, num_files: usize) {
    run_binary_test_impl(is_golden_initialized(), name, num_files);
}

macro_rules! corpus_tests {
    ($mod_name:ident, $name:expr, $num_files:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn golden_acceptance() {
                run_acceptance_test($name, $num_files);
            }

            #[test]
            fn bin_leg() {
                run_binary_test($name, $num_files);
            }
        }
    };
    (ignored $mod_name:ident, $name:expr, $num_files:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore = "massive history stress corpus (run with cargo test -- --ignored)"]
            fn lib_leg() {
                if !is_golden_initialized() {
                    eprintln!(
                        "tests/golden submodule is not initialized; skipping {} lib_leg",
                        $name
                    );
                    return;
                }
                run_lib_leg($name, $num_files);
            }

            #[test]
            #[ignore = "massive history stress corpus (run with cargo test -- --ignored)"]
            fn bin_leg() {
                if !is_golden_initialized() {
                    eprintln!(
                        "tests/golden submodule is not initialized; skipping {} bin_leg",
                        $name
                    );
                    return;
                }
                run_bin_leg($name, $num_files);
            }
        }
    };
}

corpus_tests!(diffuse_2026_08, "diffuse-2026-08", 17);
corpus_tests!(opt_1, "opt-1-cache-regex-window-slice", 5);
corpus_tests!(opt_2, "opt-2-cache-markdown-regex", 2);
corpus_tests!(opt_3, "opt-3-cache-lang-profile", 3);
corpus_tests!(opt_4, "opt-4-manual-scanner-prefilter", 4);
corpus_tests!(diffuse_history_mid, "diffuse-history-mid", 203);
corpus_tests!(untracked_files, "untracked-files", 2);
corpus_tests!(theme_scrollbar_palette, "theme-scrollbar-palette", 34);
corpus_tests!(theme_light_toggle, "theme-light-toggle", 42);
corpus_tests!(row_groups_contrast, "row-groups-contrast", 29);
corpus_tests!(rust_lifetime_char, "rust-lifetime-char", 100);
corpus_tests!(review_notes, "review-notes", 16);
corpus_tests!(fn_return_tail, "fn-return-tail", 4);
corpus_tests!(osc11_query_gate, "osc11-query-gate", 6);
corpus_tests!(ansi_optimizer, "ansi-optimizer", 10);
corpus_tests!(linenum_u32, "linenum-u32", 26);
corpus_tests!(scrollbar_grab_drag, "scrollbar-grab-drag", 7);
corpus_tests!(viewport_scroll_boundaries, "viewport-scroll-boundaries", 6);
corpus_tests!(container_compatibility, "container-compatibility", 9);
corpus_tests!(diffcoord_anchoring, "diffcoord-anchoring", 38);

#[test]
fn test_golden_initialized_detection() {
    let tmp = env::temp_dir().join(format!("diffuse_test_uninited_{}", process::id()));
    let _ = fs::remove_dir_all(&tmp);

    // Nonexistent directory
    assert!(!is_golden_initialized_at(&tmp));

    // Empty directory
    fs::create_dir_all(&tmp).unwrap();
    assert!(!is_golden_initialized_at(&tmp));

    // Directory with dummy file
    fs::write(tmp.join("README.md"), "hello").unwrap();
    assert!(!is_golden_initialized_at(&tmp));

    // Directory with expected corpus fixture
    let corpus = tmp.join("diffuse-2026-08");
    fs::create_dir_all(&corpus).unwrap();
    fs::write(corpus.join("meta.json"), "{}").unwrap();
    assert!(is_golden_initialized_at(&tmp));

    let _ = fs::remove_dir_all(&tmp);

    // Verify uninitialized skip logic executes cleanly without panic
    assert!(!run_acceptance_test_impl(false, "test-corpus", 1));
    assert!(!run_binary_test_impl(false, "test-corpus", 1));
}
