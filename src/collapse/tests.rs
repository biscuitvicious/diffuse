//! Ports of the collapse tests from test/review.test.ts: the similarity
//! pairing + the declaration section-stop, the `None` ↔ `Some(result)`
//! brace-anchor, the reflow pairing, the reverse stretch.

use crate::collapse::collapse_hunk_rows;
use crate::parse::{Hunk, Row, RowKind};

fn row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    Row::new(kind, text, new_line, old_line)
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
        .position(|r| r.kind == RowKind::Mod)
        .expect("lookup pairs with lookup");
    assert!(
        c[mod_idx].text.contains("epoch: u64"),
        "the mod's add is the new lookup"
    );
    assert!(
        c[0].kind == RowKind::Add && c[0].new_line == Some(134),
        "adds lead the block"
    );
    assert!(mod_idx > 0, "mod sits after its preceding additions");
    assert!(
        c[0].tail.is_empty(),
        "no doc tail — the epoch docs are separate adds"
    );
    assert!(
        c.iter()
            .any(|r| r.kind == RowKind::Add && r.text.contains("pub fn epoch")),
        "the epoch method is its own section"
    );
    assert_eq!(
        c.iter()
            .filter(|r| r.kind == RowKind::Add && r.text.contains("///"))
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
        .find(|r| r.kind == RowKind::Mod && r.del_text.as_deref() == Some("        None"))
        .expect("the None mod");
    assert_eq!(
        none_mod.new_line,
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
    assert_eq!(m.kind, RowKind::Mod);
    assert_eq!(
        m.del_text.as_deref(),
        Some("        self.resolved_layers"),
        "the first del pairs"
    );
    let spans = m.spans.as_ref().expect("spans");
    assert_eq!(spans.del, vec![(13, 22)], "the resolved_ strike only");
    assert!(
        spans.add.is_empty(),
        "the reflowed .iter().any(…) is common"
    );
}

// The reverse stretch: a del RUN folds into one mod against a single add —
// the mod's delText is the run's concatenation, the dels are consumed.
#[test]
fn reverse_stretch_fold() {
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
    let c = collapse_hunk_rows(&h, false, None, Some(&new_content));
    let rmod = c
        .iter()
        .find(|r| r.kind == RowKind::Mod)
        .expect("the reverse stretch mod");
    assert_eq!(
        rmod.del_text.as_ref().unwrap().split('\n').count(),
        5,
        "the old body folds into one mod with all 5 lines"
    );
    assert_eq!(rmod.new_line, Some(1060));
    assert!(rmod.del_text.as_ref().unwrap().contains(".rfind"));
    assert_eq!(
        c.iter().filter(|r| r.kind == RowKind::Del).count(),
        0,
        "the run's dels are consumed by the mod (no floating - rows)"
    );
}
