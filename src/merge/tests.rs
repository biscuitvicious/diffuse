//! Unit tests for token stream merging and multi-line subword projection.

use crate::{
    lcs::{char_slice, word_spans},
    merge::*,
    parse::{AddLine, DelLine, LineNum, Row},
};

fn mk(del: &str, add: &str) -> Row {
    Row::Mod {
        old_line: DelLine::new(LineNum::new(1).unwrap(), del),
        new_line: AddLine::with_spans(LineNum::new(1).unwrap(), add, word_spans(del, add)),
    }
}

// The split-word wholesale: a removal landing mid-word is garble — the
// whole-old + whole-new pair must win.
#[test]
fn split_word_wholesale() {
    let r = mk(
        "    let s = prefix_middle_suffix + other_operations();",
        "    let s = prefix_suffix + other_operations();",
    );
    assert!(
        is_wholesale_mod(&r),
        "a removal landing mid-word goes wholesale (prefix_~~middle_~~suffix)"
    );
    assert_eq!(
        display_text(&r),
        r.del_text().unwrap().to_string() + r.text(),
        "wholesale displayText is the whole-old + whole-new pair"
    );
}

// The trace wobble: the closing quote is a delimiter, the artifact filter
// is positional, and a del space yields to its next token's match.
#[test]
fn trace_wobble() {
    let r = mk(
        "                    trace!(host_path = ?next_lower, \"Found in lower layer\");",
        "                    trace!(host_path = ?next_lower, \"Found in layer below the top upper\");",
    );
    assert_eq!(
        display_text(&r),
        "                    trace!(host_path = ?next_lower, \"Found in lower layer below the top upper\");",
        "the wobble merges without the layer\"layer garble"
    );
    let runs = merged_runs(&r);
    let non_common: Vec<(&str, &str)> = runs
        .iter()
        .filter(|x| x.kind != RunKind::Common)
        .map(|x| {
            (
                match x.kind {
                    RunKind::Del => "del",
                    RunKind::Add => "add",
                    RunKind::Common => "common",
                    RunKind::Ctx => "ctx",
                },
                x.text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        non_common,
        vec![("del", "lower "), ("add", " below the top upper")],
        "the del lower strikes once, the rest fills"
    );
}

// The 1-to-1 mount.rs mod merge: shared prefix common, added .get(...) fills,
// no embedded newlines.
#[test]
fn mount_1_to_1_mod_merge() {
    let del = "        self.mount_stack";
    let add = "        self.mount_stack.get(guest_path)";
    let r = mk(del, add);
    assert!(!is_wholesale_mod(&r), "the mount mod stays the subword mod");
    let runs = merged_runs(&r);
    assert_eq!(runs[0].kind, RunKind::Common);
    assert_eq!(
        runs[0].text, "        self.mount_stack",
        "the shared prefix reads common"
    );
    assert_eq!(runs[1].kind, RunKind::Add);
    assert_eq!(runs[1].text, ".get(guest_path)", "the fill reads added");
    assert_eq!(runs.len(), 2, "no internal newlines in any run");
}

// The whitespace minimum: 8 → 12 spaces = 8 common + 4 added.
#[test]
fn whitespace_minimum() {
    let del = "        self.cache.borrow_mut().pop(key);";
    let add = "            self.cache.borrow_mut().pop(key);";
    let r = mk(del, add);
    let runs = merged_runs(&r);
    let parts: Vec<(&str, String)> = runs
        .iter()
        .map(|x| {
            (
                match x.kind {
                    RunKind::Common => "common",
                    RunKind::Del => "del",
                    RunKind::Add => "add",
                    RunKind::Ctx => "ctx",
                },
                x.text.clone(),
            )
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            ("common", "        ".to_string()),
            ("add", "    ".to_string()),
            ("common", "self.cache.borrow_mut().pop(key);".to_string()),
        ],
        "8 common + 4 added, never the whole run on each side"
    );
}

#[test]
fn resolved_layers_to_layers_subword() {
    let del = "                    resolved_layers: unresolved,";
    let add = "                    layers: unresolved,";
    let spans = word_spans(del, add);
    let wholesale = wholesale_pair(del, add, &spans);
    assert!(
        !wholesale,
        "resolved_layers -> layers should be subword inline mod, not wholesale"
    );
}

// A pure-deletion mod: every add span is whitespace — the row is a removal.
#[test]
fn pure_deletion_mod() {
    let r = mk("pub(super) struct X {", "pub struct X {");
    assert!(
        is_pure_deletion_mod(&r),
        "a removal with no fill is a pure deletion"
    );
    let filled = mk("pub struct X {", "pub(super) struct X {");
    assert!(
        !is_pure_deletion_mod(&filled),
        "an insertion with no strike is not a pure deletion"
    );
}

// char_slice must round-trip the spans (char offsets, non-ASCII safe).
#[test]
fn char_slice_roundtrip() {
    // The add spans index the ADD text (char offsets, non-ASCII safe —
    // the é is two UTF-8 bytes but one char).
    let add = "fn café(x: u64) -> u64 {";
    let ws = word_spans("fn café(x: u32) -> u32 {", add);
    assert_eq!(char_slice(add, ws.add[0].0, ws.add[0].1), "u64");
    assert_eq!(char_slice(add, ws.add[1].0, ws.add[1].1), "u64");
    // Multi-byte 1-char span: the é ends the slice at its full UTF-8
    // length, never a non-boundary byte.
    assert_eq!(char_slice("é", 0, 1), "é");
}

// char_slice is total — the JS `String.slice` clamps and never panics.
// An empty del/add line yields empty spans (0,0), and the wholesale
// check slices them: this panicked on the real diffuse-repo diff
// ("end byte index 1 is out of bounds for string of length 0").
#[test]
fn char_slice_empty_and_out_of_range() {
    assert_eq!(char_slice("", 0, 0), "", "empty span on empty text");
    assert_eq!(char_slice("", 0, 1), "", "start past the end clamps");
    assert_eq!(char_slice("abc", 0, 0), "");
    assert_eq!(char_slice("abc", 1, 1), "");
    assert_eq!(char_slice("abc", 3, 3), "", "span at the end");
    assert_eq!(char_slice("abc", 0, 5), "abc", "end past the end clamps");
    assert_eq!(char_slice("abc", 4, 5), "", "start past the end");
}

// An empty DEL line is a valid row: the whole-line gate yields the empty
// del span (0,0), and the wholesale check must not panic on it (the
// diffuse-repo regression).
#[test]
fn wholesale_pair_empty_del_line() {
    let spans = word_spans("", "        Some(content)");
    assert_eq!(spans.del, vec![(0, 0)]);
    assert!(
        !wholesale_pair("", "        Some(content)", &spans),
        "an empty del is not wholesale garble — no panic"
    );
}

// Unicode whitespace subwords merge (the TS block: a nbsp vs a tab, a
// zero-width space vs a space, an ideographic space vs 3 spaces): the
// whitespace change yields del + add subwords and the identifier survives
// as common.
#[test]
fn unicode_whitespace_merged_runs() {
    let cases: [(&str, &str); 3] = [
        ("\u{00a0}self.x", "\tself.x"),
        ("\u{200b}self.x", " self.x"),
        ("\u{3000}self.x", "   self.x"),
    ];
    for (del, add) in cases {
        let r = Row::Mov {
            old_line: DelLine::new(LineNum::new(1).unwrap(), del),
            new_line: AddLine::with_spans(LineNum::new(1).unwrap(), add, word_spans(del, add)),
        };
        let runs = merged_runs(&r);
        let kinds: Vec<RunKind> = runs.iter().map(|x| x.kind).collect();
        assert!(
            kinds.contains(&RunKind::Del) && kinds.contains(&RunKind::Add),
            "unicode whitespace subwords: {del:?} vs {add:?}: {kinds:?}"
        );
        assert!(
            runs.iter()
                .any(|x| x.kind == RunKind::Common && x.text.contains("self")),
            "the identifier survives as common"
        );
    }
}

#[test]
fn test_bidirectional_run_projection() {
    let cases = [
        (
            "        self.mount_stack",
            "        self.mount_stack.get(guest_path)",
        ),
        (
            "        self.cache.borrow_mut().pop(key);",
            "            self.cache.borrow_mut().pop(key);",
        ),
        (
            "    let s = prefix_middle_suffix + other();",
            "    let s = prefix_suffix + other();",
        ),
        (
            "        self.resolved_layers = resolved;",
            "        self.layers = layers;",
        ),
    ];

    for (del, add) in cases {
        let r = mk(del, add);
        let runs = merged_runs(&r);
        assert_eq!(
            project_forward(&runs),
            add,
            "project_forward(runs) must equal new text"
        );
        if !is_wholesale_mod(&r) {
            assert_eq!(
                project_reverse(&runs),
                del,
                "project_reverse(runs) must equal old text"
            );
        }
    }
}

#[test]
fn test_del_placement_before_replacement_add() {
    let r = mk(
        "    pub fn lookup(&self) -> Option<ResolutionResult> {",
        "    pub fn epoch(&self) -> u64 {",
    );
    let runs = merged_runs(&r);

    // Verify del("Option<ResolutionResult>") appears BEFORE add("u64")
    let opt_del_idx = runs
        .iter()
        .position(|run| run.kind == RunKind::Del && run.text.contains("Option<ResolutionResult>"))
        .expect("Option<ResolutionResult> deletion run exists");
    let u64_add_idx = runs
        .iter()
        .position(|run| run.kind == RunKind::Add && run.text.contains("u64"))
        .expect("u64 addition run exists");

    assert!(
        opt_del_idx < u64_add_idx,
        "del('Option<ResolutionResult>') at {} must precede add('u64') at {}",
        opt_del_idx,
        u64_add_idx
    );
}

#[test]
fn test_unpaired_del_position_before_add_at_same_offset() {
    let del_text = "    pub fn epoch(&self) -> Option<ResolutionResult> {";
    let add_text = "    pub fn epoch(&self) -> u64 {";
    let del_spans = vec![(27, 51)]; // Option<ResolutionResult>
    let add_spans = vec![(27, 30)]; // u64
    let pos = del_display_positions(&del_spans, &add_spans, del_text, add_text);
    assert_eq!(
        pos[0], 27,
        "unpaired deletion at offset 27 must be placed before add at offset 27"
    );
}

#[test]
fn test_del_position_with_filtered_intermediate_spans() {
    let del_text = "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {";
    let add_text = "    pub fn epoch(&self) -> u64 {";
    let del_spans = vec![(11, 17), (44, 68)]; // lookup, Option<ResolutionResult>
    let add_spans = vec![(11, 16), (27, 30)]; // epoch, u64
    let pos = del_display_positions(&del_spans, &add_spans, del_text, add_text);
    assert_eq!(
        pos,
        vec![11, 27],
        "Option<ResolutionResult> at offset 44 in old text must map to offset 27 (start of u64) in new text"
    );
}
