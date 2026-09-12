//! Ports of the collapse tests from test/review.test.ts: the similarity
//! pairing + the declaration section-stop, the `None` ↔ `Some(result)`
//! brace-anchor, the reflow pairing, the reverse stretch.

use std::time::Instant;

use crate::{
    collapse::collapse_hunk_rows,
    moves::mark_moves,
    parse::{AddLine, CtxLine, DelLine, DiffFile, FileStatus, Hunk, LineNum, Row, RowKind},
    syntax::registry::lang_profile,
};

fn row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    match kind {
        RowKind::Del => {
            let num = old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            Row::Del(DelLine::new(num, text))
        }
        RowKind::Add => {
            let num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            Row::Add(AddLine::new(num, text))
        }
        RowKind::Mod => {
            let old_num = old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            let new_num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            Row::Mod {
                old_line: DelLine::new(old_num, text),
                new_line: AddLine::new(new_num, text),
            }
        }
        RowKind::Ctx => {
            let old_num = old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            let new_num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
            Row::Ctx(CtxLine::new(old_num, new_num, text))
        }
        RowKind::Mov => {
            if let Some(old) = old_line.and_then(LineNum::new) {
                Row::MovSrc(DelLine::new(old, text))
            } else {
                let new_num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                Row::MovTgt(AddLine::new(new_num, text))
            }
        }
        RowKind::File | RowKind::Sep => Row::Ctx(CtxLine::new(LineNum::MIN, LineNum::MIN, text)),
    }
}

fn moved_row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    let old_num = old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
    let new_num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
    match kind {
        RowKind::Del => Row::MovSrc(DelLine::new(old_num, text)),
        _ => Row::MovTgt(AddLine::new(new_num, text)),
    }
}

fn hunk(rows: Vec<Row>) -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

fn file(hunks: Vec<Hunk>) -> DiffFile {
    DiffFile {
        path: "f.rs".into(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: FileStatus::Modified,
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
        raw_hunks: Vec::new(),
        no_content: false,
    }
}

// The similarity pairing + declaration section-stop: the old lookup must
// pair with the NEW lookup (not the FIFO-next doc comment), the additions
// render BEFORE the mod, and comment blocks are their own sections.
#[test]
fn similarity_pairing_and_section_stop() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {",
            None,
            Some(133),
        ),
        row(
            RowKind::Add,
            "    /// Current epoch. A resolution must snapshot this *before* its first",
            Some(134),
            None,
        ),
        row(
            RowKind::Add,
            "    /// lookup and pass it to both `lookup` and `insert`.",
            Some(135),
            None,
        ),
        row(
            RowKind::Add,
            "    pub fn epoch(&self) -> u64 {",
            Some(136),
            None,
        ),
        row(
            RowKind::Add,
            "        self.epoch.load(Ordering::Relaxed)",
            Some(137),
            None,
        ),
        row(RowKind::Add, "    }", Some(138), None),
        row(RowKind::Add, "", Some(139), None),
        row(
            RowKind::Add,
            "    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {",
            Some(140),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    let mod_idx = c
        .iter()
        .position(|r| r.kind() == RowKind::Mod)
        .expect("lookup pairs with lookup");
    assert!(
        c[mod_idx].text().contains("epoch: u64"),
        "the mod's add is the new lookup"
    );
    assert!(
        c[0].kind() == RowKind::Add && c[0].new_line() == Some(134),
        "adds lead the block"
    );
    assert!(mod_idx > 0, "mod sits after its preceding additions");
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.text().contains("pub fn epoch")),
        "the epoch method is its own section"
    );
    assert_eq!(
        c.iter()
            .filter(|r| r.kind() == RowKind::Add && r.text().contains("///"))
            .count(),
        2,
        "the doc comment lines are separate adds"
    );
}

// The brace-anchor: the del `None` before the fn's `}` pairs with the new
// `Some(result)` before the `}` — not the mid-block `return None;`.
#[test]
fn brace_anchor_none_some_result() {
    let mut old_content: Vec<&str> = vec![""; 130];
    old_content[113] = "    }"; // the fn's closing brace after the del
    let mut new_content: Vec<&str> = vec![""; 320];
    new_content[155] = "    }"; // the fn's closing brace after Some(result)
    new_content[299] = "        None"; // a verbatim None elsewhere
    let h = hunk(vec![
        row(
            RowKind::Del,
            "        self.cache.borrow_mut().pop(key);",
            None,
            Some(111),
        ),
        row(
            RowKind::Del,
            "        self.results.borrow_mut().pop(key);",
            None,
            Some(112),
        ),
        row(RowKind::Del, "        None", None, Some(113)),
        row(
            RowKind::Add,
            "        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();",
            Some(147),
            None,
        ),
        row(
            RowKind::Add,
            "        if entry_epoch != epoch {",
            Some(148),
            None,
        ),
        row(RowKind::Add, "            return None;", Some(153), None),
        row(RowKind::Add, "        }", Some(154), None),
        row(RowKind::Add, "        Some(result)", Some(155), None),
    ]);
    let profile = lang_profile("rs");
    let c = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), profile);
    let none_mod = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod && r.del_text() == Some("        None"))
        .expect("the None mod");
    assert_eq!(
        none_mod.new_line(),
        Some(155),
        "the brace-anchor pairs None with Some(result) despite the verbatim None in the new file"
    );
}

// In indentation-based languages like Python, the block close is defined by a
// dedent boundary. Tail statements right before the dedent pair appropriately.
#[test]
fn python_block_tail_anchor_none_some_result() {
    let mut old_content: Vec<&str> = vec![""; 130];
    old_content[113] = "def next_fn():"; // dedent boundary after return None
    let mut new_content: Vec<&str> = vec![""; 320];
    new_content[155] = "def next_fn():"; // dedent boundary after return result
    new_content[299] = "        return None"; // a verbatim return None elsewhere
    let h = hunk(vec![
        row(RowKind::Del, "    self.cache.pop(key)", None, Some(111)),
        row(RowKind::Del, "    self.results.pop(key)", None, Some(112)),
        row(RowKind::Del, "    return None", None, Some(113)),
        row(
            RowKind::Add,
            "    result = self.results.get(key)",
            Some(147),
            None,
        ),
        row(RowKind::Add, "    if not result:", Some(148), None),
        row(RowKind::Add, "        return None", Some(153), None),
        row(RowKind::Add, "    return result", Some(155), None),
    ]);
    let py = lang_profile("py");
    let c = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), py);
    let none_mod = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod && r.del_text() == Some("    return None"))
        .expect("the None mod in python");
    assert_eq!(
        none_mod.new_line(),
        Some(155),
        "the python dedent anchor pairs return None with return result"
    );
}

// The reflow pairing: the first del carries the change, the reflows drop —
// the `resolved_layers` / `.iter()` / `.any(…)` one-liner fold.
#[test]
fn reflow_pairing_first_del() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "        self.resolved_layers",
            None,
            Some(756),
        ),
        row(RowKind::Del, "            .iter()", None, Some(757)),
        row(
            RowKind::Del,
            "            .any(|l| l.path().join(sub_path).is_dir())",
            None,
            Some(758),
        ),
        row(
            RowKind::Add,
            "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())",
            Some(912),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(c.len(), 1, "one mod");
    let m = &c[0];
    assert_eq!(m.kind(), RowKind::Mod);
    assert_eq!(
        m.del_text(),
        Some("        self.resolved_layers"),
        "the first del pairs"
    );
    let spans = m.spans().expect("spans");
    assert_eq!(spans.del, vec![(13, 22)], "the resolved_ strike only");
    assert_eq!(
        spans.add,
        vec![(19, 68)],
        "the appended method chain is marked added"
    );
    let runs = crate::merge::merged_runs(m);
    assert_eq!(crate::merge::project_reverse(&runs), m.del_text().unwrap());
    assert_eq!(crate::merge::project_forward(&runs), m.text());

    // The `let up` one-liner's reflowed tail: the `.find(…)` content of the
    // dropped del feeds back as common, so the `?;` reads common, not added.
    let h2 = hunk(vec![
        row(
            RowKind::Del,
            "        let up = self.resolved_layers[ro_match + 1..]",
            None,
            Some(745),
        ),
        row(RowKind::Del, "            .iter()", None, Some(746)),
        row(
            RowKind::Del,
            "            .find(|l| l.is_upper())?;",
            None,
            Some(747),
        ),
        row(
            RowKind::Add,
            "        let up = self.layers[ro_match + 1..].iter().find(|l| l.is_upper())?;",
            Some(903),
            None,
        ),
    ]);
    let c2 = collapse_hunk_rows(&h2, None, None, None);
    assert_eq!(c2.len(), 1, "one mod");
    let m2 = &c2[0];
    let spans2 = m2.spans().expect("spans");
    assert_eq!(spans2.del, vec![(22, 31)], "the resolved_ strike");
    assert_eq!(
        spans2.add,
        vec![(44, 76)],
        "the appended method chain is marked added"
    );
    let runs2 = crate::merge::merged_runs(m2);
    assert_eq!(
        crate::merge::project_reverse(&runs2),
        m2.del_text().unwrap()
    );
    assert_eq!(crate::merge::project_forward(&runs2), m2.text());
}

// The modified-pair collapse: the mod is a 1-to-1 row; its section's additions
// and closing blank render as standalone add rows; moved lines reflow; the add
// after the blank is a separate row.
#[test]
fn modified_pair_absorbs_section_and_reflow() {
    let mut f = file(vec![hunk(vec![
        row(RowKind::Del, "let old_value = calculate();", None, Some(1)),
        row(RowKind::Add, "let new_value = calculate();", Some(1), None),
        row(RowKind::Add, "extra line", Some(2), None),
        row(RowKind::Add, "", Some(3), None), // section break
        row(RowKind::Del, "moved line", None, Some(4)),
        row(RowKind::Add, "moved line", Some(4), None),
        row(RowKind::Add, "after the blank", Some(5), None),
    ])]);
    mark_moves(&mut f);
    let c = collapse_hunk_rows(&f.hunks[0], None, None, None);
    assert_eq!(
        c.len(),
        6,
        "collapsed to 6 rows (mod, extra, blank, mov src, mov tgt, after)"
    );
    assert_eq!(c[0].kind(), RowKind::Mod, "mod row leads");
    assert_eq!(
        c[1].kind(),
        RowKind::Mov,
        "source moved line appears as MovSrc anchored after mod"
    );
    assert_eq!(c[1].old_line(), Some(4));
    assert_eq!(
        c[2].kind(),
        RowKind::Add,
        "extra line is a separate add row"
    );
    assert_eq!(c[2].new_line(), Some(2));
    assert_eq!(
        c[3].kind(),
        RowKind::Add,
        "blank line is a separate add row"
    );
    assert_eq!(c[3].new_line(), Some(3));
    assert_eq!(
        c[4].kind(),
        RowKind::Mov,
        "target moved line appears as MovTgt"
    );
    assert_eq!(c[4].new_line(), Some(4));
    assert_eq!(
        c[5].kind(),
        RowKind::Add,
        "the add after the blank is a separate row"
    );
    assert_eq!(c[5].new_line(), Some(5));
    let spans = c[0].spans().expect("spans");
    assert_eq!(spans.add.len(), 1, "mod row has baked add spans");
}

// A genuinely removed line (content NOT in any add) stays a del, but a
// del RUN whose text reflows onto the one-liner folds into ONE mod.
#[test]
fn del_only_run_folds_into_one_mod() {
    let h2 = hunk(vec![
        row(
            RowKind::Del,
            "        let up = self.resolved_layers[ro_match + 1..]",
            None,
            Some(745),
        ),
        row(RowKind::Del, "            .iter()", None, Some(746)),
        row(
            RowKind::Del,
            "            .find(|l| l.is_upper())?;",
            None,
            Some(747),
        ),
        row(
            RowKind::Add,
            "        let up = self.layers[ro_match + 1..].iter().find(|l| l.is_upper())?;",
            Some(903),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h2, None, None, None);
    assert_eq!(c.len(), 1, "one mod row");
    assert_eq!(c[0].kind(), RowKind::Mod);
}

// Fragment-pair rejection: a one-line `use` split into a 7-line block —
// every new line is a fragment of the old (weak overall similarity). No
// fragment pairs as "the" replacement: the del stays a removal and all
// seven adds survive.
#[test]
fn fragment_pair_rejection() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "use std::{cell::RefCell, num::NonZeroUsize, os::unix::fs::MetadataExt, path::PathBuf};",
            None,
            Some(1),
        ),
        row(RowKind::Add, "use std::{", Some(1), None),
        row(RowKind::Add, "    cell::RefCell,", Some(2), None),
        row(RowKind::Add, "    num::NonZeroUsize,", Some(3), None),
        row(
            RowKind::Add,
            "    os::unix::fs::MetadataExt,",
            Some(4),
            None,
        ),
        row(RowKind::Add, "    path::PathBuf,", Some(5), None),
        row(
            RowKind::Add,
            "    sync::atomic::{AtomicU64, Ordering},",
            Some(6),
            None,
        ),
        row(RowKind::Add, "};", Some(7), None),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Mod).count(),
        1,
        "use expansion pairs anchor as mod"
    );
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Add).count(),
        6,
        "subsequent lines are subword-expanded additions"
    );
    assert_eq!(c[0].kind(), RowKind::Mod, "mod leads the expansion block");
    assert_eq!(c[0].new_line(), Some(1));
}

// Subword pairs render as mod rows with fine-grained del/add token spans.
#[test]
fn subword_diff_token_spans() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    let s = prefix_middle_suffix + other_operations();",
            None,
            Some(1),
        ),
        row(
            RowKind::Add,
            "    let s = prefix_suffix + other_operations();",
            Some(1),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Mod).count(),
        1,
        "pairs as a mod row with subword strike"
    );
    assert_eq!(c[0].new_line(), Some(1));
    // Growth pair: `0` → `Option<usize>`/`None` fragments still pair.
    let g = hunk(vec![
        row(RowKind::Del, "    let mut floor: usize = 0;", None, Some(1)),
        row(
            RowKind::Add,
            "    let mut floor: Option<usize> = None;",
            Some(1),
            None,
        ),
    ]);
    assert!(
        collapse_hunk_rows(&g, None, None, None)
            .iter()
            .any(|r| r.kind() == RowKind::Mod),
        "single-del floor pairs into a mod"
    );
    // A clean rename still pairs.
    let h2 = hunk(vec![
        row(RowKind::Del, "        self.resolved_layers", None, Some(1)),
        row(RowKind::Add, "        self.layers", Some(1), None),
    ]);
    assert!(
        collapse_hunk_rows(&h2, None, None, None)
            .iter()
            .any(|r| r.kind() == RowKind::Mod),
        "a clean rename still pairs"
    );
    // The if-cross subwords too (meaningful identity).
    let i = hunk(vec![
        row(
            RowKind::Del,
            "                if k < floor {",
            None,
            Some(1),
        ),
        row(
            RowKind::Add,
            "                    && k > floor",
            Some(1),
            None,
        ),
    ]);
    assert!(
        collapse_hunk_rows(&i, None, None, None)
            .iter()
            .any(|r| r.kind() == RowKind::Mod),
        "the if-cross subwords too (meaningful identity)"
    );
}

// Tail guard: when the mod's add is the LAST line of a replacement block,
// the block's earlier lines are + rows (never the mod's unnumbered tail) —
// and an add matching a MOVED del (shown ≈ elsewhere) is real content, not
// a reflow, so it survives.
#[test]
fn tail_guard_and_reflow_corpus() {
    let h3 = hunk(vec![
        row(RowKind::Del, "        None", None, Some(113)),
        row(
            RowKind::Add,
            "        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();",
            Some(147),
            None,
        ),
        row(
            RowKind::Add,
            "        if entry_epoch != epoch {",
            Some(148),
            None,
        ),
        row(
            RowKind::Add,
            "            // Computed under an older mount state: the epoch bump (mode flip,",
            Some(149),
            None,
        ),
        row(
            RowKind::Add,
            "            // mount mutation) invalidated it.",
            Some(150),
            None,
        ),
        row(
            RowKind::Add,
            "            self.cache.borrow_mut().pop(key);",
            Some(151),
            None,
        ),
        row(
            RowKind::Add,
            "            self.results.borrow_mut().pop(key);",
            Some(152),
            None,
        ),
        row(RowKind::Add, "            return None;", Some(153), None),
        row(RowKind::Add, "        }", Some(154), None),
        row(RowKind::Add, "        Some(result)", Some(155), None),
    ]);

    let mut old_content = vec![""; 130];
    old_content[113] = "    }";
    let mut new_content = vec![""; 200];
    new_content[155] = "    }";
    let c = collapse_hunk_rows(&h3, Some(&old_content), Some(&new_content), None);
    let mod3 = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod)
        .expect("the mod");
    assert_eq!(
        mod3.new_line(),
        Some(155),
        "pairs old None with Some(result)"
    );
    assert_eq!(mod3.del_text(), Some("        None"));
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.new_line() == Some(151)),
        "the duplicate pop pair survives (not reflow-absorbed)"
    );
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.new_line() == Some(147)),
        "the block's earlier lines are adds, not tail"
    );
    let seq: Vec<u32> = c
        .iter()
        .filter(|r| r.kind() == RowKind::Add || r.kind() == RowKind::Mod)
        .map(|r| r.new_line().unwrap())
        .collect();
    assert_eq!(
        seq,
        vec![147, 148, 149, 150, 151, 152, 153, 154, 155],
        "new-file order"
    );
}

#[test]
fn test_moved_line_duplicate_in_added_block_stays_add() {
    let mut f = file(vec![hunk(vec![
        row(
            RowKind::Del,
            "        self.cache.borrow_mut().pop(key);",
            None,
            Some(111),
        ),
        row(
            RowKind::Del,
            "        self.results.borrow_mut().pop(key);",
            None,
            Some(112),
        ),
        row(
            RowKind::Add,
            "            self.cache.borrow_mut().pop(key);",
            Some(143),
            None,
        ),
        row(
            RowKind::Add,
            "            self.results.borrow_mut().pop(key);",
            Some(144),
            None,
        ),
        row(
            RowKind::Add,
            "        if entry_epoch != epoch {",
            Some(148),
            None,
        ),
        row(
            RowKind::Add,
            "            self.cache.borrow_mut().pop(key);",
            Some(151),
            None,
        ),
        row(
            RowKind::Add,
            "            self.results.borrow_mut().pop(key);",
            Some(152),
            None,
        ),
        row(RowKind::Add, "        }", Some(154), None),
    ])]);
    mark_moves(&mut f);
    let c = collapse_hunk_rows(&f.hunks[0], None, None, None);
    let r151 = c
        .iter()
        .find(|r| r.new_line() == Some(151))
        .expect("line 151 exists");
    assert_eq!(
        r151.kind(),
        RowKind::Add,
        "line 151 in the middle of add block must remain an addition, not a spurious mod"
    );
    let r152 = c
        .iter()
        .find(|r| r.new_line() == Some(152))
        .expect("line 152 exists");
    assert_eq!(
        r152.kind(),
        RowKind::Add,
        "line 152 in the middle of add block must remain an addition"
    );
}

#[test]
fn test_fn_signature_pairs_with_same_fn_name() {
    let old_content: Vec<&str> = vec![
        "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {",
        "        None",
        "    }",
    ];
    let new_content: Vec<&str> = vec![
        "    pub fn epoch(&self) -> u64 {",
        "        0",
        "    }",
        "",
        "    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {",
        "        None",
        "    }",
    ];
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {",
            None,
            Some(1),
        ),
        row(
            RowKind::Add,
            "    pub fn epoch(&self) -> u64 {",
            Some(1),
            None,
        ),
        row(RowKind::Add, "        0", Some(2), None),
        row(RowKind::Add, "    }", Some(3), None),
        row(RowKind::Add, "", Some(4), None),
        row(
            RowKind::Add,
            "    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {",
            Some(5),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), None);
    let mod_row = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod)
        .expect("mod row");
    assert_eq!(
        mod_row.new_line(),
        Some(5),
        "old lookup pairs with new lookup signature, not epoch"
    );
    let epoch_row = c
        .iter()
        .find(|r| r.new_line() == Some(1))
        .expect("epoch row");
    assert_eq!(epoch_row.kind(), RowKind::Add, "epoch is an added fn");
}

#[test]
fn markdown_table_1_to_1_pairing() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "| `hunkPath`, `unquoteGitPath`, `parseDiff` (579) | diff → hunks/rows, the fixture shape |",
            None,
            Some(40),
        ),
        row(
            RowKind::Del,
            "| `addCount` (237), `delCount` (261) | hunk new/old side extents |",
            None,
            Some(41),
        ),
        row(
            RowKind::Del,
            "| `tokenizeLine` (906), `lcsPairs` (927), `spansFrom` (988), `wordSpans` (1011) | the word-level LCS |",
            None,
            Some(42),
        ),
        row(
            RowKind::Del,
            "| `lineSimilarity` (1230), `lineSimilarityParts` (1239) | pairing scores |",
            None,
            Some(43),
        ),
        row(
            RowKind::Del,
            "| `markMoves` (1150) | exact-content relocation + the block-move rule |",
            None,
            Some(44),
        ),
        row(
            RowKind::Del,
            "| `collapseHunkRows` (1303) | pairing, stretch, reverse-stretch, reflow, the ordered walk — **the big one** |",
            None,
            Some(45),
        ),
        row(
            RowKind::Add,
            "| `hunkPath`, `unquoteGitPath`, `parseDiff` | diff → hunks/rows, the fixture shape |",
            Some(41),
            None,
        ),
        row(
            RowKind::Add,
            "| `addCount`/`delCount` | hunk new/old side extents |",
            Some(42),
            None,
        ),
        row(
            RowKind::Add,
            "| `tokenizeLine`, `lcsPairs`, `spansFrom`, `wordSpans` | the word-level LCS |",
            Some(43),
            None,
        ),
        row(
            RowKind::Add,
            "| `lineSimilarity`, `lineSimilarityParts` | pairing scores |",
            Some(44),
            None,
        ),
        row(
            RowKind::Add,
            "| `markMoves` | exact-content relocation + the block-move rule |",
            Some(45),
            None,
        ),
        row(
            RowKind::Add,
            "| `collapseHunkRows` | pairing, stretch, reverse-stretch, reflow, the ordered walk — **the big one** |",
            Some(46),
            None,
        ),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Mod).count(),
        6,
        "all 6 rows pair 1-to-1 as mod rows"
    );
}

// The stretch-window scan is LAZY: the eager build materialized every
// sub-window of every consecutive-add run (n³/6 elements — a diff with a
// huge whole-file add OOMs both engines before the pairing ran). The scan
// is generated on demand and bounded by the 4× content rule, so a 5000-line
// add with no dels costs nothing.
#[test]
fn whole_file_add_no_oom() {
    let adds: Vec<Row> = (1..=5000)
        .map(|i| {
            row(
                RowKind::Add,
                &format!("let value_{i} = fn_{i}(param_{i}); // filler {i}"),
                Some(i),
                None,
            )
        })
        .collect();
    let h = hunk(adds);
    let t = Instant::now();
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(c.len(), 5000, "all adds survive unpaired");
    assert!(
        t.elapsed().as_secs() < 10,
        "no eager window blowup (took {:?})",
        t.elapsed()
    );
}

#[test]
fn single_del_pairs_1_to_1_with_best_matching_add() {
    let h = hunk(vec![
        row(RowKind::Del, "if k < floor {", None, Some(10)),
        row(
            RowKind::Add,
            "let mut floor = self.floors.last();",
            Some(20),
            None,
        ),
        row(RowKind::Add, "if let Some(floor) = floor {", Some(21), None),
        row(RowKind::Add, "&& k > floor", Some(22), None),
        row(RowKind::Add, "{", Some(23), None),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    let m = c
        .iter()
        .find(|r| r.old_line() == Some(10))
        .expect("the mod row");
    assert_eq!(m.kind(), RowKind::Mod);
    assert_eq!(m.new_line(), Some(21));
    assert_eq!(
        m.text(),
        "if let Some(floor) = floor {",
        "statement head if pairs with if let Some"
    );
}

#[test]
fn test_multiline_expansion_subword_distribution() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "                if k < floor {",
            None,
            Some(1040),
        ),
        row(
            RowKind::Add,
            "                if let Some(floor) = floor",
            Some(1082),
            None,
        ),
        row(
            RowKind::Add,
            "                    && k > floor",
            Some(1083),
            None,
        ),
        row(RowKind::Add, "                {", Some(1084), None),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(c.len(), 3);
    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(c[0].new_line(), Some(1082));
    assert_eq!(c[0].text(), "                if let Some(floor) = floor");

    assert_eq!(c[1].kind(), RowKind::Add);
    assert_eq!(c[1].new_line(), Some(1083));
    assert_eq!(c[1].text(), "                    && k > floor");

    assert_eq!(c[2].kind(), RowKind::Add);
    assert_eq!(c[2].new_line(), Some(1084));
}

// Kept-del anchoring: a genuinely removed line renders right after the mod
// whose old position preceded it, never interrupting the adds.
#[test]
fn kept_del_anchoring() {
    let h2 = hunk(vec![
        row(
            RowKind::Del,
            "        if fingerprint.is_valid() {",
            None,
            Some(108),
        ),
        row(
            RowKind::Del,
            "            return self.results.borrow_mut().get(key).cloned();",
            None,
            Some(109),
        ),
        row(
            RowKind::Add,
            "        if !fingerprint.is_valid() {",
            Some(142),
            None,
        ),
        moved_row(
            RowKind::Add,
            "            self.cache.borrow_mut().pop(key);",
            Some(143),
            None,
        ),
        moved_row(
            RowKind::Add,
            "            self.results.borrow_mut().pop(key);",
            Some(144),
            None,
        ),
        row(RowKind::Add, "            return None;", Some(145), None),
    ]);
    let c = collapse_hunk_rows(&h2, None, None, None);
    assert_eq!(c[0].kind(), RowKind::Mod, "mod leads");
    assert_eq!(c[0].new_line(), Some(142));
    assert_eq!(
        c[1].kind(),
        RowKind::Del,
        "anchored del follows its head mod"
    );
    assert_eq!(c[1].old_line(), Some(109));
    assert_eq!(c[2].kind(), RowKind::Mov, "then the moves");
    assert_eq!(c[2].new_line(), Some(143));
    assert!(c[2].is_moved());
    assert_eq!(c[3].kind(), RowKind::Mov, "then the moves");
    assert_eq!(c[3].new_line(), Some(144));
    assert!(c[3].is_moved());
    assert_eq!(c[4].kind(), RowKind::Add, "then pure add");
    assert_eq!(c[4].new_line(), Some(145));
}

// Multi-line deletion into a 1-to-1 mod row: the first line pairs into a
// single-line mod row, verbatim reflows drop, and the remaining deleted lines
// render as separate `-` lines (no internal newlines / multi-line folded rows).
#[test]
fn multiline_del_mount_pairs_1_to_1_with_two_del_rows() {
    let h = hunk(vec![
        row(RowKind::Del, "        self.mount_stack", None, Some(1019)),
        row(RowKind::Del, "            .filesystems", None, Some(1020)),
        row(RowKind::Del, "            .iter()", None, Some(1021)),
        row(
            RowKind::Del,
            "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            None,
            Some(1022),
        ),
        row(
            RowKind::Del,
            "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
            None,
            Some(1023),
        ),
        row(
            RowKind::Add,
            "        self.mount_stack.get(guest_path)",
            Some(1060),
            None,
        ),
        row(RowKind::Add, "    }", Some(1061), None),
    ]);
    let mut old_content: Vec<&str> = vec![""; 1030];
    old_content[1023] = "    }";
    let mut new_content: Vec<&str> = vec![""; 20];
    new_content[8] = "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {";
    new_content[9] = "        self.filesystems";
    new_content[10] = "            .iter()";
    new_content[11] = "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))";
    new_content[12] = "            .map(|fs| fs.as_ref() as &dyn Filesystem)";
    new_content[13] = "    }";
    let c = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), None);
    let rmod = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod)
        .expect("the 1-to-1 mod row");
    assert_eq!(
        rmod.del_text(),
        Some("        self.mount_stack"),
        "the mod row's del_text is strictly single-line"
    );
    assert_eq!(rmod.new_line(), Some(1060));
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Del).count(),
        4,
        "the 4 remaining deleted lines render as separate `-` lines"
    );
}

#[test]
fn no_similarity_pairing_across_different_functions() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "            .find(|l| l.is_upper())?;",
            None,
            Some(747),
        ),
        row(
            RowKind::Add,
            "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())",
            Some(912),
            None,
        ),
    ]);
    let mut old_content: Vec<&str> = vec![""; 800];
    let mut new_content: Vec<&str> = vec![""; 1000];
    old_content[739] = "fn resolve_layer_stack() {";
    old_content[746] = "            .find(|l| l.is_upper())?;";
    old_content[770] = "fn dir_exists_at(&self, sub_path: &Path) -> bool {";
    new_content[739] = "fn resolve_layer_stack() {";
    new_content[899] = "fn dir_exists_at(&self, sub_path: &Path) -> bool {";
    new_content[911] = "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())";

    let collapsed = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), None);
    let has_mod = collapsed.iter().any(|r| r.kind() == RowKind::Mod);
    assert!(
        !has_mod,
        "del in resolve_layer_stack must NOT pair as a Mod row with add in dir_exists_at"
    );
}

#[test]
fn struct_and_fn_visibility_change_pairs_across_doc_comments() {
    let h = hunk(vec![
        row(RowKind::Add, "/// new doc comment line", Some(575), None),
        row(
            RowKind::Del,
            "pub(super) struct ResolvedOverlayLayer {",
            None,
            Some(496),
        ),
        row(
            RowKind::Add,
            "pub struct ResolvedOverlayLayer {",
            Some(584),
            None,
        ),
    ]);
    let collapsed = collapse_hunk_rows(&h, None, None, None);
    let mod_row = collapsed.iter().find(|r| r.kind() == RowKind::Mod);
    assert!(
        mod_row.is_some(),
        "pub(super) struct and pub struct must pair into a Mod row across doc comment adds"
    );
    assert_eq!(mod_row.unwrap().old_line(), Some(496));
    assert_eq!(mod_row.unwrap().new_line(), Some(584));
}

#[test]
fn refactored_fn_renamed_lines_pair_as_mod() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "                    resolved_layers: unresolved,",
            None,
            Some(254),
        ),
        row(
            RowKind::Add,
            "                    layers: unresolved,",
            Some(302),
            None,
        ),
    ]);
    let mut old_content = vec![""; 300];
    let mut new_content = vec![""; 350];
    old_content[200] = "fn to_os_string(&self) {";
    new_content[280] = "fn into_filesystem(self) {";

    let collapsed = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), None);
    let mod_row = collapsed.iter().find(|r| r.kind() == RowKind::Mod);
    assert!(
        mod_row.is_some(),
        "lines inside a renamed method must pair as a Mod row when old fn name does not exist in new content"
    );
    assert_eq!(mod_row.unwrap().old_line(), Some(254));
    assert_eq!(mod_row.unwrap().new_line(), Some(302));
}

#[test]
fn comment_block_reflow_subword_pairing() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "            // Walk the non-top-upper layers top → bottom, honoring each",
            None,
            Some(1078),
        ),
        row(
            RowKind::Del,
            "            // layer's markers — a frozen (snapshotted) upper carries whiteouts and opaque dirs",
            None,
            Some(1079),
        ),
        row(
            RowKind::Del,
            "            // that must mask everything below them.",
            None,
            Some(1080),
        ),
        row(
            RowKind::Add,
            "            // Walk the non-top-upper lowers top → bottom, honoring each layer's markers —",
            Some(1078),
            None,
        ),
        row(
            RowKind::Add,
            "            // a frozen (snapshotted) layer carries",
            Some(1079),
            None,
        ),
        row(
            RowKind::Add,
            "            // whiteouts and opaque dirs that must mask everything below it.",
            Some(1080),
            None,
        ),
    ]);

    let collapsed = collapse_hunk_rows(&h, None, None, None);
    println!("COLLAPSED COMMENT BLOCK: {:#?}", collapsed);
}

#[test]
fn test_resolved_layers_subword_collapse() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "        self.resolved_layers = resolved;",
            Some(612),
            None,
        ),
        row(
            RowKind::Add,
            "        self.layers = resolved;",
            None,
            Some(770),
        ),
    ]);
    let collapsed = collapse_hunk_rows(&h, None, None, None);
    println!("COLLAPSED: {:#?}", collapsed);
    assert_eq!(collapsed.len(), 1, "Should collapse to 1 Mod row");
    assert_eq!(collapsed[0].kind(), RowKind::Mod);
}

#[test]
fn test_closing_brace_released_from_add_block() {
    let h = hunk(vec![
        row(RowKind::Del, "        self.mount_stack", None, Some(1020)),
        row(RowKind::Del, "            .filesystems", None, Some(1021)),
        row(RowKind::Del, "            .iter()", None, Some(1022)),
        row(
            RowKind::Del,
            "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            None,
            Some(1023),
        ),
        row(
            RowKind::Del,
            "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
            None,
            Some(1024),
        ),
        row(
            RowKind::Add,
            "        self.mount_stack.get(guest_path)",
            Some(1060),
            None,
        ),
        row(RowKind::Add, "    }", Some(1061), None),
        row(RowKind::Add, "", Some(1062), None),
        row(
            RowKind::Add,
            "    /// Mutable variant of [`Self::find_mount`] — for live mode changes",
            Some(1063),
            None,
        ),
        row(
            RowKind::Add,
            "    pub fn find_mount_mut(&mut self, guest_path: &Path) -> Option<&mut dyn Filesystem> {",
            Some(1064),
            None,
        ),
        row(
            RowKind::Add,
            "        self.mount_stack.get_mut(guest_path)",
            Some(1065),
            None,
        ),
        row(RowKind::Ctx, "    }", Some(1066), Some(1025)),
        row(RowKind::Ctx, "", Some(1067), Some(1026)),
    ]);

    let mut new_content: Vec<&str> = vec![""; 1100];
    new_content[1059] = "        self.mount_stack.get(guest_path)";
    new_content[1060] = "    }";
    new_content[1061] = "";
    new_content[1062] = "    /// Mutable variant of [`Self::find_mount`] — for live mode changes";
    new_content[1063] =
        "    pub fn find_mount_mut(&mut self, guest_path: &Path) -> Option<&mut dyn Filesystem> {";
    new_content[1064] = "        self.mount_stack.get_mut(guest_path)";
    new_content[1065] = "    }";
    new_content[1066] = "";

    let rs = lang_profile("rs");
    let collapsed = collapse_hunk_rows(&h, None, Some(&new_content), rs);
    // The mod leads; the reflowed del chain follows; the released `}` and blank
    // read as context, and the absorbed trailing `}` and blank read as adds.
    assert_eq!(collapsed[0].kind(), RowKind::Mod);
    assert_eq!(collapsed[0].new_line(), Some(1060));
    assert_eq!(collapsed[1].kind(), RowKind::Del);
    assert_eq!(
        collapsed[5].kind(),
        RowKind::Ctx,
        "released brace is context"
    );
    assert_eq!(collapsed[5].new_line(), Some(1061));
    assert_eq!(
        collapsed[6].kind(),
        RowKind::Ctx,
        "released blank is context"
    );
    assert_eq!(collapsed[6].new_line(), Some(1062));
    assert_eq!(collapsed[7].kind(), RowKind::Add);
    assert_eq!(collapsed[7].new_line(), Some(1063));
    assert_eq!(collapsed[10].kind(), RowKind::Add, "absorbed brace is add");
    assert_eq!(collapsed[10].new_line(), Some(1066));
    assert_eq!(collapsed[11].new_line(), Some(1067));
}

#[test]
fn test_hunk_patience_anchoring_prevents_cross_block_mispairing() {
    let h = hunk(vec![
        // Function alpha
        row(RowKind::Del, "pub fn alpha() -> u32 {", None, Some(10)),
        row(RowKind::Del, "    let val = 42;", None, Some(11)),
        row(RowKind::Del, "    val", None, Some(12)),
        row(RowKind::Del, "}", None, Some(13)),
        // Function beta
        row(RowKind::Del, "pub fn beta() -> u32 {", None, Some(20)),
        row(RowKind::Del, "    let val = 99;", None, Some(21)),
        row(RowKind::Del, "    val", None, Some(22)),
        row(RowKind::Del, "}", None, Some(23)),
        // New adds
        row(RowKind::Add, "pub fn alpha() -> u64 {", Some(10), None),
        row(RowKind::Add, "    let val = 4200;", Some(11), None),
        row(RowKind::Add, "    val", Some(12), None),
        row(RowKind::Add, "}", Some(13), None),
        row(RowKind::Add, "pub fn beta() -> u64 {", Some(20), None),
        row(RowKind::Add, "    let val = 9900;", Some(21), None),
        row(RowKind::Add, "    val", Some(22), None),
        row(RowKind::Add, "}", Some(23), None),
    ]);

    let rs = lang_profile("rs");
    let collapsed = collapse_hunk_rows(&h, None, None, rs);
    let alpha_head = collapsed
        .iter()
        .find(|r| r.text().contains("pub fn alpha()"))
        .expect("alpha head");
    assert_eq!(alpha_head.kind(), RowKind::Mod);
    assert_eq!(alpha_head.new_line(), Some(10));

    let beta_head = collapsed
        .iter()
        .find(|r| r.text().contains("pub fn beta()"))
        .expect("beta head");
    assert_eq!(beta_head.kind(), RowKind::Mod);
    assert_eq!(beta_head.new_line(), Some(20));

    // let val = 42 (old 11) must pair with let val = 4200 (new 11)
    let val_4200 = collapsed
        .iter()
        .find(|r| r.text().contains("let val = 4200"))
        .expect("val 4200");
    assert_eq!(val_4200.kind(), RowKind::Mod);
    assert_eq!(val_4200.old_line(), Some(11));

    // let val = 99 (old 21) must pair with let val = 9900 (new 21)
    let val_9900 = collapsed
        .iter()
        .find(|r| r.text().contains("let val = 9900"))
        .expect("val 9900");
    assert_eq!(val_9900.kind(), RowKind::Mod);
    assert_eq!(val_9900.old_line(), Some(21));
}

#[test]
fn test_unpaired_del_anchors_after_intermediate_context() {
    let h = hunk(vec![
        row(
            RowKind::Ctx,
            "        let prefix = true;",
            Some(300),
            Some(200),
        ),
        row(
            RowKind::Ctx,
            "        let stats = self.stats();",
            Some(301),
            Some(201),
        ),
        row(
            RowKind::Del,
            "        dropped_legacy_cleanup();",
            None,
            Some(202),
        ),
        row(RowKind::Add, "        let alpha = 100;", Some(302), None),
        row(RowKind::Add, "        let beta = 200;", Some(303), None),
    ]);

    let collapsed = collapse_hunk_rows(&h, None, None, None);

    // Row 0: let prefix Ctx at line 300
    assert_eq!(collapsed[0].kind(), RowKind::Ctx);
    assert_eq!(collapsed[0].new_line(), Some(300));

    // Row 1: let stats Ctx at line 301
    assert_eq!(collapsed[1].kind(), RowKind::Ctx);
    assert_eq!(collapsed[1].new_line(), Some(301));

    // Row 2: dropped_legacy_cleanup Del at old line 202 (must anchor AFTER let
    // stats Ctx, NOT at top of hunk)
    assert_eq!(collapsed[2].kind(), RowKind::Del);
    assert_eq!(collapsed[2].old_line(), Some(202));

    // Row 3: let alpha Add at line 302
    assert_eq!(collapsed[3].kind(), RowKind::Add);
    assert_eq!(collapsed[3].new_line(), Some(302));

    // Row 4: let beta Add at line 303
    assert_eq!(collapsed[4].kind(), RowKind::Add);
    assert_eq!(collapsed[4].new_line(), Some(303));
}

#[test]
fn test_comment_lines_do_not_pair_with_code_lines() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    /// The `env` field uses `Arc<RefCell<ProcessEnvironment>>`.",
            None,
            Some(70),
        ),
        row(
            RowKind::Del,
            "            env: Arc::new(RefCell::new(ProcessEnvironment::new(identity))),",
            None,
            Some(87),
        ),
        row(
            RowKind::Add,
            "            env: Shared::new(ProcessEnvironment::new(identity)),",
            Some(79),
            None,
        ),
    ]);

    let collapsed = collapse_hunk_rows(&h, None, None, None);

    // Row 0: Deleted doc comment stays Del (does not pair with code line)
    assert_eq!(collapsed[0].kind(), RowKind::Del);
    assert_eq!(collapsed[0].old_line(), Some(70));

    // Row 1: Code replacement pairs as Mod with the code line
    assert_eq!(collapsed[1].kind(), RowKind::Mod);
    assert_eq!(collapsed[1].old_line(), Some(87));
    assert_eq!(collapsed[1].new_line(), Some(79));
}

#[test]
fn test_cross_context_pairing_is_forbidden() {
    let h = hunk(vec![
        // Cluster 0
        row(
            RowKind::Del,
            "        let old_result = compute_total(a, b);",
            None,
            Some(10),
        ),
        // Context barrier
        row(
            RowKind::Ctx,
            "        let unchanged_barrier = step();",
            Some(20),
            Some(11),
        ),
        // Cluster 1
        row(
            RowKind::Add,
            "        let new_result = compute_total(a, b);",
            Some(21),
            None,
        ),
    ]);

    let collapsed = collapse_hunk_rows(&h, None, None, None);

    // Row 0 in Cluster 0 remains a Del
    assert_eq!(collapsed[0].kind(), RowKind::Del);
    assert_eq!(collapsed[0].old_line(), Some(10));

    // Row 1 is the Context barrier
    assert_eq!(collapsed[1].kind(), RowKind::Ctx);
    assert_eq!(collapsed[1].new_line(), Some(20));

    // Row 2 in Cluster 1 remains an Add (not paired across the context barrier)
    assert_eq!(collapsed[2].kind(), RowKind::Add);
    assert_eq!(collapsed[2].new_line(), Some(21));
}

#[test]
fn test_multiline_statement_expansion_preserves_closing_delimiter() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    let p3 = pass_filter_reflows(h, &p2.out, &p2.kept_dels, &p2.pure_adds, &pairing_ctx);",
            None,
            Some(76),
        ),
        row(
            RowKind::Add,
            "    let p3 = pass_filter_reflows(",
            Some(76),
            None,
        ),
        row(RowKind::Add, "        h,", Some(77), None),
        row(RowKind::Add, "        &p2.out,", Some(78), None),
        row(RowKind::Add, "        &p2.kept_dels,", Some(79), None),
        row(RowKind::Add, "        &p2.pure_adds,", Some(80), None),
        row(RowKind::Add, "        &pairing_ctx,", Some(81), None),
        row(RowKind::Add, "        profile,", Some(82), None),
        row(RowKind::Add, "    );", Some(83), None),
    ]);

    let rs = lang_profile("rs");
    let collapsed = collapse_hunk_rows(&h, None, None, rs);

    // Row on 81 (&pairing_ctx,) should NOT carry `);` as a deleted span
    let row_81 = collapsed
        .iter()
        .find(|r| r.new_line() == Some(81))
        .expect("row 81");
    let del_spans_81 = row_81.spans().map(|s| &s.del).cloned().unwrap_or_default();
    let del_texts_81: Vec<&str> = del_spans_81
        .iter()
        .map(|&(s, e)| &row_81.del_text().unwrap()[s..e])
        .collect();
    assert!(
        !del_texts_81.iter().any(|t| t.contains(");")),
        "row 81 must not delete `);` because `);` is on line 83: del_spans={del_texts_81:?}"
    );

    // Row on 83 (    );) should recognize `);` as common
    let row_83 = collapsed
        .iter()
        .find(|r| r.new_line() == Some(83))
        .expect("row 83");
    let add_spans_83 = row_83.spans().map(|s| &s.add).cloned().unwrap_or_default();
    let add_texts_83: Vec<&str> = add_spans_83
        .iter()
        .map(|&(s, e)| &row_83.text()[s..e])
        .collect();
    assert!(
        !add_texts_83.iter().any(|t| t.contains(");")),
        "row 83 must not mark `);` as added; it should be common: add_spans={add_texts_83:?}"
    );
}
#[test]
fn wrap_tail_reflow_keeps_source_strike() {
    // A fmt re-wrap: the old line's tail `b[j-1]` wraps onto its own new
    // line.  The mod row must still strike the tail that left it — the add
    // row below materializes the wrapped line, but the source strike is
    // what tells the reviewer the content moved from here to there.
    let rs = crate::syntax::registry::lang_profile("rs");
    let h = hunk(vec![
        row(
            RowKind::Del,
            "        // DP columns: v_cols[j] represents the bit-vector after processing b[j-1]",
            None,
            Some(36),
        ),
        row(
            RowKind::Add,
            "        // DP columns: v_cols[j] represents the bit-vector after processing",
            Some(36),
            None,
        ),
        row(RowKind::Add, "        // b[j-1]", Some(37), None),
    ]);
    let c = collapse_hunk_rows(&h, None, None, rs);
    assert_eq!(c.len(), 2, "one mod + one add");
    assert_eq!(c[0].kind(), RowKind::Mod);
    let spans = c[0].spans().expect("mod spans");
    assert_eq!(
        spans.del,
        vec![(75, 82)],
        "the wrapped tail ` b[j-1]` stays struck on the mod row; got {spans:?}"
    );
    assert_eq!(c[1].kind(), RowKind::Add);
}

#[test]
fn test_wholesale_replacement_mod_deconstructs_to_del_and_add() {
    let old_line = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let new_line = "lambda mu nu xi omicron pi rho sigma tau upsilon phi";

    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(220)),
        row(RowKind::Add, new_line, Some(220), None),
    ]);

    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(
        c.len(),
        2,
        "must deconstruct wholesale replacement into 2 rows (del + add)"
    );
    assert_eq!(c[0].kind(), RowKind::Del, "first row must be Del");
    assert_eq!(c[0].old_line(), Some(220));
    assert_eq!(c[0].text(), old_line);
    assert_eq!(c[1].kind(), RowKind::Add, "second row must be Add");
    assert_eq!(c[1].new_line(), Some(220));
    assert_eq!(c[1].text(), new_line);
    assert!(
        c.iter().all(|r| !matches!(r, Row::Mod { .. })),
        "no wholesale Mod row may exist"
    );
}

#[test]
fn test_long_json_lines_produce_granular_subline_spans_under_raised_cap() {
    let old_line = "{\"t\":\"line\",\"kind\":\"mod\",\"newLine\":170,\"oldLine\":164,\"text\":\"      rewrites). Break only for syntax (no `undefined`, no dynamic typing)\",\"delText\":\"  Break only for syntax (no `undefined`, no dynamic typing) and naming\",\"spans\":{\"del\":[[0,2],[59,70]],\"add\":[[0,17]]},\"runs\":[{\"kind\":\"del\",\"text\":\"  \"},{\"kind\":\"add\",\"color\":\"text\",\"text\":\"      rewrites). \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"Break only for syntax (no `undefined`, no dynamic typing)\"},{\"kind\":\"del\",\"text\":\" and naming\"}]}";
    let new_line = "{\"t\":\"line\",\"kind\":\"mod\",\"newLine\":170,\"oldLine\":164,\"text\":\"      rewrites). Break only for syntax (no `undefined`, no dynamic typing)\",\"delText\":\"  Break only for syntax (no `undefined`, no dynamic typing) and naming\",\"spans\":{\"del\":[[0,2],[59,70]],\"add\":[[0,17]]},\"runs\":[{\"kind\":\"add\",\"color\":\"text\",\"text\":\"    \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"  \"},{\"kind\":\"add\",\"color\":\"text\",\"text\":\"rewrites). \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"Break only for syntax (no `undefined`, no dynamic typing)\"},{\"kind\":\"del\",\"text\":\" and naming\"}]}";

    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(220)),
        row(RowKind::Add, new_line, Some(220), None),
    ]);

    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(
        c.len(),
        1,
        "must pair into 1 mod row with raised token product cap"
    );
    assert_eq!(c[0].kind(), RowKind::Mod);
    let spans = c[0].spans().expect("mod spans must be present");
    assert!(!spans.del.is_empty(), "del spans must not be empty");
    assert!(!spans.add.is_empty(), "add spans must not be empty");
    assert_ne!(spans.del, vec![(0, old_line.len())]);
    assert_ne!(spans.add, vec![(0, new_line.len())]);
}

#[test]
fn test_json_property_modification_pairs_when_text_appears_elsewhere() {
    let old_content = vec![
        "                  \"color\": \"text\",",
        "                  \"text\": \"        \"",
        "                },",
        "                  \"text\": \"        \"",
    ];
    let new_content = vec![
        "                  \"color\": \"text\",",
        "                  \"text\": \"    \"",
        "                },",
        "                  \"text\": \"        \"",
    ];

    let h = hunk(vec![
        Row::Ctx(CtxLine::new(
            LineNum::new(2027).unwrap(),
            LineNum::new(2028).unwrap(),
            "                  \"color\": \"text\",",
        )),
        Row::Del(DelLine::new(
            LineNum::new(2028).unwrap(),
            "                  \"text\": \"        \"",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2029).unwrap(),
            "                  \"text\": \"    \"",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(2029).unwrap(),
            LineNum::new(2030).unwrap(),
            "                },",
        )),
    ]);

    let profile = lang_profile("json");
    let c = collapse_hunk_rows(&h, Some(&old_content), Some(&new_content), profile);

    let mod_row = c.iter().find(|r| r.kind() == RowKind::Mod);
    assert!(
        mod_row.is_some(),
        "del 2028 and add 2029 must pair into a Mod row, got: {:?}",
        c
    );
    let m = mod_row.unwrap();
    assert_eq!(m.old_line(), Some(2028));
    assert_eq!(m.new_line(), Some(2029));
    assert_eq!(
        m.del_text(),
        Some("                  \"text\": \"        \"")
    );
    assert_eq!(m.text(), "                  \"text\": \"    \"");
}

#[test]
fn test_palette_tweaks_render_txt_ansi_pair() {
    let del = "{48;5;22}{38;5;151} 148 {38;5;120}+{39}{23}{48;5;22}            {48;5;22}{48;5;22}sim{48;5;22}{48;5;22}{38;5;117}:{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;147}if{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}ot{48;5;22}{48;5;22}{38;5;117}.{39}{48;5;22}{48;5;22}trimmed{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;117}={39}{48;5;22}{48;5;22}{38;5;117}={39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}nt{48;5;22}{48;5;22}{38;5;117}.{39}{48;5;22}{48;5;22}trimmed{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;117}{{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;221}1.0{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;117}}{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;147}else{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;117}{{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;221}0.0{39}{48;5;22}{48;5;22} {48;5;22}{48;5;22}{38;5;117}}{39}{48;5;22}{48;5;22}{38;5;117},{39}{48;5;22}{48;5;22}{0}";
    let add = "{48;2;22;38;23}{38;5;120} 148 {38;5;120}+{39}{23}{48;2;22;38;23}            {48;2;22;38;23}{48;2;22;38;23}sim{48;2;22;38;23}{48;2;22;38;23}{38;5;117}:{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;147}if{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}ot{48;2;22;38;23}{48;2;22;38;23}{38;5;117}.{39}{48;2;22;38;23}{48;2;22;38;23}trimmed{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;117}={39}{48;2;22;38;23}{48;2;22;38;23}{38;5;117}={39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}nt{48;2;22;38;23}{48;2;22;38;23}{38;5;117}.{39}{48;2;22;38;23}{48;2;22;38;23}trimmed{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;117}{{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;221}1.0{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;117}}{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;147}else{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;117}{{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;221}0.0{39}{48;2;22;38;23}{48;2;22;38;23} {48;2;22;38;23}{48;2;22;38;23}{38;5;117}}{39}{48;2;22;38;23}{48;2;22;38;23}{38;5;117},{39}{48;2;22;38;23}{48;2;22;38;23}{0}";

    let h = hunk(vec![
        row(RowKind::Del, del, None, Some(148)),
        row(RowKind::Add, add, Some(148), None),
    ]);

    let c = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].kind(), RowKind::Del);
    assert_eq!(c[1].kind(), RowKind::Add);
    assert!(c.iter().all(|r| !matches!(r, Row::Mod { .. })));
}
