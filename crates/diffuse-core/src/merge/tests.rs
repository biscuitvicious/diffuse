//! Unit tests for token stream merging and multi-line subword projection.

use crate::{
    lcs::{char_slice, word_spans, word_spans_with_profile},
    merge::*,
    parse::{AddLine, DelLine, Row},
    tape::AlignmentTape,
};
use diffuse_syntax::registry::lang_profile;

fn mk(del: &str, add: &str) -> Row {
    Row::mod_with_spans(
        DelLine::new(1, del),
        AddLine::new(1, add),
        word_spans(del, add),
    )
}

// Subword inline mod: an identifier modification (prefix_middle_suffix ->
// prefix_suffix) renders cleanly inline with common context and struck middle_,
// not wholesale garble.
#[test]
fn split_word_subword() {
    let r = mk(
        "    let s = prefix_middle_suffix + other_operations();",
        "    let s = prefix_suffix + other_operations();",
    );
    assert!(
        !is_wholesale_mod(&r),
        "identifier modification must render as inline subword diff, not wholesale"
    );
    let runs = merged_runs(&r);
    assert_eq!(
        project_forward(&runs),
        "    let s = prefix_suffix + other_operations();"
    );
    assert_eq!(
        project_reverse(&runs),
        "    let s = prefix_middle_suffix + other_operations();"
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
        .filter(|x| x.kind != RunKind::Ctx)
        .map(|x| {
            (
                match x.kind {
                    RunKind::Del | RunKind::MovedSrc => "del",
                    RunKind::Add => "add",
                    RunKind::Ctx | RunKind::MovedTgt => "ctx",
                },
                x.text,
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
    assert_eq!(runs[0].kind, RunKind::Ctx);
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
    let parts: Vec<(&str, &str)> = runs
        .iter()
        .map(|x| {
            (
                match x.kind {
                    RunKind::Del | RunKind::MovedSrc => "del",
                    RunKind::Add => "add",
                    RunKind::Ctx | RunKind::MovedTgt => "ctx",
                },
                x.text,
            )
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            ("add", "    "),
            ("ctx", "        self.cache.borrow_mut().pop(key);"),
        ],
        "4 added at col 0 + 8 common, never the whole run on each side"
    );
}

#[test]
fn resolved_layers_to_layers_subword() {
    let del = "                    resolved_layers: unresolved,";
    let add = "                    layers: unresolved,";
    let spans = word_spans(del, add);
    let wholesale = is_wholesale_replacement_spans(del, add, &spans);
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
    assert_eq!(char_slice(add, ws.add[0].0, ws.add[0].1), "64");
    assert_eq!(char_slice(add, ws.add[1].0, ws.add[1].1), "64");
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
        is_wholesale_replacement_spans("", "        Some(content)", &spans),
        "an empty del line replaced with content is wholesale replacement — no panic"
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
        let r = Row::mod_with_spans(
            DelLine::new(1, del),
            AddLine::new(1, add),
            word_spans(del, add),
        );
        let runs = merged_runs(&r);
        let kinds: Vec<RunKind> = runs.iter().map(|x| x.kind).collect();
        assert!(
            kinds.contains(&RunKind::Del) && kinds.contains(&RunKind::Add),
            "unicode whitespace subwords: {del:?} vs {add:?}: {kinds:?}"
        );
        assert!(
            runs.iter()
                .any(|x| x.kind == RunKind::Ctx && x.text.contains("self")),
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
    let spans = word_spans(del_text, add_text);
    let runs = merged_runs_from_texts(del_text, add_text, &spans);
    assert_eq!(project_reverse(&runs), del_text);
    assert_eq!(project_forward(&runs), add_text);
}

#[test]
fn test_del_position_with_filtered_intermediate_spans() {
    let del_text = "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {";
    let add_text = "    pub fn epoch(&self) -> u64 {";
    let spans = word_spans(del_text, add_text);
    let runs = merged_runs_from_texts(del_text, add_text, &spans);
    assert_eq!(project_reverse(&runs), del_text);
    assert_eq!(project_forward(&runs), add_text);
}

#[test]
fn test_trailing_deletion_placed_at_end_of_add_text() {
    let del_text = "                if k < floor {";
    let add_text = "                if let Some(floor) = floor";
    let spans = word_spans(del_text, add_text);
    let runs = merged_runs_from_texts(del_text, add_text, &spans);
    assert_eq!(project_reverse(&runs), del_text);
    assert_eq!(project_forward(&runs), add_text);
}

#[test]
fn test_mod_row_subline_transformation_invariants() {
    let cases = vec![
        ("    let x = 1;", "    let x = 2;"),
        (
            "pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {",
            "pub fn epoch(&self) -> u64 {",
        ),
        (
            "                if k < floor {",
            "                if let Some(floor) = floor",
        ),
        (
            "    let s = prefix_middle_suffix + other_operations();",
            "    let s = prefix_suffix + other_operations();",
        ),
        (
            "trace!(host_path = ?next_lower, \"Found in lower layer\");",
            "trace!(host_path = ?next_lower, \"Found in layer below the top upper\");",
        ),
    ];

    for (old, new) in cases {
        let r = mk(old, new);
        let runs = merged_runs(&r);
        assert_eq!(
            project_new_runs(&runs),
            new,
            "project_new(runs) must equal new text"
        );
        // Note: For non-wholesale mods, del spans reflect the deleted portions
        // and common spans reflect preserved portions.
    }
}

#[test]
fn test_doc_comment_subword_merge() {
    let old = "    /// Parse a diff and emit structured token-stream JSON or JSON Lines.";
    let new = "    /// Parse a diff and emit structured token-stream JSON, JSON Lines, or";
    let r = mk(old, new);
    let runs = merged_runs(&r);
    assert_eq!(project_new_runs(&runs), new);
}

#[test]
fn leading_indent_shift_pair_projects_exactly() {
    // A 12→20 space indent shift with a mid-line rewrite: the leading-ws
    // alignment in merged_runs_from_texts must keep both projections
    // disk-exact — stripping del runs leaves the new line, stripping add
    // runs leaves the old line.
    let del_text = "            last.text.push_str(&run.text);";
    let add_text = "                    last.text.len() + run.text.len(),";
    let spans = word_spans_with_profile(del_text, add_text, lang_profile("rs"));
    let runs = merged_runs_from_texts(del_text, add_text, &spans);
    assert_eq!(
        project_forward(&runs),
        add_text,
        "strike-removed runs must reconstruct the new line"
    );
    assert_eq!(
        project_reverse(&runs),
        del_text,
        "add-removed runs must reconstruct the old line"
    );
}

#[test]
fn test_leading_indent_shift_with_prefix_removal() {
    let del_text = "                        } else if let Some(p) = profile {";
    let add_text = "                            if let Some(p) = profile {";
    let spans = word_spans_with_profile(del_text, add_text, lang_profile("rs"));
    let runs = merged_runs_from_texts(del_text, add_text, &spans);

    assert_eq!(
        project_forward(&runs),
        add_text,
        "strike-removed runs must reconstruct the new line"
    );
    assert_eq!(
        project_reverse(&runs),
        del_text,
        "add-removed runs must reconstruct the old line"
    );

    assert_eq!(runs[0].kind, RunKind::Add);
    assert_eq!(runs[0].text, "    ");
    assert_eq!(runs[1].kind, RunKind::Ctx);
    assert_eq!(runs[1].text, "                        ");
    assert_eq!(runs[2].kind, RunKind::Del);
    assert_eq!(runs[2].text, "} else ");
}

#[test]
fn test_leading_indent_unindent_with_prefix_removal() {
    let del_text = "                            } else if let Some(p) = profile {";
    let add_text = "                        if let Some(p) = profile {";
    let spans = word_spans_with_profile(del_text, add_text, lang_profile("rs"));
    let runs = merged_runs_from_texts(del_text, add_text, &spans);

    assert_eq!(
        project_forward(&runs),
        add_text,
        "strike-removed runs must reconstruct the new line"
    );
    assert_eq!(
        project_reverse(&runs),
        del_text,
        "add-removed runs must reconstruct the old line"
    );

    assert_eq!(runs[0].kind, RunKind::Del);
    assert_eq!(runs[0].text, "    ");
    assert_eq!(runs[1].kind, RunKind::Ctx);
    assert_eq!(runs[1].text, "                        ");
    assert_eq!(runs[2].kind, RunKind::Del);
    assert_eq!(runs[2].text, "} else ");
}

#[test]
fn test_leading_indent_same_with_prefix_removal() {
    let del_text = "                        } else if let Some(p) = profile {";
    let add_text = "                        if let Some(p) = profile {";
    let spans = word_spans_with_profile(del_text, add_text, lang_profile("rs"));
    let runs = merged_runs_from_texts(del_text, add_text, &spans);

    assert_eq!(
        project_forward(&runs),
        add_text,
        "strike-removed runs must reconstruct the new line"
    );
    assert_eq!(
        project_reverse(&runs),
        del_text,
        "add-removed runs must reconstruct the old line"
    );

    assert_eq!(runs[0].kind, RunKind::Ctx);
    assert_eq!(runs[0].text, "                        ");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "} else ");
}

#[test]
fn trailing_generic_pair_projects_exactly() {
    // The `coalesce_runs` generics row (merge/mod.rs:545 in the
    // diffuse-history-mid corpus): two `>` del fragments pair with the
    // `<'a>`-style inserts; the merged display must project exactly in both
    // directions.
    let del_text = "pub fn coalesce_runs(runs: Vec<Run>) -> Vec<Run> {";
    let add_text = "pub fn coalesce_runs<'a>(runs: Vec<Run<'a>>) -> Vec<Run<'a>> {";
    let spans = Spans {
        del: vec![(34, 35), (47, 48)],
        add: vec![(20, 24), (38, 43), (55, 60)],
    };
    let runs = merged_runs_from_texts(del_text, add_text, &spans);
    assert_eq!(
        project_forward(&runs),
        add_text,
        "strike-removed runs must reconstruct the new line"
    );
    assert_eq!(
        project_reverse(&runs),
        del_text,
        "add-removed runs must reconstruct the old line"
    );
}

#[test]
fn test_del_span_does_not_jump_over_common_tokens_invertible() {
    let old = "                if !run.is_empty() && !moved_dels.contains(&run[0]) {";
    let new = "                    if run.len() > 1";
    let r = mk(old, new);
    let runs = merged_runs(&r);

    let old_proj = project_old_runs(&runs);
    assert_eq!(
        old_proj, old,
        "old run projection must perfectly reconstruct the old line"
    );

    let new_proj = project_new_runs(&runs);
    assert_eq!(
        new_proj, new,
        "new run projection must perfectly reconstruct the new line"
    );
}

#[test]
fn test_colon_space_reconstruction() {
    let old = "\t\"description\": \"Interactive git-diff review for pi: wrapped hunks with syntax highlighting, persistent margin notes, and hunk-level git staging.\",";
    let new = "\t\"description\": \"Interactive git-diff review for pi (extension): wrapped hunks with syntax highlighting, persistent margin notes, and hunk-level git staging.\",";
    let r = mk(old, new);
    let runs = merged_runs(&r);
    assert_eq!(project_reverse(&runs), old);
    assert_eq!(project_forward(&runs), new);
}

#[test]
fn test_pi_review_extension_reconstruction() {
    let old = "the pi review extension (TypeScript renderer + TUI), moved in at PLAN.md";
    let new = "the pi review extension. The extension is a **thin client**: `/review`";
    let r = mk(old, new);
    let runs = merged_runs(&r);
    assert_eq!(project_reverse(&runs), old);
    assert_eq!(project_forward(&runs), new);
}

#[test]
fn test_golden_txt_line_reconstruction() {
    let del = "3. **`<name>.golden.txt`** — the frozen *rendered* output (the TUI's";
    let add = "5. **`render.txt`** — the frozen *terminal rendered* output. It locks the hand-tuned visual preferences (strike/fill minimums, anchor pairings, move-block shape, reverse-stretch).";
    let r = mk(del, add);
    let runs = merged_runs(&r);
    assert_eq!(project_reverse(&runs), del);
    assert_eq!(project_forward(&runs), add);
}

#[test]
fn test_box_drawing_spacing_reconstruction() {
    let old = "│   ├── package.json        builds the extension bundle";
    let new =
        "│        ├── package.json   builds the extension bundle (native diffuse TUI wrapper)";
    let profile = diffuse_syntax::registry::lang_profile("md");
    let ws = crate::lcs::word_spans_with_profile(old, new, profile);
    let r = Row::mod_with_spans(DelLine::new(1, old), AddLine::new(1, new), ws);
    let runs = merged_runs(&r);
    assert_eq!(project_reverse(&runs), old);
    assert_eq!(project_forward(&runs), new);
}

#[test]
fn test_render_txt_line_96_reconstruction() {
    let old = "{48;5;236}{38;5;147}  96{38;5;117}≈{38;5;120}+{39}{23}{48;5;28}    {48;5;28}{48;5;28}{38;5;150}//{39}{48;5;28}{48;5;28}{38;5;150} The dels' row ids, in order — the TS iterates {39}{48;5;28}{48;5;28}{38;5;51}`{39}{48;5;28}{48;5;28}h{48;5;28}{48;5;28}{38;5;117}.{39}{48;5;28}{48;5;28}rows{48;5;28}{48;5;28}{38;5;51}`{39}{48;5;28}{48;5;28}{38;5;150}.{39}{48;5;28}{48;5;236}";
    let new = "{48;5;22}{38;5;151}  96 {38;5;120}+{39}{23}{48;5;22}    {48;5;22}{48;5;22}{38;5;150}//{39}{48;5;22}{48;5;22}{38;5;150} The dels' row ids, in order — the TS iterates {39}{48;5;22}{48;5;22}{38;5;51}`{39}{48;5;22}{48;5;22}h{48;5;22}{48;5;22}{38;5;117}.{39}{48;5;22}{48;5;22}rows{48;5;22}{48;5;22}{38;5;51}`{39}{48;5;22}{48;5;22}{38;5;150}.{39}{48;5;22}{48;5;22}";
    let spans = crate::lcs::word_spans(old, new);
    let runs = merged_runs_from_texts(old, new, &spans);
    let rev = project_reverse(&runs);
    assert_eq!(rev, old);
}

#[test]
fn test_is_wholesale_replacement_branches() {
    use crate::{
        merge::{is_wholesale_replacement, is_wholesale_replacement_spans},
        parse::{CtxLine, Spans},
    };

    // Non-mod row returns false
    let ctx = Row::Ctx(CtxLine::new(1, 1, "hello"));
    assert!(!is_wholesale_replacement(&ctx));

    // Both empty returns false
    assert!(!is_wholesale_replacement_spans("", "", &Spans::default()));

    // Partial mod row returns false
    let r_partial = Row::mod_with_spans(
        DelLine::new(1, "hello world"),
        AddLine::new(1, "hello rust"),
        Spans {
            del: vec![(6, 11)],
            add: vec![(6, 10)],
        },
    );
    assert!(!is_wholesale_replacement(&r_partial));

    // Common indent alone without non-whitespace context returns true (wholesale)
    let r_indent = Row::mod_with_spans(
        DelLine::new(1, "  foo"),
        AddLine::new(1, "  bar"),
        Spans {
            del: vec![(0, 5)],
            add: vec![(0, 5)],
        },
    );
    assert!(is_wholesale_replacement(&r_indent));

    // Common non-whitespace context returns false
    let r_non_ws = Row::mod_with_spans(
        DelLine::new(1, "  foo(a);"),
        AddLine::new(1, "  foo(b);"),
        Spans {
            del: vec![(6, 7)],
            add: vec![(6, 7)],
        },
    );
    assert!(!is_wholesale_replacement(&r_non_ws));

    // Completely struck out and added returns true
    let r_wholesale = Row::mod_with_spans(
        DelLine::new(1, "alpha"),
        AddLine::new(1, "beta"),
        Spans {
            del: vec![(0, 5)],
            add: vec![(0, 4)],
        },
    );
    assert!(is_wholesale_replacement(&r_wholesale));
}

// The render path historically rebuilt its own unprofiled tape while
// assemble used the profiled tape; the two coincide on this battery. Fails
// loudly with any newly divergent pair (a new divergence changes render
// output under unification — see the witness below).
#[test]
fn profiled_unprofiled_merged_runs_agree() {
    let profile = lang_profile("rs").expect("rs profile must exist");
    let pairs: &[(&str, &str)] = &[
        (
            "    fn alpha_field_0(x: i32) -> i32 { x + 1 }",
            "    fn beta_field_0(x: i32) -> i32 { x + 1 }",
        ),
        (
            "    let old_value_1 = compute(1, 2);",
            "    let new_value_1 = compute(1, 3);",
        ),
        (
            "    let s = prefix_middle_suffix + other_operations();",
            "    let s = prefix_suffix + other_operations();",
        ),
        (
            "trace!(host_path = ?next_lower, \"Found in lower layer\");",
            "trace!(host_path = ?next_lower, \"Found in layer below the top upper\");",
        ),
        (
            "if a == b && c != d { foo(); }",
            "if a == b || c != d { bar(); }",
        ),
        (
            "let x: Vec<String> = vec![\"a\", \"b\"];",
            "let x: Vec<String> = vec![\"a\", \"c\"];",
        ),
        (
            "// ünïcodé cömment with emoji 🎉",
            "// ünïcodé cömment with emoji 🎊",
        ),
        ("", "fn fresh() {}"),
        ("fn gone() {}", ""),
        ("identical line, no change", "identical line, no change"),
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
        ("foo(1, 2, 3)", "foo(1, 2, 3, 4)"),
        (
            "self.field.method(arg).chain()",
            "self.field.method(other).chain()",
        ),
    ];
    for (del, add) in pairs {
        let plain: Vec<(RunKind, &str)> = AlignmentTape::from_texts(del, add, None)
            .to_merged_runs()
            .iter()
            .map(|r| (r.kind, r.text))
            .collect();
        let prof: Vec<(RunKind, &str)> = AlignmentTape::from_texts(del, add, Some(profile))
            .to_merged_runs()
            .iter()
            .map(|r| (r.kind, r.text))
            .collect();
        assert_eq!(
            plain, prof,
            "profiled/unprofiled merged runs diverge for {del:?} -> {add:?}"
        );
    }
}

// Divergence witness (diffuse-history-mid golden): numbers inside a
// rewritten comment tail anchor differently under the rs profile — the
// unprofiled legacy tape keeps the wholesale tail while the profiled tape
// splits the numbers out. Under render unification the profiled form is the
// canonical presentation (assemble and render share one tape); the
// unprofiled computation survives only as the fallback for rows without
// cached runs. This witness pins both forms so any drift in either tape
// surfaces immediately.
#[test]
fn profiled_unprofiled_merged_runs_diverge_on_comment_tail_numbers() {
    use RunKind::{Add, Ctx, Del};

    let profile = lang_profile("rs").expect("rs profile must exist");
    let del = "    assert_eq!(f.hunks[0].rows.len(), 8); // lines 10..=17";
    let add = "    assert_eq!(f.hunks[0].rows.len(), 18); // lines 5..=22 (6 above + mod11 + ctx12..15 + mod16 + 6 below)";
    let runs = |p| {
        AlignmentTape::from_texts(del, add, p)
            .to_merged_runs()
            .iter()
            .map(|r| (r.kind, r.text))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        runs(None),
        vec![
            (Ctx, "    assert_eq!(f.hunks[0].rows.len(), "),
            (Del, "8"),
            (Add, "18"),
            (Ctx, "); // lines "),
            (Del, "10..=17"),
            (
                Add,
                "5..=22 (6 above + mod11 + ctx12..15 + mod16 + 6 below)",
            ),
        ],
        "unprofiled render tape keeps the wholesale comment tail"
    );
    assert_eq!(
        runs(Some(profile)),
        vec![
            (Ctx, "    assert_eq!(f.hunks[0].rows.len(), "),
            (Del, "8"),
            (Add, "18"),
            (Ctx, "); // lines "),
            (Del, "10"),
            (Add, "5"),
            (Ctx, "..="),
            (Del, "17"),
            (Add, "22 (6 above + mod11 + ctx12..15 + mod16 + 6 below)"),
        ],
        "profiled assemble tape splits the comment tail numbers out"
    );
}

#[test]
fn test_run_kind_sign_char_and_from_sign_char() {
    assert_eq!(RunKind::Ctx.sign_char(), ' ');
    assert_eq!(RunKind::Del.sign_char(), '-');
    assert_eq!(RunKind::Add.sign_char(), '+');

    assert_eq!(RunKind::from_sign_char(' '), Some(RunKind::Ctx));
    assert_eq!(RunKind::from_sign_char('_'), Some(RunKind::Ctx));
    assert_eq!(RunKind::from_sign_char('-'), Some(RunKind::Del));
    assert_eq!(RunKind::from_sign_char('+'), Some(RunKind::Add));
    assert_eq!(RunKind::from_sign_char('x'), None);
}

#[test]
fn test_runs_new_text_and_old_text() {
    let runs = [
        Run {
            kind: RunKind::Ctx,
            text: "let ",
        },
        Run {
            kind: RunKind::Del,
            text: "old_val",
        },
        Run {
            kind: RunKind::Add,
            text: "new_val",
        },
        Run {
            kind: RunKind::Ctx,
            text: " = 42;",
        },
    ];
    assert_eq!(new_text_from_runs(&runs), "let new_val = 42;");
    assert_eq!(old_text_from_runs(&runs), "let old_val = 42;");
}
