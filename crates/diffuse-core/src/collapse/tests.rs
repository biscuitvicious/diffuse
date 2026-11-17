//! Ports of the collapse tests from test/review.test.ts: the similarity
//! pairing + the declaration section-stop, the `None` ↔ `Some(result)`
//! brace-anchor, the reflow pairing, the reverse stretch.

use std::time::Instant;

use super::*;
use crate::{
    collapse::collapse_hunk_rows,
    merge::{RunKind, merged_runs},
    moves::mark_moves,
    parse::{AddLine, CtxLine, DelLine, DiffFile, FileStatus, Hunk, Row, RowKind},
};
use diffuse_syntax::registry::lang_profile;

fn row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    match kind {
        RowKind::Del => {
            let num = old_line.expect("Del row requires old_line in test helper");
            Row::Del(DelLine::new(num, text))
        }
        RowKind::Add => {
            let num = new_line.expect("Add row requires new_line in test helper");
            Row::Add(AddLine::new(num, text))
        }
        RowKind::Mod => {
            let old_num = old_line.expect("Mod row requires old_line in test helper");
            let new_num = new_line.expect("Mod row requires new_line in test helper");
            Row::mod_row(DelLine::new(old_num, text), AddLine::new(new_num, text))
        }
        RowKind::Ctx => {
            let old_num = old_line.expect("Ctx row requires old_line in test helper");
            let new_num = new_line.expect("Ctx row requires new_line in test helper");
            Row::Ctx(CtxLine::new(old_num, new_num, text))
        }
        RowKind::Mov => {
            if let Some(old) = old_line {
                Row::mov_src(DelLine::new(old, text))
            } else {
                let new_num =
                    new_line.expect("Mov row requires new_line when not old_line in test helper");
                Row::mov_tgt(AddLine::new(new_num, text))
            }
        }
        RowKind::File | RowKind::Sep => Row::Sep(text.to_string()),
    }
}

fn moved_row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    match kind {
        RowKind::Del => Row::mov_src(DelLine::new(
            old_line.expect("old_line required in test helper"),
            text,
        )),
        _ => Row::mov_tgt(AddLine::new(
            new_line.expect("new_line required in test helper"),
            text,
        )),
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
        has_file_headers: false,
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

// A preceding `}` is not a sound return-slot signal: it can also close a
// block inside a continued expression. The del `None` therefore pairs by
// content with the mid-block `return None;`, and `Some(result)` after the
// brace remains an add.
#[test]
fn brace_close_tail_does_not_claim_slot() {
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
        .find(|r| r.kind() == RowKind::Mod && r.old_text() == Some("        None"))
        .expect("the None mod");
    assert_eq!(
        none_mod.new_line(),
        Some(153),
        "content pairs None with the verbatim `return None;`"
    );
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.new_line() == Some(155)),
        "`Some(result)` after the brace stays an add"
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
        .find(|r| r.kind() == RowKind::Mod && r.old_text() == Some("    return None"))
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
    assert_eq!(c.len(), 3, "1 anchor mod + 2 aux dels");
    let m = &c[0];
    assert_eq!(m.kind(), RowKind::Mod);
    assert_eq!(m.old_line(), Some(756));
    assert_eq!(m.new_line(), Some(912));
    assert_eq!(m.group(), Some(RowGroup::Anchor));

    assert_eq!(c[1].kind(), RowKind::Del);
    assert_eq!(c[1].old_line(), Some(757));
    assert_eq!(c[1].group(), Some(RowGroup::Aux { anchor: 912 }));

    assert_eq!(c[2].kind(), RowKind::Del);
    assert_eq!(c[2].old_line(), Some(758));
    assert_eq!(c[2].group(), Some(RowGroup::Aux { anchor: 912 }));
    let runs = crate::merge::merged_runs(m);
    assert_eq!(crate::merge::project_reverse(&runs), m.old_text().unwrap());
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
    assert_eq!(c2.len(), 3, "1 anchor mod + 2 aux dels");
    let m2 = &c2[0];
    assert_eq!(m2.kind(), RowKind::Mod);
    assert_eq!(m2.old_line(), Some(745));
    assert_eq!(m2.new_line(), Some(903));
    assert_eq!(m2.group(), Some(RowGroup::Anchor));

    assert_eq!(c2[1].kind(), RowKind::Del);
    assert_eq!(c2[1].old_line(), Some(746));
    assert_eq!(c2[1].group(), Some(RowGroup::Aux { anchor: 903 }));

    assert_eq!(c2[2].kind(), RowKind::Del);
    assert_eq!(c2[2].old_line(), Some(747));
    assert_eq!(c2[2].group(), Some(RowGroup::Aux { anchor: 903 }));

    let runs2 = crate::merge::merged_runs(m2);
    assert_eq!(
        crate::merge::project_reverse(&runs2),
        m2.old_text().unwrap()
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
    assert_eq!(c.len(), 3, "1 anchor mod + 2 aux dels");
    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(c[0].old_line(), Some(745));
    assert_eq!(c[0].new_line(), Some(903));
    assert_eq!(c[0].group(), Some(RowGroup::Anchor));
    assert_eq!(c[1].kind(), RowKind::Del);
    assert_eq!(c[1].old_line(), Some(746));
    assert_eq!(c[1].group(), Some(RowGroup::Aux { anchor: 903 }));
    assert_eq!(c[2].kind(), RowKind::Del);
    assert_eq!(c[2].old_line(), Some(747));
    assert_eq!(c[2].group(), Some(RowGroup::Aux { anchor: 903 }));
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

// Tail guard and reflow corpus: block lines keep new-file order, an add
// matching a moved del survives, and a trailing expression after a block
// close does not manufacture a zero-overlap tail pair.
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
        Some(153),
        "content pairs old None with the verbatim `return None;`"
    );
    assert_eq!(mod3.old_text(), Some("        None"));
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.new_line() == Some(155)),
        "`Some(result)` after the brace stays an add"
    );
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
        rmod.old_text(),
        Some("        self.mount_stack"),
        "the mod row's old_text is strictly single-line"
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
            None,
            Some(612),
        ),
        row(
            RowKind::Add,
            "        self.layers = resolved;",
            Some(770),
            None,
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
        .map(|&(s, e)| &row_81.old_text().unwrap()[s..e])
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
    let rs = diffuse_syntax::registry::lang_profile("rs");
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
            2027,
            2028,
            "                  \"color\": \"text\",",
        )),
        Row::Del(DelLine::new(
            2028,
            "                  \"text\": \"        \"",
        )),
        Row::Add(AddLine::new(2029, "                  \"text\": \"    \"")),
        Row::Ctx(CtxLine::new(2029, 2030, "                },")),
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
        m.old_text(),
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

#[test]
fn test_multiline_to_single_line_collapse_reflow() {
    let old_dels = vec![
        row(
            RowKind::Del,
            "pub(crate) fn is_statement_start(",
            None,
            Some(512),
        ),
        row(RowKind::Del, "    text: &str,", None, Some(513)),
        row(
            RowKind::Del,
            "    profile: Option<&crate::syntax::profile::LangProfile>,",
            None,
            Some(514),
        ),
        row(RowKind::Del, ") -> bool {", None, Some(515)),
    ];
    let new_adds = vec![row(
        RowKind::Add,
        "pub(crate) fn is_statement_start(text: &str, profile: Option<&LangProfile>) -> bool {",
        Some(515),
        None,
    )];

    let mut rows = old_dels;
    rows.extend(new_adds);
    let h = hunk(rows);
    let profile = lang_profile("rust");

    let c = collapse_hunk_rows(&h, None, None, profile);

    // Assert: 1 anchor Mod row + 3 AuxDel rows
    assert_eq!(
        c.len(),
        4,
        "Expected 1 anchor Mod row + 3 aux dels, got: {:?}",
        c
    );
    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(c[0].old_line(), Some(512));
    assert_eq!(c[0].new_line(), Some(515));
    assert_eq!(c[0].group(), Some(RowGroup::Anchor));

    assert_eq!(c[1].kind(), RowKind::Del);
    assert_eq!(c[1].old_line(), Some(513));
    assert_eq!(c[1].group(), Some(RowGroup::Aux { anchor: 515 }));

    assert_eq!(c[2].kind(), RowKind::Del);
    assert_eq!(c[2].old_line(), Some(514));
    assert_eq!(c[2].group(), Some(RowGroup::Aux { anchor: 515 }));

    assert_eq!(c[3].kind(), RowKind::Del);
    assert_eq!(c[3].old_line(), Some(515));
    assert_eq!(c[3].group(), Some(RowGroup::Aux { anchor: 515 }));

    // Verify subword runs: crate::syntax::profile:: must be struck out on line 514
    let runs = merged_runs(&c[2]);
    let struck: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    assert!(
        struck
            .iter()
            .any(|s| s.contains("crate::syntax::profile::")),
        "Expected crate::syntax::profile:: to be struck out in the aux del row, got struck: {:?}",
        struck
    );
    let common: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::MovedSrc)
        .map(|r| r.text)
        .collect();
    assert!(
        common.iter().any(|s| s.contains("profile: Option<&")),
        "Expected profile: Option<& to be shared context/moved text, got common: {:?}",
        common
    );
    let runs3 = merged_runs(&c[3]);
    let common3: Vec<&str> = runs3
        .iter()
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::MovedSrc)
        .map(|r| r.text)
        .collect();
    assert!(
        common3.iter().any(|s| s.contains("-> bool {")),
        "Expected -> bool {{ to be shared context/moved text on line 515, got common: {:?}",
        common3
    );
}

#[test]
fn test_linenum_u32_lcs_collapse_walk_stepwise() {
    let patch = include_str!("../../../../tests/golden/linenum-u32/diff.patch");
    let patch_lines: Vec<&str> = patch.lines().collect();
    let file_patch = patch_lines[3948..3977].join("\n");
    let files = diffuse_parse::parse_diff(&file_patch);
    let hunk = &files[0].hunks[0];
    let rs = lang_profile("rs");
    // Call assemble and walk
    let mut p1 = crate::cluster::pairing::dp_pairing_with_splits(hunk, None, None, rs, &[]);
    crate::collapse::pass_residual_pairing(hunk, &mut p1, None, None, rs);
    let paired_dels: rustc_hash::FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: rustc_hash::FxHashMap<usize, usize> =
        p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();
    let pairing_ctx = super::PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        collapse_blocks: &p1.collapse_blocks,
        expand_blocks: &p1.expand_blocks,
    };
    let p2 = crate::collapse::assemble::pass_assemble_mods(hunk, &pairing_ctx, rs);
    let p3 = crate::collapse::reflow::pass_filter_reflows(
        hunk,
        &p2.out,
        &p2.kept_dels,
        &p2.pure_adds,
        &p2.del_groups,
        rs,
    );
    let res = crate::collapse::walk::pass_sort_ordered_walk(hunk, p2, p3);
    let mut prev_old = None;
    for (i, r) in res.iter().enumerate() {
        if let Some(ol) = r.old_line() {
            if let Some(prev) = prev_old {
                assert_eq!(
                    ol,
                    prev + 1,
                    "old_line non-stepwise at row {i}: expected {}, got {}",
                    prev + 1,
                    ol
                );
            }
            prev_old = Some(ol);
        }
    }
}

#[test]
fn test_parallel_assemble_mods_multi_row_parity() {
    let mut rows = Vec::new();
    for i in 1..=32 {
        rows.push(row(
            RowKind::Del,
            &format!("    let count_{i} = calculate_value(param_{i});"),
            None,
            Some(i as u32),
        ));
    }
    for i in 1..=32 {
        rows.push(row(
            RowKind::Add,
            &format!("    let count_{i} = calculate_value(param_{i}, extra_flag);"),
            Some(i as u32),
            None,
        ));
    }
    let h = hunk(rows);
    let collapsed = collapse_hunk_rows(&h, None, None, None);
    assert_eq!(collapsed.len(), 32, "Expected 32 Mod rows");
    for (idx, r) in collapsed.iter().enumerate() {
        let i = idx + 1;
        match r {
            Row::Mod {
                old_line,
                new_line,
                spans,
                ..
            } => {
                assert_eq!(old_line.num, i as u32);
                assert_eq!(new_line.num, i as u32);
                assert_eq!(
                    old_line.text,
                    format!("    let count_{i} = calculate_value(param_{i});")
                );
                assert_eq!(
                    new_line.text,
                    format!("    let count_{i} = calculate_value(param_{i}, extra_flag);")
                );
                assert!(!spans.is_empty(), "Mod row must have subword spans");
            }
            _ => panic!("Expected Row::Mod at index {idx}, got {r:?}"),
        }
    }
}

#[test]
fn test_expansion_group_anchor_and_aux_tagging() {
    use crate::parse::RowGroup;
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
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);
    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(
        c[0].group(),
        Some(RowGroup::Anchor),
        "Expansion anchor row must have RowGroup::Anchor"
    );
    for (idx, r) in c[1..].iter().enumerate() {
        assert_eq!(r.kind(), RowKind::Add);
        assert_eq!(
            r.group(),
            Some(RowGroup::Aux { anchor: 1 }),
            "Expansion auxiliary add row at index {idx} must have RowGroup::Aux with anchor 1"
        );
    }
}

#[test]
fn test_auxiliary_deletion_in_expansion_block_receives_row_group_aux() {
    use crate::parse::RowGroup;
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    /// movs, notes). At most five segments exist, stored inline. Empty when",
            None,
            Some(53),
        ),
        row(
            RowKind::Del,
            "    /// [`FileCounts::is_empty`].",
            None,
            Some(54),
        ),
        row(
            RowKind::Del,
            "    pub fn segments(&self) -> SmallVec<[CountSegment; 5]> {",
            None,
            Some(55),
        ),
        row(
            RowKind::Add,
            "    /// movs, notes, in_progress, resolved). At most seven segments exist,",
            Some(67),
            None,
        ),
        row(
            RowKind::Add,
            "    /// stored inline. Empty when [`FileCounts::is_empty`].",
            Some(68),
            None,
        ),
        row(
            RowKind::Add,
            "    pub fn segments(&self) -> SmallVec<[CountSegment; 7]> {",
            Some(69),
            None,
        ),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(
        c[0].group(),
        Some(RowGroup::Anchor),
        "First row must be expansion anchor"
    );

    // The deletion row for old line 54 should be tagged as an aux del of anchor 67
    let del_54 = c
        .iter()
        .find(|r| r.old_line() == Some(54))
        .expect("old line 54 must be present");
    assert_eq!(del_54.kind(), RowKind::Del);
    assert_eq!(
        del_54.group(),
        Some(RowGroup::Aux { anchor: 67 }),
        "Deletion inside expansion block must have RowGroup::Aux with anchor 67"
    );
}

fn kinds_texts(runs: &[(RunKind, String)]) -> Vec<(&str, &str)> {
    runs.iter()
        .map(|(k, t)| {
            (
                match k {
                    RunKind::Del => "del",
                    RunKind::MovedTgt => "ctx",
                    RunKind::Add => "add",
                    RunKind::Ctx => "ctx",
                    RunKind::MovedSrc => "moved",
                },
                t.as_str(),
            )
        })
        .collect()
}

#[test]
fn test_relabel_moved_runs_empty_moved_is_identity() {
    use crate::collapse::relabel_moved_runs;
    let runs = vec![
        (RunKind::Ctx, "    assert_eq!(".to_string()),
        (RunKind::Del, "super::x".to_string()),
    ];
    let out = relabel_moved_runs(runs.clone(), &[]);
    assert_eq!(kinds_texts(&out), kinds_texts(&runs));
}

#[test]
fn test_relabel_moved_runs_full_coverage() {
    use crate::collapse::relabel_moved_runs;
    let runs = vec![
        (RunKind::Ctx, "    assert_eq!(".to_string()),
        (RunKind::Del, "super::x".to_string()),
    ];
    let out = relabel_moved_runs(runs, &[(15, 23)]);
    assert_eq!(
        kinds_texts(&out),
        vec![("ctx", "    assert_eq!("), ("moved", "super::x")]
    );
}

#[test]
fn test_relabel_moved_runs_partial_coverage_splits() {
    use crate::collapse::relabel_moved_runs;
    // Old tail `a, b`: `a` moved away, `, ` genuinely deleted, `b` moved.
    let runs = vec![(RunKind::Del, "a, b".to_string())];
    let out = relabel_moved_runs(runs, &[(0, 1), (3, 4)]);
    assert_eq!(
        kinds_texts(&out),
        vec![("moved", "a"), ("del", ", "), ("moved", "b")]
    );
}

#[test]
fn test_relabel_moved_runs_leaves_add_and_ctx_alone() {
    use crate::collapse::relabel_moved_runs;
    let runs = vec![
        (RunKind::Ctx, "ab".to_string()),
        (RunKind::Add, "cd".to_string()),
        (RunKind::Del, "ab".to_string()),
    ];
    // Moved range covers the del run's span only; ctx/add runs keep kinds
    // (add runs consume new-side text, never old offsets, so the del run
    // sits at old [2, 4) despite preceding the add in run order).
    let out = relabel_moved_runs(runs, &[(2, 4)]);
    assert_eq!(
        kinds_texts(&out),
        vec![("ctx", "ab"), ("add", "cd"), ("moved", "ab")]
    );
}

#[test]
fn test_relabel_moved_runs_clamps_out_of_range() {
    use crate::collapse::relabel_moved_runs;
    let runs = vec![(RunKind::Del, "ab".to_string())];
    let out = relabel_moved_runs(runs, &[(1, 99)]);
    assert_eq!(kinds_texts(&out), vec![("del", "a"), ("moved", "b")]);
    let runs = vec![(RunKind::Del, "ab".to_string())];
    let out = relabel_moved_runs(runs, &[(5, 9)]);
    assert_eq!(kinds_texts(&out), vec![("del", "ab")]);
}

/// Seeded anchors must not swallow run members: a verbatim seed pairs its
/// anchor 1-to-1, and sibling dels of the same run still render (as
/// reflow pairs or standalone dels) — never vanish. Every old line
/// renders.
#[test]
fn seeded_anchor_does_not_absorb_run_members() {
    // Mirrors the `word_spans` early-return refactor: old `};` (indent 8)
    // seeds with new `};` (indent 4); the DP pairs the `WordSpans {`
    // bodies; the leftover lone-`}` dels must reflow-pair, never vanish.
    let h = hunk(vec![
        row(RowKind::Del, "        };", None, Some(549)),
        row(RowKind::Del, "    }", None, Some(550)),
        row(RowKind::Del, "    WordSpans {", None, Some(551)),
        row(RowKind::Del, "        del: x,", None, Some(552)),
        row(RowKind::Del, "        add: y,", None, Some(553)),
        row(RowKind::Del, "    }", None, Some(554)),
        row(RowKind::Add, "        }", Some(844), None),
        row(RowKind::Add, "    } else {", Some(845), None),
        row(RowKind::Add, "    WordSpans {", Some(846), None),
        row(RowKind::Add, "        del: x,", Some(847), None),
        row(RowKind::Add, "        add: y,", Some(848), None),
        row(RowKind::Add, "        }", Some(849), None),
        row(RowKind::Add, "    };", Some(850), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);
    let rendered_old: std::collections::HashSet<u32> =
        c.iter().filter_map(|r| r.old_line()).collect();
    for n in 549..=554 {
        assert!(
            rendered_old.contains(&n),
            "old line {n} must render (Mod, Del, or reflow), got olds {rendered_old:?}"
        );
    }
    // The verbatim `};` relocation still pairs as the anchor.
    let anchor = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod && r.old_line() == Some(549))
        .expect("seeded }; anchor must pair");
    assert_eq!(anchor.new_line(), Some(850));
}

/// In a 1-to-K expansion where an import list expands into multiple imports,
/// unrelated additions (e.g. `io::{self, Write}`) must not hijack the
/// structural delimiters (`::{`, `,`, `}`) from the donor line's true
/// continuation row (`sync::{Arc, Mutex, MutexGuard}`).
#[test]
fn test_import_block_expansion_delimiter_hijacking() {
    let h = hunk(vec![
        row(RowKind::Del, "use std::sync::{Arc, Mutex};", None, Some(1)),
        row(RowKind::Add, "use std::{", Some(1), None),
        row(RowKind::Add, "    fs,", Some(2), None),
        row(RowKind::Add, "    io::{self, Write},", Some(3), None),
        row(
            RowKind::Add,
            "    sync::{Arc, Mutex, MutexGuard},",
            Some(4),
            None,
        ),
        row(RowKind::Add, "};", Some(5), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Line 2 (fs,) and Line 3 (io::{self, Write},) are independent additions.
    // They must NOT hijack delimiters (::{, }, ,) from the donor line.
    let io_row = c
        .iter()
        .find(|r| r.text().contains("io::{"))
        .expect("io row");
    let io_runs = merged_runs(io_row);
    let io_moved_runs: Vec<_> = io_runs
        .iter()
        .filter(|r| r.kind == RunKind::MovedTgt)
        .collect();
    assert!(
        io_moved_runs.is_empty(),
        "io row must not claim MovedTgt tokens from donor line, but claimed: {io_moved_runs:?}"
    );

    // Line 4 (sync::{Arc, Mutex, MutexGuard},) is the true reflow destination of
    // sync::{Arc, Mutex}. Its own ::{ must NOT be an Add while sync is
    // MovedTgt.
    let sync_row = c
        .iter()
        .find(|r| r.text().contains("sync::{"))
        .expect("sync row");
    let sync_runs = merged_runs(sync_row);
    let delim_run = sync_runs
        .iter()
        .find(|r| r.text.contains("::{"))
        .expect("sync row ::{ run");
    assert_ne!(
        delim_run.kind,
        RunKind::Add,
        "sync row's ::{{ delimiter must not be demoted to Add by another line's claim"
    );

    // Concatenating the non-add tokens across the visual reflow group:
    // Anchor context + aux MovedTgt tokens must reconstruct the exact original old
    // line.
    let mut reconstructed = String::new();
    for r in &c {
        for run in merged_runs(r) {
            if run.kind == RunKind::Ctx || run.kind == RunKind::MovedTgt {
                reconstructed.push_str(run.text);
            }
        }
    }
    assert_eq!(
        reconstructed, "use std::sync::{Arc, Mutex};",
        "Reflow group non-add runs must reconstruct original old line, got: {reconstructed:?}"
    );
}

#[test]
fn test_import_expansion_reaches_terminal_close_delimiter() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "use std::{collections::HashMap, sync::Arc};",
            None,
            Some(23),
        ),
        row(RowKind::Add, "use std::{", Some(23), None),
        row(RowKind::Add, "    borrow::Cow,", Some(24), None),
        row(RowKind::Add, "    collections::HashMap,", Some(25), None),
        row(RowKind::Add, "    fs, mem,", Some(26), None),
        row(RowKind::Add, "    num::NonZero,", Some(27), None),
        row(RowKind::Add, "    path::Path,", Some(28), None),
        row(RowKind::Add, "    sync::{", Some(29), None),
        row(RowKind::Add, "        Arc,", Some(30), None),
        row(
            RowKind::Add,
            "        atomic::{self, AtomicU64},",
            Some(31),
            None,
        ),
        row(RowKind::Add, "    },", Some(32), None),
        row(RowKind::Add, "    time::Instant,", Some(33), None),
        row(RowKind::Add, "};", Some(34), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Line 34 (};) must be part of the reflow group with anchor 23
    let last_row = c
        .iter()
        .find(|r| r.text() == "};")
        .expect("terminal }; row");
    assert_eq!(
        last_row.group(),
        Some(crate::parse::RowGroup::Aux { anchor: 23 }),
        "terminal '}};' row must be in the reflow group as Aux with anchor 23"
    );

    // Line 23 (use std::{) must NOT have a struck ';'
    let anchor_row = &c[0];
    let anchor_del_runs: Vec<_> = merged_runs(anchor_row)
        .into_iter()
        .filter(|r| r.kind == RunKind::Del && r.text.contains(';'))
        .collect();
    assert!(
        anchor_del_runs.is_empty(),
        "anchor row must not strike out ';' when terminal '}};' is part of reflow group, but found: {anchor_del_runs:?}"
    );

    // The anchor row reconstructs the old line in full (Ctx + MovedSrc + Del).
    let anchor_reconstructed: String = merged_runs(&c[0])
        .into_iter()
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::MovedSrc || r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    assert_eq!(
        anchor_reconstructed, "use std::{collections::HashMap, sync::Arc};",
        "Anchor row must reconstruct the old line"
    );

    // Concatenating surviving runs (Ctx + MovedTgt) across the reflow rows matches
    // surviving tokens.
    let surviving: String = c
        .iter()
        .flat_map(merged_runs)
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::MovedTgt)
        .map(|r| r.text)
        .collect();
    assert_eq!(
        surviving, "use std::{collections::HashMap,sync::Arc};",
        "Surviving text across reflow rows must match"
    );
}

#[test]
fn test_import_expansion_with_misaligned_brace_indentation() {
    // Del line is indented with 4 spaces; terminal brace has 8 spaces (misaligned).
    let h = hunk(vec![
        row(
            RowKind::Del,
            "    use std::{collections::HashMap, sync::Arc};",
            None,
            Some(10),
        ),
        row(RowKind::Add, "    use std::{", Some(10), None),
        row(
            RowKind::Add,
            "        collections::HashMap,",
            Some(11),
            None,
        ),
        row(RowKind::Add, "        sync::{", Some(12), None),
        row(RowKind::Add, "            Arc,", Some(13), None),
        row(RowKind::Add, "          },", Some(14), None), // misaligned inner brace (10 spaces)
        row(RowKind::Add, "        };", Some(15), None),   /* misaligned terminal brace (8
                                                            * spaces vs del's 4) */
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Terminal row must be part of the reflow group with anchor 10
    let last_row = c
        .iter()
        .find(|r| r.text().trim() == "};")
        .expect("terminal }; row");
    assert_eq!(
        last_row.group(),
        Some(crate::parse::RowGroup::Aux { anchor: 10 }),
        "terminal '}};' row with misaligned indent must still be in the reflow group"
    );

    // Anchor row must not strike out ';'
    let anchor_del_runs: Vec<_> = merged_runs(&c[0])
        .into_iter()
        .filter(|r| r.kind == RunKind::Del && r.text.contains(';'))
        .collect();
    assert!(
        anchor_del_runs.is_empty(),
        "anchor row must not strike out ';' when terminal '}};' is part of reflow group"
    );

    // Terminal row must have }; marked as MovedTgt, with leading indent marked as
    // Add
    let tgt_runs: Vec<_> = merged_runs(last_row)
        .into_iter()
        .filter(|r| r.kind == RunKind::MovedTgt)
        .map(|r| r.text)
        .collect();
    assert_eq!(
        tgt_runs,
        vec!["};"],
        "misaligned terminal row must claim '}};' as MovedTgt"
    );
}

#[test]
fn test_import_nesting_outer_wrapper_braces_are_add_fill() {
    let h = hunk(vec![
        row(
            RowKind::Del,
            "use std::io::{BufRead, Write};",
            None,
            Some(7),
        ),
        row(RowKind::Add, "use std::{", Some(7), None),
        row(
            RowKind::Add,
            "    io::{self, BufRead, Write},",
            Some(8),
            None,
        ),
        row(RowKind::Add, "    ops,", Some(9), None),
        row(RowKind::Add, "};", Some(10), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    for r in &c {
        eprintln!("ROW {:?} (group={:?}): {:?}", r.kind(), r.group(), r.text());
        for run in merged_runs(r) {
            eprintln!("  RUN {:?}: {:?}", run.kind, run.text);
        }
    }

    // Reconstruction invariant: concatenating non-add runs across the group must
    // yield the original line "use std::io::{BufRead, Write};"
    let reconstructed: String = c
        .iter()
        .flat_map(merged_runs)
        .filter(|r| r.kind != RunKind::Add && r.kind != RunKind::Del && r.kind != RunKind::MovedSrc)
        .map(|r| r.text)
        .collect();
    assert_eq!(
        reconstructed, "use std::io::{BufRead, Write};",
        "Non-add runs across reflow group must reconstruct original line"
    );
}

#[test]
fn test_multiline_import_collapse_nested_core() {
    let diff = r#"diff --git a/crates/diffuse-tui/src/app/geometry/tests.rs b/crates/diffuse-tui/src/app/geometry/tests.rs
--- a/crates/diffuse-tui/src/app/geometry/tests.rs
+++ b/crates/diffuse-tui/src/app/geometry/tests.rs
@@ -2,7 +2,13 @@ use std::collections::HashMap;
 
 use crate::{
     app::{AppState, temp_notes_path},
-    core::{PipelineOptions, parse::RowKind, pipeline::run_pipeline_full, wire::PipelineOutput},
+    core::{
+        PipelineOptions,
+        merge::RunKind,
+        parse::RowKind,
+        pipeline::run_pipeline_full,
+        wire::{OutputRun, PipelineOutput},
+    },
     view::{truncate_pad, visible_width},
 };
"#;
    let out = crate::pipeline::run_pipeline_diff(diff);
    let rows = &out.files[0].hunks[0].rows;
    assert!(
        rows.iter().any(|r| r.kind == crate::parse::RowKind::Mod),
        "Expected core::{{...}} reflow to pair into Mod"
    );
}

#[test]
fn test_multiline_inter_argument_residue_localized_to_aux_tail() {
    let old_line = "check!(r.del_text().is_some(), \"{}: mod row carries delText\", f.path);";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(332)),
        row(RowKind::Add, "check!(", Some(332), None),
        row(RowKind::Add, "    r.del_text().is_some(),", Some(333), None),
        row(
            RowKind::Add,
            "    \"{}: mod row carries delText\",",
            Some(334),
            None,
        ),
        row(RowKind::Add, "    f.path", Some(335), None),
        row(RowKind::Add, ");", Some(336), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Anchor row should not have orphan Del runs for inter-argument residues
    let anchor_del_runs: Vec<_> = merged_runs(&c[0])
        .into_iter()
        .filter(|r| r.kind == RunKind::Del)
        .collect();
    assert!(
        anchor_del_runs.is_empty(),
        "Anchor row should not retain inter-argument Del residues, but found: {:?}",
        anchor_del_runs
    );

    // Aux line 333 should end with Del(" ")
    let aux1_runs = merged_runs(&c[1]);
    assert_eq!(
        aux1_runs.last().map(|r| (r.kind, r.text)),
        Some((RunKind::Del, " ")),
        "Aux row 1 should end with Del(\" \"), but got runs: {:?}",
        aux1_runs
    );

    // Aux line 334 should end with Del(" ")
    let aux2_runs = merged_runs(&c[2]);
    assert_eq!(
        aux2_runs.last().map(|r| (r.kind, r.text)),
        Some((RunKind::Del, " ")),
        "Aux row 2 should end with Del(\" \"), but got runs: {:?}",
        aux2_runs
    );
}

#[test]
fn test_multiline_deleted_identifier_localized_to_aux_row() {
    let old_line = "check!(a, deleted_arg, c);";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(10)),
        row(RowKind::Add, "check!(", Some(10), None),
        row(RowKind::Add, "    a,", Some(11), None),
        row(RowKind::Add, "    c,", Some(12), None),
        row(RowKind::Add, ");", Some(13), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Anchor row should have no orphan Del runs
    let anchor_del_runs: Vec<_> = merged_runs(&c[0])
        .into_iter()
        .filter(|r| r.kind == RunKind::Del)
        .collect();
    assert!(
        anchor_del_runs.is_empty(),
        "Anchor row should not retain Del residues, but found: {:?}",
        anchor_del_runs
    );

    // Aux row 1 (a,) should have trailing Del containing "deleted_arg"
    let aux1_runs = merged_runs(&c[1]);
    let has_del_arg = aux1_runs
        .iter()
        .any(|r| r.kind == RunKind::Del && r.text.contains("deleted_arg"));
    assert!(
        has_del_arg,
        "Aux row 1 should carry deleted_arg Del run, but got runs: {:?}",
        aux1_runs
    );
}

#[test]
fn test_reflow_atomicity_trailing_deletions_do_not_interleave_anchor_and_aux_add() {
    use crate::parse::RowGroup;
    let h = hunk(vec![
        row(
            RowKind::Del,
            "let pairs = crate::lcs::lcs_pairs_ids(&ot.ids, nt_ids, ws, alnum);",
            None,
            Some(189),
        ),
        row(RowKind::Del, "let mut matched: usize = 0;", None, Some(190)),
        row(RowKind::Del, "for j in pairs.values() {", None, Some(191)),
        row(
            RowKind::Del,
            "    matched += nt_token_chars[*j];",
            None,
            Some(192),
        ),
        row(RowKind::Del, "}", None, Some(193)),
        row(RowKind::Add, "let matched =", Some(191), None),
        row(
            RowKind::Add,
            "    crate::lcs::lcs_matched_chars_ids(&ot.ids, nt_ids, ws, alnum, nt_token_chars, scratch);",
            Some(192),
            None,
        ),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let anchor_pos = c
        .iter()
        .position(|r| r.group() == Some(RowGroup::Anchor))
        .expect("must produce an anchor row");

    let aux_adds: Vec<(usize, &Row)> = c
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            matches!(r.group(), Some(RowGroup::Aux { anchor }) if anchor == 191)
                && matches!(r, Row::AuxAdd { .. } | Row::MovTgt { .. })
        })
        .collect();

    assert!(
        !aux_adds.is_empty(),
        "must produce at least one aux addition"
    );
    for (offset, (idx, _)) in aux_adds.into_iter().enumerate() {
        assert_eq!(
            idx,
            anchor_pos + 1 + offset,
            "Aux addition must be contiguous with anchor without intervening deletions"
        );
    }
}

#[test]
fn test_multiline_reflow_closing_terminator_does_not_attach_to_intermediate_item() {
    let old_line = "    let last_line_hash = hash_line_content(\"    let y = 2;\");";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(308)),
        row(
            RowKind::Add,
            "    let compound_hash = LineHash::from_lines([",
            Some(312),
            None,
        ),
        row(RowKind::Add, "        \"    let x = 1;\",", Some(313), None),
        row(RowKind::Add, "        \"    let y = 2;\",", Some(314), None),
        row(RowKind::Add, "    ]);", Some(315), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let aux1 = c.iter().find(|r| r.text().contains("let x = 1")).unwrap();
    let aux1_dels: Vec<_> = merged_runs(aux1)
        .into_iter()
        .filter(|r| r.kind == RunKind::Del)
        .collect();
    assert!(
        aux1_dels.is_empty(),
        "Intermediate item should not have Del attached, got: {:?}",
        aux1_dels
    );
}

#[test]
fn test_prefix_companion_does_not_steal_anchor_call_arguments() {
    let old_line = "            ctx.theme.write_token(\"text\", text, \"ctx\", None, None, out);";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(376)),
        row(RowKind::Add, "            ctx.theme", Some(381), None),
        row(
            RowKind::Add,
            "                .write_token(TokenColor::Default, text, \"ctx\", None, None, out);",
            Some(382),
            None,
        ),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let pfx = c.iter().find(|r| r.text().contains("ctx.theme")).unwrap();
    let pfx_has_text_del = merged_runs(pfx)
        .into_iter()
        .any(|r| r.kind == RunKind::Del && r.text.contains("text"));
    assert!(
        !pfx_has_text_del,
        "Prefix row ctx.theme must not steal 'text' deletion from method call"
    );
}

#[test]
fn test_residue_inside_call_does_not_append_after_trailing_comma() {
    let old_line = "let line_hash = line_content.as_deref().map(hash_line_content);";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(223)),
        row(
            RowKind::Add,
            "let line_hash = if !selected_lines.is_empty() {",
            Some(224),
            None,
        ),
        row(
            RowKind::Add,
            "    selected_lines.iter().map(|s| s.as_str()),",
            Some(225),
            None,
        ),
        row(RowKind::Add, "} else {", Some(226), None),
        row(RowKind::Add, "    None", Some(227), None),
        row(RowKind::Add, "};", Some(228), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let map_row = c.iter().find(|r| r.text().contains(".map")).unwrap();
    let runs = merged_runs(map_row);
    if let Some(pos) = runs
        .iter()
        .position(|r| r.kind == RunKind::Del && r.text.contains("hash_line_content"))
    {
        let subsequent_runs = &runs[pos + 1..];
        let del_precedes_comma = subsequent_runs.iter().any(|r| r.text.contains(','));
        assert!(
            del_precedes_comma,
            "hash_line_content deletion must be inside map, preceding the trailing comma, but runs are: {:?}",
            runs
        );
    }
}

#[test]
fn test_multiline_call_reflow_does_not_attach_closing_paren_to_anchor() {
    let old_line = "    assert_eq!(super::resolve_theme(Some(\"dark\")), DiffuseTheme::dark());";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(134)),
        row(RowKind::Add, "    assert_eq!(", Some(134), None),
        row(
            RowKind::Add,
            "        super::resolve_theme(Some(\"dark\"), false),",
            Some(135),
            None,
        ),
        row(
            RowKind::Add,
            "        DiffuseTheme::dark()",
            Some(136),
            None,
        ),
        row(RowKind::Add, "    );", Some(137), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let anchor = c.iter().find(|r| r.old_line_num() == Some(134)).unwrap();
    let anchor_runs = merged_runs(anchor);
    // Anchor row assert_eq!( must not contain any deleted paren
    let has_paren_del = anchor_runs
        .iter()
        .any(|r| r.kind == RunKind::Del && r.text.contains(')'));
    assert!(
        !has_paren_del,
        "Anchor row assert_eq!( must not contain deleted ')', but runs are: {:?}",
        anchor_runs
    );

    // Continuation row 1 carries super::resolve_theme call, added arguments,
    // closing delimiters, and deleted space
    let aux1 = c.iter().find(|r| r.new_line_num() == Some(135)).unwrap();
    let aux1_runs = merged_runs(aux1);
    assert_eq!(
        aux1_runs,
        vec![
            crate::merge::Run {
                kind: RunKind::Add,
                text: "        ",
            },
            crate::merge::Run {
                kind: RunKind::MovedTgt,
                text: "super::resolve_theme(Some(\"dark\")",
            },
            crate::merge::Run {
                kind: RunKind::Add,
                text: ", false",
            },
            crate::merge::Run {
                kind: RunKind::MovedTgt,
                text: "),",
            },
            crate::merge::Run {
                kind: RunKind::Del,
                text: " ",
            },
        ]
    );
}

#[test]
fn test_multiline_match_arm_expansion_monotonicity() {
    let old_line = "                Row::Add(add_line) | Row::MovTgt(add_line) => {";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(136)),
        row(
            RowKind::Add,
            "                Row::Add(add_line)",
            Some(136),
            None,
        ),
        row(
            RowKind::Add,
            "                | Row::MovTgt { line: add_line, .. }",
            Some(137),
            None,
        ),
        row(
            RowKind::Add,
            "                | Row::AuxAdd { line: add_line, .. } => {",
            Some(138),
            None,
        ),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    let anchor = &c[0];
    let anchor_runs = merged_runs(anchor);
    let anchor_has_open_paren = anchor_runs
        .iter()
        .any(|r| r.kind == RunKind::Del && r.text.contains('('));
    assert!(
        !anchor_has_open_paren,
        "Anchor line 136 must not contain struck '(', but got: {:?}",
        anchor_runs
    );

    let aux1 = &c[1];
    let aux1_runs = merged_runs(aux1);
    assert_eq!(
        aux1_runs,
        vec![
            crate::merge::Run {
                kind: RunKind::Add,
                text: "                ",
            },
            crate::merge::Run {
                kind: RunKind::MovedTgt,
                text: "| Row::MovTgt",
            },
            crate::merge::Run {
                kind: RunKind::Del,
                text: "(add_line)",
            },
            crate::merge::Run {
                kind: RunKind::Add,
                text: " { line: add_line, .. }",
            },
        ]
    );

    let aux2 = &c[2];
    assert_eq!(
        merged_runs(aux2),
        vec![
            crate::merge::Run {
                kind: RunKind::Add,
                text: "                | Row::AuxAdd { line: add_line, .. }",
            },
            crate::merge::Run {
                kind: RunKind::MovedTgt,
                text: " => {",
            },
        ]
    );
}

#[test]
fn test_multiline_expansion_universal_unicode_delimiters_no_profile() {
    let old_line = "alpha（content）beta";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(1)),
        row(RowKind::Add, "alpha", Some(1), None),
        row(RowKind::Add, "prefix content suffix", Some(2), None),
        row(RowKind::Add, "beta", Some(3), None),
    ]);
    let c = collapse_hunk_rows(&h, None, None, None);
    let aux = &c[1];
    let aux_runs = merged_runs(aux);
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::Del && r.text == "（"),
        "aux must carry opening unicode bracket '（' right before content, got: {:?}",
        aux_runs
    );
}

fn load_scrollbar_grab_drag_dispatch_hunk() -> Hunk {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let patch_path = manifest_dir.join("../../tests/golden/scrollbar-grab-drag/diff.patch");
    let patch = std::fs::read_to_string(&patch_path).expect("patch must exist");
    let files = crate::parse::parse_diff(&patch);
    let dispatch = files
        .iter()
        .find(|f| f.path.contains("dispatch.rs"))
        .expect("dispatch.rs must exist");
    dispatch.hunks[1].clone()
}

#[test]
fn test_dbc_invariant_1_reflow_cluster_confinement() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let p1 = crate::cluster::pairing::dp_pairing(&h, None, None, profile);
    let paired_dels: FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: FxHashMap<usize, usize> = p1.pair_of.iter().map(|(&a, &d)| (d, a)).collect();
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        collapse_blocks: &p1.collapse_blocks,
        expand_blocks: &p1.expand_blocks,
    };
    let p2 = pass_assemble_mods(&h, &pairing_ctx, profile);
    let p3 = pass_filter_reflows(
        &h,
        &p2.out,
        &p2.kept_dels,
        &p2.pure_adds,
        &p2.del_groups,
        profile,
    );

    check_reflow_cluster_confinement(&h, &p3).expect("Reflow cluster confinement violated");
}

#[test]
fn test_dbc_invariant_2_no_orphaned_continuation_dels() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);

    check_no_orphaned_continuation_dels(&h, &res)
        .expect("No orphaned continuation deletions violated");
}

#[test]
fn test_dbc_invariant_3_context_old_line_monotonicity() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);

    check_context_old_line_monotonicity(&res).expect("Context old line monotonicity violated");
}

#[test]
fn test_dbc_invariant_4_del_add_topological_order() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);

    check_del_add_topological_order(&h, &res).expect("Del/add topological order violated");
}

#[test]
fn test_dbc_macro_invariant_1_reflow_cluster_confinement() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let p1 = crate::cluster::pairing::dp_pairing(&h, None, None, profile);
    let paired_dels: FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: FxHashMap<usize, usize> = p1.pair_of.iter().map(|(&a, &d)| (d, a)).collect();
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        collapse_blocks: &p1.collapse_blocks,
        expand_blocks: &p1.expand_blocks,
    };
    let p2 = pass_assemble_mods(&h, &pairing_ctx, profile);
    let p3 = pass_filter_reflows(
        &h,
        &p2.out,
        &p2.kept_dels,
        &p2.pure_adds,
        &p2.del_groups,
        profile,
    );
    crate::ensure!(
        check_reflow_cluster_confinement(&h, &p3).is_ok(),
        "{}",
        check_reflow_cluster_confinement(&h, &p3)
            .err()
            .unwrap_or_default()
    );
}

#[test]
fn test_dbc_macro_invariant_2_no_orphaned_continuation_dels() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);
    crate::invariant!(
        check_no_orphaned_continuation_dels(&h, &res).is_ok(),
        "{}",
        check_no_orphaned_continuation_dels(&h, &res)
            .err()
            .unwrap_or_default()
    );
}

#[test]
fn test_dbc_macro_invariant_3_context_old_line_monotonicity() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);
    crate::invariant!(
        check_context_old_line_monotonicity(&res).is_ok(),
        "{}",
        check_context_old_line_monotonicity(&res)
            .err()
            .unwrap_or_default()
    );
}

#[test]
fn test_dbc_macro_invariant_4_del_add_topological_order() {
    let h = load_scrollbar_grab_drag_dispatch_hunk();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);
    crate::invariant!(
        check_del_add_topological_order(&h, &res).is_ok(),
        "{}",
        check_del_add_topological_order(&h, &res)
            .err()
            .unwrap_or_default()
    );
}

#[test]
fn test_reflow_filter_preserves_context_old_line_monotonicity() {
    let h = hunk(vec![
        Row::Ctx(CtxLine::new(1, 1, "context before")),
        Row::Add(crate::parse::AddLine::new(2, "}")),
        Row::Del(crate::parse::DelLine::new(2, "files.push(item);")),
        Row::Add(crate::parse::AddLine::new(3, "files.push(item);")),
        Row::Del(crate::parse::DelLine::new(3, "}")),
        Row::Ctx(CtxLine::new(4, 4, "context after")),
    ]);
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let res = collapse_hunk_rows(&h, None, None, profile);
    assert!(check_context_old_line_monotonicity(&res).is_ok());
}

#[test]
fn test_token_rs_line_722_not_wholesale_mod() {
    let old = std::fs::read_to_string(
        "../../tests/golden/container-compatibility/files/crates-diffuse-core-src-token.old.rs",
    )
    .or_else(|_| {
        std::fs::read_to_string(
            "tests/golden/container-compatibility/files/crates-diffuse-core-src-token.old.rs",
        )
    })
    .unwrap();
    let new = std::fs::read_to_string(
        "../../tests/golden/container-compatibility/files/crates-diffuse-core-src-token.new.rs",
    )
    .or_else(|_| {
        std::fs::read_to_string(
            "tests/golden/container-compatibility/files/crates-diffuse-core-src-token.new.rs",
        )
    })
    .unwrap();
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let (hunks, _) = diffuse_git::diff_text(&old, &new, 3, diffuse_git::WhitespaceMode::Exact);
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let collapsed =
        crate::collapse::collapse_hunk_rows(&hunks[1], Some(&old_lines), Some(&new_lines), profile);

    // Line 722 must not pair with line 687 as a Mod row!
    for r in &collapsed {
        if let Row::Mod {
            old_line, new_line, ..
        } = r
        {
            assert!(
                !(old_line.num == 687 && new_line.num == 722),
                "Line 687 and 722 must not pair as Mod row; got: {:?}",
                crate::merge::merged_runs(r)
            );
        }
    }
}

#[test]
fn test_aux_opening_delimiter_matches_trailing_delimiter_from_reflowed_anchor() {
    let old_line =
        "                            && matches!(prev, Row::Add(_)) && nl > d_anchor_new {";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(2588)),
        row(
            RowKind::Add,
            "                            && matches!(prev, Row::Add(_))",
            Some(2589),
            None,
        ),
        row(
            RowKind::Add,
            "                            && nl > d_anchor_new",
            Some(2590),
            None,
        ),
        row(RowKind::Add, "                        {", Some(2591), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    // Row 1 (aux for 2590: && nl > d_anchor_new) must NOT contain struck '{'
    let aux1 = &c[1];
    let aux1_runs = merged_runs(aux1);
    assert!(
        !aux1_runs
            .iter()
            .any(|r| r.kind == RunKind::Del && r.text.contains('{')),
        "Line 2590 must not contain struck '{{', got: {:?}",
        aux1_runs
    );

    // Row 2 (aux for 2591: {) must match '{' as MovedTgt
    let aux2 = &c[2];
    let aux2_runs = merged_runs(aux2);
    assert!(
        aux2_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedTgt && r.text == "{"),
        "Line 2591 must have MovedTgt '{{', got: {:?}",
        aux2_runs
    );
}

#[test]
fn test_aux_single_delimiter_reflow_without_alphanumeric_deletion() {
    let old_line = "                    && ol <= prev_old {";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(2455)),
        row(
            RowKind::Add,
            "                    && ol <= prev_old",
            Some(2455),
            None,
        ),
        row(RowKind::Add, "                {", Some(2456), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    assert_eq!(c.len(), 2);
    let anchor = &c[0];
    let anchor_runs = merged_runs(anchor);
    assert_eq!(anchor.group(), Some(RowGroup::Anchor));
    assert!(
        anchor_runs
            .iter()
            .any(|r| r.kind == RunKind::Del && r.text == " "),
        "Preceding space must be struck, got: {:?}",
        anchor_runs
    );
    assert!(
        anchor_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedSrc && r.text == "{"),
        "Anchor must have MovedSrc for '{{', got: {:?}",
        anchor_runs
    );

    let aux = &c[1];
    let aux_runs = merged_runs(aux);
    assert_eq!(aux.group(), Some(RowGroup::Aux { anchor: 2455 }));
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::Add && r.text.contains("                ")),
        "Aux row must have added indentation, got: {:?}",
        aux_runs
    );
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedTgt && r.text == "{"),
        "Aux row must have MovedTgt for '{{', got: {:?}",
        aux_runs
    );
}

#[test]
fn test_aux_delimiter_run_with_whitespace_variation() {
    let old_line = "    let foo = bar [ (";
    let h = hunk(vec![
        row(RowKind::Del, old_line, None, Some(10)),
        row(RowKind::Add, "    let foo = bar", Some(10), None),
        row(RowKind::Add, "        [(", Some(11), None),
    ]);
    let rs = lang_profile("rs");
    let c = collapse_hunk_rows(&h, None, None, rs);

    assert_eq!(c.len(), 2);
    let anchor = &c[0];
    let anchor_runs = merged_runs(anchor);
    assert_eq!(anchor.group(), Some(RowGroup::Anchor));
    assert!(
        anchor_runs
            .iter()
            .any(|r| r.kind == RunKind::Del && r.text.contains(' ')),
        "Preceding whitespace must show as struck Del, got: {:?}",
        anchor_runs
    );
    assert!(
        anchor_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedSrc && r.text.contains('[')),
        "Anchor must have MovedSrc for '[', got: {:?}",
        anchor_runs
    );
    assert!(
        anchor_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedSrc && r.text.contains('(')),
        "Anchor must have MovedSrc for '(', got: {:?}",
        anchor_runs
    );

    let aux = &c[1];
    let aux_runs = merged_runs(aux);
    assert_eq!(aux.group(), Some(RowGroup::Aux { anchor: 10 }));
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::Add && r.text.contains("        ")),
        "Indentation must show as Add, got: {:?}",
        aux_runs
    );
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedTgt && r.text.contains('[')),
        "Aux must have MovedTgt for '[', got: {:?}",
        aux_runs
    );
    assert!(
        aux_runs
            .iter()
            .any(|r| r.kind == RunKind::MovedTgt && r.text.contains('(')),
        "Aux must have MovedTgt for '(', got: {:?}",
        aux_runs
    );
}

#[test]
fn test_residual_declaration_roles_and_compatibility() {
    use super::residual::{
        are_roles_compatible, declaration_role, is_import_statement, non_ws_char_similarity,
    };

    // Declarations
    assert_eq!(declaration_role("pub struct Foo {"), Some("struct"));
    assert_eq!(declaration_role("pub(crate) enum Bar {"), Some("enum"));
    assert_eq!(
        declaration_role("export interface Baz {"),
        Some("interface")
    );
    assert_eq!(declaration_role("async fn run()"), Some("fn"));
    assert_eq!(
        declaration_role("pub const MAX: usize = 10;"),
        Some("const")
    );
    assert_eq!(declaration_role("let x = 1;"), None);
    assert_eq!(declaration_role(""), None);

    // Compatibility
    assert!(are_roles_compatible("struct", "enum"));
    assert!(are_roles_compatible("interface", "type"));
    assert!(are_roles_compatible("const", "static"));
    assert!(!are_roles_compatible("struct", "fn"));
    assert!(!are_roles_compatible("fn", "const"));

    // Imports
    assert!(is_import_statement("use std::collections::HashMap;"));
    assert!(is_import_statement("import { foo } from 'bar';"));
    assert!(is_import_statement("require('fs')"));
    assert!(!is_import_statement("let user = null;"));

    // Char similarity
    let sim = non_ws_char_similarity("let abc = 123;", "let abd = 123;");
    assert!(sim > 0.8);
    let zero = non_ws_char_similarity("", "abc");
    assert_eq!(zero, 0.0);
}

#[test]
fn test_normalize_identical_mods_converts_faithful_pair_to_ctx() {
    let rs = lang_profile("rs");
    let h = hunk(vec![]);
    let res = vec![
        row(RowKind::Ctx, "    handle_event(", Some(920), Some(914)),
        row(RowKind::Mod, "    );", Some(923), Some(917)),
        row(RowKind::Ctx, "    assert_eq!(x);", Some(924), Some(921)),
    ];
    let out = pass_normalize_identical_mods(&h, res, rs, &Default::default());
    assert_eq!(
        out[1].kind(),
        RowKind::Ctx,
        "faithful identical Mod must become Ctx"
    );
    assert_eq!(out[1].old_line(), Some(917));
    assert_eq!(out[1].new_line(), Some(923));
}

#[test]
fn test_normalize_identical_mods_keeps_crossed_pair_as_mod() {
    // Miniature of the theme-light-toggle `);` case: a leftover del with a
    // higher old line sits before the identical pair, so no Ctx placement
    // is legal and the pair must stay Mod (never trip the DBC).
    let rs = lang_profile("rs");
    let h = hunk(vec![]);
    let res = vec![
        row(RowKind::Del, "    assert!(", None, Some(918)),
        row(RowKind::Mod, "    );", Some(923), Some(917)),
    ];
    let out = pass_normalize_identical_mods(&h, res, rs, &Default::default());
    assert_eq!(
        out[1].kind(),
        RowKind::Mod,
        "crossed identical pair must stay Mod"
    );
}

#[test]
fn test_normalize_identical_mods_skips_anchors_but_converts_plain() {
    use crate::parse::Spans;
    let rs = lang_profile("rs");
    let h = hunk(vec![]);
    let anchor = Row::Mod {
        old_line: DelLine::new(1, "    };"),
        new_line: AddLine::new(2, "    };"),
        spans: Spans {
            del: vec![],
            add: vec![],
        },
        cached_runs: vec![],
        is_anchor: true,
    };
    let res = vec![
        anchor,
        row(RowKind::Mod, "    };", Some(4), Some(3)),
        row(RowKind::Del, "    changed;", None, Some(5)),
    ];
    let out = pass_normalize_identical_mods(&h, res, rs, &Default::default());
    assert_eq!(out[0].kind(), RowKind::Mod, "anchor Mod must be untouched");
    assert_eq!(
        out[1].kind(),
        RowKind::Ctx,
        "plain identical Mod must become Ctx"
    );
    assert_eq!(out[2].kind(), RowKind::Del);
}

#[test]
fn test_normalize_identical_mods_skips_block_anchors() {
    // A collapse anchor whose joined del_text equals the add text must stay
    // Mod: absorbed member lines (e.g. opt-2 `win` + `.iter()`) need the
    // anchor to render, and converting it would drop them from the stream.
    use rustc_hash::FxHashSet;
    let rs = lang_profile("rs");
    let h = hunk(vec![]);
    let res = vec![row(RowKind::Mod, "    win.iter()", Some(371), Some(371))];
    let anchors: FxHashSet<u32> = [371].into_iter().collect();
    let out = pass_normalize_identical_mods(&h, res, rs, &anchors);
    assert_eq!(out[0].kind(), RowKind::Mod, "block anchor must stay Mod");
}

#[test]
fn test_collapse_two_to_one_emits_anchor_mod_and_aux_del() {
    let rs = lang_profile("rs");
    let h = hunk(vec![
        Row::Del(DelLine::new(10, "fn render(&self) -> String {")),
        Row::Del(DelLine::new(11, "    return self.name.clone(); }")),
        Row::Add(AddLine::new(
            21,
            "fn render(&self) -> String { return self.name.clone(); }",
        )),
    ]);
    let out = collapse_hunk_rows(&h, None, None, rs);
    assert_eq!(
        out.len(),
        2,
        "Expected exactly 2 rows (anchor Mod + AuxDel), got: {out:?}"
    );
    match &out[0] {
        Row::Mod {
            old_line,
            new_line,
            is_anchor,
            ..
        } => {
            assert_eq!(old_line.num, 10);
            assert_eq!(old_line.text, "fn render(&self) -> String {");
            assert_eq!(new_line.num, 21);
            assert_eq!(
                new_line.text,
                "fn render(&self) -> String { return self.name.clone(); }"
            );
            assert!(*is_anchor, "Expected is_anchor to be true");
        }
        _ => panic!("Expected Row::Mod at index 0, got {:?}", out[0]),
    }
    match &out[1] {
        Row::AuxDel { anchor, line, .. } => {
            assert_eq!(
                *anchor, 21,
                "Expected AuxDel anchor to point to new line 21"
            );
            assert_eq!(line.num, 11);
            assert_eq!(line.text, "    return self.name.clone(); }");
        }
        _ => panic!("Expected Row::AuxDel at index 1, got {:?}", out[1]),
    }
}

#[test]
fn test_lcs_mod_hunk_15_stepwise() {
    let diff = r#"diff --git a/src/lcs/mod.rs b/crates/diffuse-core/src/lcs/mod.rs
--- a/src/lcs/mod.rs
+++ b/crates/diffuse-core/src/lcs/mod.rs
@@ -541,18 +836,42 @@
         matched_chars as f64 / char_len(new_text) as f64
     } else {
         0.0
     };
-    if !has_anchors && ratio < 0.25 {
-        return WordSpans {
+    let res = if !has_anchors && ratio < 0.25 {
+        WordSpans {
             del: vec![(0, char_len(old_text))],
             add: vec![(0, char_len(new_text))],
-        };
-    }
-    WordSpans {
-        del: merge_whitespace_gaps(old_text, spans_from(old_text, &ot, &matched_old)),
-        add: merge_whitespace_gaps(new_text, spans_from(new_text, &nt, &matched_new)),
-    }
+        }
+    } else {
+        WordSpans {
+            del: merge_whitespace_gaps(old_text, spans_from(old_text, &ot, &matched_old)),
+            add: merge_whitespace_gaps(new_text, spans_from(new_text, &nt, &matched_new)),
+        }
+    };
+
+    crate::ensure!(
+        res.del
+            .iter()
+            .all(|&(start, end)| start <= end && end <= char_len(old_text)),
+        "WordSpans del spans must fall within old_text bounds"
+    );
+    crate::ensure!(
+        res.add
+            .iter()
+            .all(|&(start, end)| start <= end && end <= char_len(new_text)),
+        "WordSpans add spans must fall within new_text bounds"
+    );
+    crate::ensure!(
+        res.del.windows(2).all(|w| w[0].1 <= w[1].0),
+        "WordSpans del spans must be non-overlapping and sorted"
+    );
+    crate::ensure!(
+        res.add.windows(2).all(|w| w[0].1 <= w[1].0),
+        "WordSpans add spans must be non-overlapping and sorted"
+    );
+
+    res
 }
"#;
    let files = crate::parse::parse_diff(diff);
    assert_eq!(files.len(), 1);
    let hunk = &files[0].hunks[0];
    let profile = lang_profile("rs");
    let c = collapse_hunk_rows(hunk, None, None, profile);
    let mut prev_old = None;
    for (i, r) in c.iter().enumerate() {
        if let Some(ol) = r.old_line() {
            if let Some(prev) = prev_old {
                assert_eq!(
                    ol,
                    prev + 1,
                    "old_line non-stepwise at row {i}: expected {}, got {}",
                    prev + 1,
                    ol
                );
            }
            prev_old = Some(ol);
        }
    }
}

#[test]
fn test_interior_donor_transitive_cooccurrence_and_deficit_conservation() {
    use super::assemble::matches_interior_donor;

    // 1. Motivating example: variable inlining (Transitive Co-occurrence)
    // Old: let scrollbar_seek = is_on_scrollbar;
    // Old M: if scrollbar_seek {
    // New M: if is_on_scrollbar {
    let old_m = "                if scrollbar_seek {";
    let new_m = "                if is_on_scrollbar {";
    let del_spans = vec![(19, 33)]; // "scrollbar_seek"
    let add_spans = vec![(19, 34)]; // "is_on_scrollbar"

    let donor_del = "                let scrollbar_seek = is_on_scrollbar;";
    assert!(
        matches_interior_donor(old_m, new_m, &del_spans, &add_spans, donor_del),
        "deleted variable declaration must qualify as an interior donor via transitive co-occurrence"
    );

    // 2. High-entropy deficit conservation (Predicate B)
    // Old: render(&opts);
    // New: render(&MarkdownOptions::default());
    // Del: ..MarkdownOptions::default()
    let old_m2 = "    render(&opts);";
    let new_m2 = "    render(&MarkdownOptions::default());";
    let del_spans2 = vec![(12, 16)]; // "opts"
    let add_spans2 = vec![(12, 38)]; // "MarkdownOptions::default()"
    let donor_del2 = "    ..MarkdownOptions::default()";
    assert!(
        matches_interior_donor(old_m2, new_m2, &del_spans2, &add_spans2, donor_del2),
        "high-entropy expression in candidate deletion must qualify via deficit conservation"
    );

    // 3. Rejection of unrelated deletion sharing no tokens
    let unrelated_del = "                let max_scroll = total - vh;";
    assert!(
        !matches_interior_donor(old_m, new_m, &del_spans, &add_spans, unrelated_del),
        "unrelated deletion must be rejected"
    );

    // 4. Rejection of short / low-entropy variable names (e.g. single-letter 'a' ->
    //    'b')
    let old_m3 = "    if a {";
    let new_m3 = "    if b {";
    let del_spans3 = vec![(7, 8)];
    let add_spans3 = vec![(7, 8)];
    let short_donor = "    let a = b;";
    assert!(
        !matches_interior_donor(old_m3, new_m3, &del_spans3, &add_spans3, short_donor),
        "short trivial identifiers below entropy floor must not form mod groups"
    );

    // 5. Rejection of candidate in different scope (mismatched indentation)
    let outer_scope_donor = "let scrollbar_seek = is_on_scrollbar;"; // 0 spaces vs 16 spaces
    assert!(
        !matches_interior_donor(old_m, new_m, &del_spans, &add_spans, outer_scope_donor),
        "candidate in distinct indentation scope must be rejected"
    );

    // 6. Substring within larger identifier must NOT trigger (word-boundary
    //    enforcement)
    let embedded_donor = "                let not_scrollbar_seek_here = is_on_scrollbar_flag;";
    assert!(
        !matches_interior_donor(old_m, new_m, &del_spans, &add_spans, embedded_donor),
        "embedded substrings without word boundary must be rejected"
    );
}
