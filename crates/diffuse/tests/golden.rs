//! The golden acceptance — TWO legs over the frozen corpus
//! (`tests/golden/mirage-2025-08.json`), both walking the wire-contract
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
    fs::{self},
    process::{Command, Stdio},
    sync::{Arc, LazyLock, Mutex},
};

use serde::{Deserialize, Serialize};

use diffuse::{
    core::{
        apply::{apply_file_diff, apply_hunk_rows},
        lcs::char_len,
        merge::{RunKind, display_text, is_wholesale_mod},
        parse::{Row, RowKind},
        wire::{OutputFile, PipelineOutput},
    },
    render::{default_theme, encode_sgr, render_pipeline_output},
    syntax::registry::lang_profile,
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    diff: String,
    #[serde(default, alias = "contents")]
    new_content: HashMap<String, String>,
    /// The OLD file contents (the opt-* commits carry this; the original
    /// corpora don't). The apply-oracle (PLAN.md §4 Phase 6) uses it to
    /// prove `apply(diff, old) == new` mechanically — the diff is a semantic
    /// program, not just a render.
    #[serde(default, alias = "old_contents")]
    old_content: HashMap<String, String>,
}

use std::path::{Path, PathBuf};

fn golden_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden"))
}

fn write_file_plain(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;
    Ok(())
}

static FIXTURE_CACHE: LazyLock<Mutex<HashMap<String, Arc<Fixture>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static EXPECTED_RENDER_CACHE: LazyLock<Mutex<HashMap<String, Arc<String>>>> =
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
    files: Vec<MetaFileRecord>,
}

fn load_fixture(name: &str) -> Arc<Fixture> {
    let mut cache = FIXTURE_CACHE.lock().unwrap();
    if let Some(f) = cache.get(name) {
        return Arc::clone(f);
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
    for record in meta.files {
        if let Some(new_file) = record.new {
            let new_path = files_dir.join(&new_file);
            let content = fs::read_to_string(&new_path)
                .unwrap_or_else(|e| panic!("failed reading {}: {e}", new_path.display()));
            new_content.insert(record.path.clone(), content);
        }
        if let Some(old_file) = record.old {
            let old_path = files_dir.join(&old_file);
            let content = fs::read_to_string(&old_path)
                .unwrap_or_else(|e| panic!("failed reading {}: {e}", old_path.display()));
            old_content.insert(record.path, content);
        }
    }
    let f = Fixture {
        diff,
        new_content,
        old_content,
    };
    let arc = Arc::new(f);
    cache.insert(name.to_string(), Arc::clone(&arc));
    arc
}

fn expected_wire_path(name: &str) -> PathBuf {
    golden_dir().join(name).join("wire.json")
}

fn expected_render_path(name: &str) -> PathBuf {
    golden_dir().join(name).join("render.txt")
}

fn expected_markdown_path(name: &str) -> PathBuf {
    golden_dir().join(name).join("markdown.md")
}

fn load_expected_render(name: &str) -> Arc<String> {
    let mut cache = EXPECTED_RENDER_CACHE.lock().unwrap();
    if let Some(r) = cache.get(name) {
        return Arc::clone(r);
    }
    let render_path = expected_render_path(name);
    let expected = fs::read_to_string(&render_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read expected render output for {name} from {}: {e}",
            render_path.display()
        )
    });
    let arc = Arc::new(expected);
    cache.insert(name.to_string(), Arc::clone(&arc));
    arc
}

/// The walk: every invariant over the whole corpus. A failure increments
/// `failures` and prints the exact row; the caller asserts zero at the end.
fn walk_and_assert(out: &PipelineOutput, corpus: &str, min_files: usize) {
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
        out.files.len() >= min_files,
        "corpus parses into {min_files}+ files, got {}",
        out.files.len()
    );
    for f in &out.files {
        check!(!f.path.is_empty(), "a file has an empty path");
        // The header counts must equal a re-walk of the rows
        // (moves are neither added nor removed; mods are one row).
        let mut adds = 0usize;
        let mut dels = 0usize;
        let mut mods = 0usize;
        let mut movs = 0usize;
        for h in &f.hunks {
            for r in &h.rows {
                match r.kind {
                    RowKind::Add if r.moved => movs += 1,
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
                    RowKind::Mod => {
                        check!(
                            r.new_line.is_some() && r.old_line.is_some(),
                            "{}: mod row has newLine {:?} and oldLine {:?}",
                            f.path,
                            r.new_line,
                            r.old_line
                        );
                        check!(r.del_text.is_some(), "{}: mod row carries delText", f.path);
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
                let text_chars = char_len(&r.text);
                for &(s, e) in &r.spans.add {
                    check!(
                        s <= e && e <= text_chars,
                        "{}: add span {s}..{e} beyond text len {text_chars}",
                        f.path
                    );
                }
                if let Some(dt) = &r.del_text {
                    let del_chars = char_len(dt);
                    for &(s, e) in &r.spans.del {
                        check!(
                            s <= e && e <= del_chars,
                            "{}: del span {s}..{e} beyond delText len {del_chars}",
                            f.path
                        );
                    }
                } else {
                    check!(
                        r.spans.del.is_empty(),
                        "{}: del spans without delText",
                        f.path
                    );
                }
                // The runs concatenate to the display text; del runs carry
                // no color (the strike is the renderer's), common/add do.
                let concat: String = r.runs.iter().map(|x| x.text.as_str()).collect();
                let expected = display_text(&Row::from(r));
                check!(
                    concat == expected,
                    "{}: runs concatenate to the display text ({} chars vs {})",
                    f.path,
                    concat.chars().count(),
                    expected.chars().count()
                );
                // The DISK-EXACT invariant: removing the struck (del) content
                // from the render must leave exactly the row's new-side line —
                // what a user would see on disk. The merge guarantees it (the
                // common/add runs cover the new text; the del runs are the
                // The DISK-EXACT invariant: removing the struck (del) content
                // from the render must leave exactly the row's new-side line —
                // what a user would see on disk. The merge guarantees it (the
                // common/add runs cover the new text; the del runs are the
                // old fragments), and this walk locks it.
                if (r.kind == RowKind::Mod || r.kind == RowKind::Add) && r.del_text.is_some() {
                    let kept: String = r
                        .runs
                        .iter()
                        .filter(|x| x.kind != RunKind::Del)
                        .map(|x| x.text.as_str())
                        .collect();
                    check!(
                        kept == r.text,
                        "{}:{}: the strike-removed render differs from the new line",
                        f.path,
                        r.new_line.unwrap_or(0)
                    );
                }
                for run in &r.runs {
                    match run.kind {
                        RunKind::Del => check!(
                            run.color.is_none() && run.sgr.is_none(),
                            "{}: a del run carries no color/sgr",
                            f.path
                        ),
                        RunKind::Common | RunKind::Add | RunKind::Ctx | RunKind::Mov => {
                            check!(
                                run.color.is_some(),
                                "{}: a {:?} run carries a semantic color",
                                f.path,
                                run.kind
                            );
                            if let Some(s) = run.sgr {
                                check!(
                                    matches!(s, 1 | 3 | 4 | 9),
                                    "{}: invalid sgr {s} (md emphasis)",
                                    f.path
                                );
                            }
                        }
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

        // 1. Verify reconstructed content completeness:
        if f.status != "deleted" && f.new_line_count > 0 {
            check!(
                !f.new_content.is_empty(),
                "{}: new_content must not be empty for non-deleted file (new_line_count={})",
                f.path,
                f.new_line_count
            );
            check!(
                f.new_content.len() == f.new_line_count,
                "{}: new_content len {} != new_line_count {}",
                f.path,
                f.new_content.len(),
                f.new_line_count
            );
        }
        if f.status != "added" && f.old_line_count > 0 {
            check!(
                !f.old_content.is_empty(),
                "{}: old_content must not be empty for non-added file (old_line_count={})",
                f.path,
                f.old_line_count
            );
            check!(
                f.old_content.len() == f.old_line_count,
                "{}: old_content len {} != old_line_count {}",
                f.path,
                f.old_content.len(),
                f.old_line_count
            );
        }

        // 2. Verify visible rows completeness and accuracy
        if !f.hunks.is_empty() {
            check!(
                !f.rows.is_empty(),
                "{}: visible rows must not be empty for file with hunks",
                f.path
            );
        }

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
                        r.text == expected_text,
                        "{}:{}: visible row {} text {:?} != new_content text {:?}",
                        f.path,
                        nl,
                        ri,
                        r.text,
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
                    r.text == expected_text,
                    "{}:{}: visible old row {} text {:?} != old_content text {:?}",
                    f.path,
                    ol,
                    ri,
                    r.text,
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
                let concat: String = r.runs.iter().map(|x| x.text.as_str()).collect();
                check!(
                    concat == r.text,
                    "{}: visible ctx row {} runs concat {:?} != text {:?}",
                    f.path,
                    ri,
                    concat,
                    r.text
                );
            }
        }
    }
    assert_eq!(
        failures, 0,
        "{failures} golden-walk failure(s) — see the FAIL lines above"
    );
}

fn print_wire_diff(name: &str, actual: &PipelineOutput, expected: &PipelineOutput) {
    eprintln!("\n----- WIRE DIFF [{name}] -----");
    if actual.files.len() != expected.files.len() {
        eprintln!(
            "  File count mismatch: actual={}, expected={}",
            actual.files.len(),
            expected.files.len()
        );
    }
    for (fi, (af, ef)) in actual.files.iter().zip(expected.files.iter()).enumerate() {
        if af.path != ef.path {
            eprintln!(
                "  [file {fi}] Path mismatch: actual={:?}, expected={:?}",
                af.path, ef.path
            );
        }
        if af.hunks.len() != ef.hunks.len() {
            eprintln!(
                "  [file {fi} {:?}] Hunk count mismatch: actual={}, expected={}",
                af.path,
                af.hunks.len(),
                ef.hunks.len()
            );
        }
        for (hi, (ah, eh)) in af.hunks.iter().zip(ef.hunks.iter()).enumerate() {
            if ah.rows.len() != eh.rows.len() {
                eprintln!(
                    "  [file {fi} {:?} hunk {hi}] Row count mismatch: actual={}, expected={}",
                    af.path,
                    ah.rows.len(),
                    eh.rows.len()
                );
            }
            for (ri, (ar, er)) in ah.rows.iter().zip(eh.rows.iter()).enumerate() {
                if ar != er {
                    eprintln!("  [file {fi} {:?} hunk {hi} row {ri}] mismatch:", af.path);
                    if ar.kind != er.kind
                        || ar.new_line != er.new_line
                        || ar.old_line != er.old_line
                        || ar.text != er.text
                    {
                        eprintln!(
                            "    actual:   kind={:?}, nl={:?}, ol={:?}, text={:?}",
                            ar.kind, ar.new_line, ar.old_line, ar.text
                        );
                        eprintln!(
                            "    expected: kind={:?}, nl={:?}, ol={:?}, text={:?}",
                            er.kind, er.new_line, er.old_line, er.text
                        );
                    }
                    if ar.del_text != er.del_text {
                        eprintln!("      del_text actual:   {:?}", ar.del_text);
                        eprintln!("      del_text expected: {:?}", er.del_text);
                    }
                    if ar.spans != er.spans {
                        eprintln!("      spans actual:   {:?}", ar.spans);
                        eprintln!("      spans expected: {:?}", er.spans);
                    }
                    if ar.runs != er.runs {
                        eprintln!("      runs actual:   {:?}", ar.runs);
                        eprintln!("      runs expected: {:?}", er.runs);
                    }
                    if ar.wholesale != er.wholesale || ar.moved != er.moved {
                        eprintln!(
                            "      flags actual:   wholesale={}, moved={}",
                            ar.wholesale, ar.moved
                        );
                        eprintln!(
                            "      flags expected: wholesale={}, moved={}",
                            er.wholesale, er.moved
                        );
                    }
                }
            }
        }
        if af.new_content != ef.new_content {
            eprintln!(
                "  [file {fi} {:?}] new_content mismatch: actual len={}, expected len={}",
                af.path,
                af.new_content.len(),
                ef.new_content.len()
            );
        }
        if af.old_content != ef.old_content {
            eprintln!(
                "  [file {fi} {:?}] old_content mismatch: actual len={}, expected len={}",
                af.path,
                af.old_content.len(),
                ef.old_content.len()
            );
        }
        if af.rows.len() != ef.rows.len() {
            eprintln!(
                "  [file {fi} {:?}] Visible rows count mismatch: actual={}, expected={}",
                af.path,
                af.rows.len(),
                ef.rows.len()
            );
        } else {
            for (ri, (ar, er)) in af.rows.iter().zip(ef.rows.iter()).enumerate() {
                if ar != er {
                    eprintln!("  [file {fi} {:?} visible row {ri}] mismatch:", af.path);
                    if ar.kind != er.kind
                        || ar.new_line != er.new_line
                        || ar.old_line != er.old_line
                        || ar.text != er.text
                    {
                        eprintln!(
                            "    actual:   kind={:?}, nl={:?}, ol={:?}, text={:?}",
                            ar.kind, ar.new_line, ar.old_line, ar.text
                        );
                        eprintln!(
                            "    expected: kind={:?}, nl={:?}, ol={:?}, text={:?}",
                            er.kind, er.new_line, er.old_line, er.text
                        );
                    }
                    if ar.runs != er.runs {
                        eprintln!("      runs actual:   {:?}", ar.runs);
                        eprintln!("      runs expected: {:?}", er.runs);
                    }
                }
            }
        }
    }
    eprintln!("----- END WIRE DIFF -----\n");
}

static EXPECTED_WIRE_CACHE: LazyLock<Mutex<HashMap<String, Arc<PipelineOutput>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn load_expected_wire(name: &str) -> Arc<PipelineOutput> {
    let mut cache = EXPECTED_WIRE_CACHE.lock().unwrap();
    if let Some(out) = cache.get(name) {
        return Arc::clone(out);
    }
    let wire_path = expected_wire_path(name);
    let expected_str = fs::read_to_string(&wire_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read expected wire JSON for {name} from {}: {e}",
            wire_path.display()
        )
    });
    let parsed: PipelineOutput = serde_json::from_str(&expected_str)
        .unwrap_or_else(|e| panic!("[{name}] failed to parse expected wire JSON: {e}"));
    let arc = Arc::new(parsed);
    cache.insert(name.to_string(), Arc::clone(&arc));
    arc
}

fn wire_files_match(a: &OutputFile, b: &OutputFile) -> bool {
    a.path == b.path
        && a.status == b.status
        && a.new_line_count == b.new_line_count
        && a.old_line_count == b.old_line_count
        && a.new_content == b.new_content
        && a.old_content == b.old_content
        && a.hunks == b.hunks
        && a.counts == b.counts
        && a.fences == b.fences
        && a.notes == b.notes
        && a.content_runs == b.content_runs
        && a.rows == b.rows
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

fn assert_wire_json_matches(output: &PipelineOutput, name: &str) {
    let wire_path = expected_wire_path(name);
    if std::env::var("GEN_GOLDEN").is_ok() || std::env::var("UPDATE_WIRE").is_ok() {
        let actual_pretty = format!(
            "{}\n",
            serde_json::to_string_pretty(output).expect("serialize pretty JSON")
        );
        write_file_plain(&wire_path, actual_pretty.as_bytes())
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", wire_path.display()));
        eprintln!("{name}: wire JSON updated ({})", wire_path.display());
        return;
    }
    let expected = load_expected_wire(name);
    if !wire_outputs_match(output, expected.as_ref()) {
        print_wire_diff(name, output, expected.as_ref());
        panic!(
            "[{name}] wire JSON output deviates from expected static golden ({}). See wire diff above. Use GEN_GOLDEN=1 or UPDATE_WIRE=1 to update.",
            wire_path.display()
        );
    }
}

fn print_render_diff(name: &str, actual: &str, expected: &str) {
    eprintln!("\n----- RENDER DIFF [{name}] -----");
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let diff = difflib::unified_diff(
        &expected_lines,
        &actual_lines,
        &format!("{name}.render.txt (expected)"),
        &format!("{name}.render.txt (actual)"),
        "",
        "",
        3,
    );
    for line in diff {
        eprintln!("{line}");
    }
    eprintln!("----- END RENDER DIFF -----\n");
}

fn assert_render_matches(output: &PipelineOutput, name: &str) {
    let theme = default_theme();
    let actual_raw = render_pipeline_output(output, &theme, true);
    let actual_encoded = encode_sgr(&actual_raw);
    let render_path = expected_render_path(name);

    if std::env::var("GEN_GOLDEN").is_ok() || std::env::var("UPDATE_RENDER").is_ok() {
        write_file_plain(&render_path, actual_encoded.as_bytes())
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", render_path.display()));
        eprintln!(
            "{name}: render static golden updated ({})",
            render_path.display()
        );
        return;
    }

    let expected_encoded = load_expected_render(name);
    if actual_encoded != *expected_encoded.as_ref() {
        print_render_diff(name, &actual_encoded, expected_encoded.as_ref());
        panic!(
            "[{name}] terminal render output deviates from expected static golden ({}). See render diff above. Use GEN_GOLDEN=1 or UPDATE_RENDER=1 to update.",
            render_path.display()
        );
    }
}

fn print_markdown_diff(name: &str, actual: &str, expected: &str) {
    eprintln!("\n----- MARKDOWN DIFF [{name}] -----");
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
    for line in diff {
        eprintln!("{line}");
    }
    eprintln!("----- END MARKDOWN DIFF -----\n");
}

fn assert_markdown_matches(output: &PipelineOutput, name: &str) {
    let opts = diffuse::render::MarkdownOptions::default();
    let actual_md = diffuse::render::render_markdown_output(output, &opts);
    let md_path = expected_markdown_path(name);

    if std::env::var("GEN_GOLDEN").is_ok()
        || std::env::var("UPDATE_MARKDOWN").is_ok()
        || std::env::var("UPDATE_MD").is_ok()
    {
        write_file_plain(&md_path, actual_md.as_bytes())
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", md_path.display()));
        eprintln!(
            "{name}: markdown static golden updated ({})",
            md_path.display()
        );
        return;
    }

    let expected_md = fs::read_to_string(&md_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read expected markdown output for {name} from {}: {e}. Use GEN_GOLDEN=1 or UPDATE_MARKDOWN=1 to create/update.",
            md_path.display()
        )
    });

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-m")
        .arg("--color=never")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the diffuse binary in markdown mode");
    {
        use std::io::Write;
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

    if std::env::var("GEN_GOLDEN").is_ok()
        || std::env::var("UPDATE_MARKDOWN").is_ok()
        || std::env::var("UPDATE_MD").is_ok()
    {
        return;
    }

    let expected_md = fs::read_to_string(&md_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read expected markdown output for {name} from {}: {e}. Use GEN_GOLDEN=1 or UPDATE_MARKDOWN=1 to create/update.",
            md_path.display()
        )
    });

    if bin_md != expected_md {
        print_markdown_diff(name, &bin_md, &expected_md);
        panic!(
            "[{name}] bin markdown output deviates from expected static golden ({}). See markdown diff above.",
            md_path.display()
        );
    }
}

fn run_lib_leg(name: &str, min: usize) -> (Arc<Fixture>, PipelineOutput) {
    let fixture = load_fixture(name);
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
    let output = diffuse::core::pipeline::run_pipeline_full(
        &fixture.diff,
        new_content,
        old_content,
        diffuse::core::pipeline::PipelineOptions::default(),
    );
    walk_and_assert(&output, name, min);
    assert_wire_json_matches(&output, name);
    (fixture, output)
}

fn run_bin_leg(name: &str, min: usize) {
    let fixture = load_fixture(name);
    let input = serde_json::to_vec(fixture.as_ref()).expect("serialize fixture");
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the diffuse binary");
    {
        use std::io::Write;
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
        "[{name}] the bin exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let decoded: PipelineOutput =
        serde_json::from_slice(&out.stdout).expect("decode the bin's token-stream JSON");
    walk_and_assert(&decoded, name, min);
    assert_wire_json_matches(&decoded, name);
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
        let applied = apply_file_diff(&slice, &old_vec);
        let expected: Vec<String> = new.split('\n').map(String::from).collect();
        assert_eq!(
            applied, expected,
            "[{name}:{path}] apply(file_diff, old) != new — the fixture's diff is not a valid transformation"
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

macro_rules! corpus_tests {
    ($mod_name:ident, $name:expr, $min:expr, $has_render:expr, $has_oracle:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn golden_acceptance() {
                let (fixture, output) = run_lib_leg($name, $min);
                if $has_render {
                    assert_render_matches(&output, $name);
                    assert_markdown_matches(&output, $name);
                }
                if $has_oracle {
                    run_oracle_for_corpus($name, &fixture, &output);
                }
            }

            #[test]
            fn bin_leg() {
                run_bin_leg($name, $min);
                if $has_render {
                    run_bin_markdown_leg($name);
                }
            }
        }
    };
    (ignored $mod_name:ident, $name:expr, $min:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore = "massive history stress corpus (run with cargo test -- --ignored)"]
            fn lib_leg() {
                run_lib_leg($name, $min);
            }

            #[test]
            #[ignore = "massive history stress corpus (run with cargo test -- --ignored)"]
            fn bin_leg() {
                run_bin_leg($name, $min);
            }
        }
    };
}

corpus_tests!(mirage_2025_08, "mirage-2025-08", 8, true, false);
corpus_tests!(diffuse_2025_08, "diffuse-2025-08", 26, true, false);
corpus_tests!(opt_1, "opt-1-cache-regex-window-slice", 5, true, true);
corpus_tests!(opt_2, "opt-2-cache-markdown-regex", 2, true, true);
corpus_tests!(opt_3, "opt-3-cache-lang-profile", 3, true, true);
corpus_tests!(opt_4, "opt-4-manual-scanner-prefilter", 4, true, true);
corpus_tests!(diffuse_history_mid, "diffuse-history-mid", 45, true, false);
corpus_tests!(untracked_files, "untracked-files", 2, true, false);
