use super::*;
use crate::{
    cluster::dp::Weights,
    merge::RunKind,
    syntax::registry::lang_profile,
    tape::{AlignmentTape, TapeChunk},
};

#[test]
fn test_block_reflow_involution() {
    let mut tape = AlignmentTape::default();
    tape.push(TapeChunk::Keep("let result = compute("));
    tape.push(TapeChunk::Strike("alpha, beta);"));
    tape.push(TapeChunk::Fill("\n    alpha,\n    beta);"));

    let expansion = BlockReflow::new_expansion(10, 20, 3, 0, tape);
    assert!(expansion.is_expansion());
    assert_eq!(expansion.k(), 3);

    let collapse = expansion.inverse();
    assert!(collapse.is_collapse());
    assert_eq!(collapse.k(), 3);
    assert_eq!(collapse.old_range, 20..23);
    assert_eq!(collapse.new_range, 10..11);
    assert_eq!(collapse.anchor_old, 0);
    assert_eq!(collapse.anchor_new, 0);

    // Exact involution property: (R^-1)^-1 == R
    assert_eq!(collapse.inverse(), expansion);
}

#[test]
fn test_block_reflow_tape_reversibility() {
    let old = "    let x = foo(a, b);";
    let new_cont = ["    let x = foo(", "        a, b);"];
    let rs = lang_profile("rs");

    let reflow = build_expansion_reflow(old, &new_cont, 0, rs);
    let inv = reflow.inverse();

    assert_eq!(inv.inverse(), reflow);
    assert_eq!(inv.tape.project_old(), reflow.tape.project_new());
    assert_eq!(inv.tape.project_new(), reflow.tape.project_old());
}

#[test]
fn test_block_reflow_symmetric_indent_penalty() {
    let w = Weights::default();
    let score_a_to_b = BlockReflow::score(4, 8, 40, 50, 35, 2, 0, &w);
    let score_b_to_a = BlockReflow::score(8, 4, 50, 40, 35, 2, 0, &w);

    // Indentation penalty deviation |4 - 8| == |8 - 4|, yielding equal penalty
    let pen_a = w.gamma * 4.0 * w.indent_penalty_factor;
    let pen_b = w.gamma * 4.0 * w.indent_penalty_factor;
    assert_eq!(pen_a, pen_b);
    assert_eq!(score_a_to_b, score_b_to_a);
}

#[test]
fn test_subline_relocation_tracks_relocated_tokens() {
    let old = "    transform(alpha, beta);";
    let new_lines = ["    transform(", "        alpha, beta);"];

    let reflow = build_expansion_reflow(old, &new_lines, 0, None);
    let relocations: Vec<&TapeChunk> = reflow
        .tape
        .chunks()
        .iter()
        .filter(|c| matches!(c, TapeChunk::Relocate { .. }))
        .collect();

    assert!(
        !relocations.is_empty(),
        "Continuation tokens transferred across lines must be tracked as Relocate"
    );

    let runs = reflow.tape.to_merged_runs();
    assert!(
        runs.iter().any(|r| r.kind == RunKind::Mov),
        "Relocated subline tokens must produce RunKind::Mov in merged runs"
    );
}

#[test]
fn test_collapse_reflow_and_inverse_involution() {
    let old_lines = ["    transform(", "        alpha, beta);"];
    let new = "    transform(alpha, beta);";

    let reflow = build_collapse_reflow(&old_lines, new, 0, None);
    assert!(reflow.is_collapse());
    assert_eq!(reflow.old_range, 0..2);
    assert_eq!(reflow.new_range, 0..1);

    let inv = reflow.inverse();
    assert!(inv.is_expansion());
    assert_eq!(inv.inverse(), reflow);
}
