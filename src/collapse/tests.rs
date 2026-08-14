//! Ports of the collapse tests from test/review.test.ts: the similarity
//! pairing + the declaration section-stop, the `None` ↔ `Some(result)`
//! brace-anchor, the reflow pairing, the reverse stretch.

use crate::collapse::collapse_hunk_rows;
use crate::moves::mark_moves;
use crate::parse::{AddLine, CtxLine, DelLine, DiffFile, FileStatus, Hunk, LineNum, Row, RowKind};

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
        RowKind::Sep | RowKind::File => Row::Sep(text.to_string()),
    }
}

fn moved_row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    let old_num = old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
    let new_num = new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
    match kind {
        RowKind::Del => Row::Del(DelLine::new(old_num, text)),
        _ => Row::Mov {
            old_line: DelLine::new(old_num, text),
            new_line: AddLine::new(new_num, text),
        },
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
        status: FileStatus::Modified,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
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
    let c = collapse_hunk_rows(&h, false, None, None);
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
    let mut old_content: Vec<String> = Vec::new();
    old_content.resize(130, String::new());
    old_content[113] = "    }".to_string(); // the fn's closing brace after the del
    let mut new_content: Vec<String> = Vec::new();
    new_content.resize(320, String::new());
    new_content[155] = "    }".to_string(); // the fn's closing brace after Some(result)
    new_content[299] = "        None".to_string(); // a verbatim None elsewhere
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
    let c = collapse_hunk_rows(&h, false, Some(&old_content), Some(&new_content));
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
    let c = collapse_hunk_rows(&h, false, None, None);
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
    assert!(
        spans.add.is_empty(),
        "the reflowed .iter().any(…) is common"
    );
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
    let c2 = collapse_hunk_rows(&h2, false, None, None);
    assert_eq!(c2.len(), 1, "one mod");
    let m2 = &c2[0];
    let spans2 = m2.spans().expect("spans");
    assert_eq!(spans2.del, vec![(22, 31)], "the resolved_ strike");
    assert!(
        spans2.add.is_empty(),
        "the reflowed .iter().find(…)?; reads common"
    );
}

// The modified-pair collapse: the mod is a 1-to-1 row; its section's additions
// and closing blank render as standalone add rows; moved lines reflow; the add
// after the blank is a separate row.
#[test]
fn modified_pair_absorbs_section_and_reflow() {
    let mut f = file(vec![hunk(vec![
        row(RowKind::Del, "old one", None, Some(1)),
        row(RowKind::Add, "new one", Some(1), None),
        row(RowKind::Add, "extra line", Some(2), None),
        row(RowKind::Add, "", Some(3), None), // section break
        row(RowKind::Del, "moved line", None, Some(4)),
        row(RowKind::Add, "moved line", Some(4), None),
        row(RowKind::Add, "after the blank", Some(5), None),
    ])]);
    mark_moves(&mut f);
    let c = collapse_hunk_rows(&f.hunks[0], false, None, None);
    assert_eq!(c.len(), 4, "collapsed to 4 separate single-line rows");
    assert_eq!(c[0].kind(), RowKind::Mod, "mod row leads");
    assert_eq!(
        c[1].kind(),
        RowKind::Add,
        "extra line is a separate add row"
    );
    assert_eq!(c[1].new_line(), Some(2));
    assert_eq!(
        c[2].kind(),
        RowKind::Add,
        "blank line is a separate add row"
    );
    assert_eq!(c[2].new_line(), Some(3));
    assert_eq!(
        c[3].kind(),
        RowKind::Add,
        "the add after the blank is a separate row"
    );
    assert_eq!(c[3].new_line(), Some(5));
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
    let c = collapse_hunk_rows(&h2, false, None, None);
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
    let c = collapse_hunk_rows(&h, false, None, None);
    println!("COLLAPSED FRAGMENT: {:#?}", c);
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Del && r.old_line() == Some(1)),
        "the split old line stays a removal"
    );
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Mod).count(),
        0,
        "no fragment mod (whole old vs one new fragment)"
    );
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Add).count(),
        7,
        "all seven split lines survive"
    );
    assert_eq!(c[0].kind(), RowKind::Del, "removal leads the block");
    assert_eq!(c[1].kind(), RowKind::Add);
    assert_eq!(c[1].new_line(), Some(1));
}

// Wholesale pairs render as the -/+ baseline rows: a WORD-led removal
// landing mid-word is garble — the collapse un-pairs it into the plain
// `-` + `+` rows. Meaningful identity = subword still pairs (the
// fragment-count mismatches included).
#[test]
fn wholesale_pairs_render_as_baseline_rows() {
    // The split-word garble: `prefix_~~middle_~~suffix` — must NOT pair.
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
    let c = collapse_hunk_rows(&h, false, None, None);
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Del && r.old_line() == Some(1)),
        "the old line stays a removal"
    );
    assert!(
        c.iter()
            .any(|r| r.kind() == RowKind::Add && r.new_line() == Some(1)),
        "the new line stays an addition"
    );
    assert_eq!(
        c.iter().filter(|r| r.kind() == RowKind::Mod).count(),
        0,
        "no mod row"
    );
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
        collapse_hunk_rows(&g, false, None, None)
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
        collapse_hunk_rows(&h2, false, None, None)
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
        collapse_hunk_rows(&i, false, None, None)
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

    let c = collapse_hunk_rows(&h3, false, None, None);
    let mod3 = c
        .iter()
        .find(|r| r.kind() == RowKind::Mod)
        .expect("the mod");
    assert_eq!(
        mod3.new_line(),
        Some(153),
        "pairs old None with return None;"
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
    let c = collapse_hunk_rows(&h, true, None, None);
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
    let t = std::time::Instant::now();
    let c = collapse_hunk_rows(&h, true, None, None);
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
    let c = collapse_hunk_rows(&h, true, None, None);
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
    let c = collapse_hunk_rows(&h, false, None, None);
    assert_eq!(c.len(), 3);
    assert_eq!(c[0].kind(), RowKind::Mod);
    assert_eq!(c[0].new_line(), Some(1082));
    assert_eq!(c[0].text(), "                if let Some(floor) = floor");
    let spans1 = c[0].spans().expect("spans1");
    assert_eq!(
        spans1.del,
        vec![(28, 30)],
        "trailing space and brace ' {{' are struck on line 1082"
    );

    assert_eq!(c[1].kind(), RowKind::Add);
    assert_eq!(c[1].new_line(), Some(1083));
    assert_eq!(c[1].text(), "                    && k > floor");
    let spans2 = c[1].spans().expect("spans2");
    assert_eq!(
        spans2.del,
        vec![(21, 22)],
        "< is struck on line 1083 preceding >"
    );
    assert_eq!(
        spans2.add,
        vec![(0, 23), (25, 26)],
        "add2 highlights full leading indentation prefix && and >"
    );

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
    let c = collapse_hunk_rows(&h2, false, None, None);
    assert_eq!(c[0].kind(), RowKind::Mod, "mod leads");
    assert_eq!(c[0].new_line(), Some(142));
    assert_eq!(c[1].kind(), RowKind::Add, "then the moves");
    assert_eq!(c[1].new_line(), Some(143));
    assert!(c[1].is_moved());
    assert_eq!(c[2].kind(), RowKind::Add, "then the moves");
    assert_eq!(c[2].new_line(), Some(144));
    assert!(c[2].is_moved());
    assert_eq!(
        c[3].kind(),
        RowKind::Mod,
        "the return pairs with return None;"
    );
    assert_eq!(c[3].new_line(), Some(145));
    assert!(
        c[3].del_text().unwrap().contains("return self.results"),
        "the kept del anchors the second mod"
    );
}

// Multi-line deletion into a 1-to-1 mod row: the first line pairs into a single-line
// mod row, verbatim reflows drop, and the remaining deleted lines render as separate
// `-` lines (no internal newlines / multi-line folded rows).
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
    let mut old_content: Vec<String> = Vec::new();
    old_content.resize(1030, String::new());
    old_content[1023] = "    }".to_string();
    let mut new_content: Vec<String> = Vec::new();
    new_content.resize(20, String::new());
    new_content[8] =
        "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {".to_string();
    new_content[9] = "        self.filesystems".to_string();
    new_content[10] = "            .iter()".to_string();
    new_content[11] =
        "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))".to_string();
    new_content[12] = "            .map(|fs| fs.as_ref() as &dyn Filesystem)".to_string();
    new_content[13] = "    }".to_string();
    let c = collapse_hunk_rows(&h, false, Some(&old_content), Some(&new_content));
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
    let mut old_content: Vec<String> = vec![String::new(); 800];
    let mut new_content: Vec<String> = vec![String::new(); 1000];
    old_content[739] = "fn resolve_layer_stack() {".to_string();
    old_content[746] = "            .find(|l| l.is_upper())?;".to_string();
    old_content[770] = "fn dir_exists_at(&self, sub_path: &Path) -> bool {".to_string();
    new_content[739] = "fn resolve_layer_stack() {".to_string();
    new_content[899] = "fn dir_exists_at(&self, sub_path: &Path) -> bool {".to_string();
    new_content[911] =
        "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())".to_string();

    let collapsed = collapse_hunk_rows(&h, false, Some(&old_content), Some(&new_content));
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
    let collapsed = collapse_hunk_rows(&h, false, None, None);
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
    let mut old_content = vec![String::new(); 300];
    let mut new_content = vec![String::new(); 350];
    old_content[200] = "fn to_os_string(&self) {".to_string();
    new_content[280] = "fn into_filesystem(self) {".to_string();

    let collapsed = collapse_hunk_rows(&h, false, Some(&old_content), Some(&new_content));
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

    let collapsed = collapse_hunk_rows(&h, false, None, None);
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
    let collapsed = collapse_hunk_rows(&h, false, None, None);
    println!("COLLAPSED: {:#?}", collapsed);
    assert_eq!(collapsed.len(), 1, "Should collapse to 1 Mod row");
    assert_eq!(collapsed[0].kind(), RowKind::Mod);
}
