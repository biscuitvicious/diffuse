//! Coincidental context splitting.
//!
//! Unified-diff aligners (xdiff) emit closing delimiters (`}`, `break;`,
//! blank lines) as context rows by position even when the match is
//! coincidental rather than a true anchor. Every downstream stage treats
//! `Row::Ctx` as an impenetrable cluster boundary, so one coincidental row
//! fractures a single logical change block into single-sided clusters the
//! DP can never reunite. This pass rewrites trivial Ctx rows to
//! Del(old)+Add(new) in place, before move detection, so flanking clusters
//! merge and the DP finds the global optimum.

use diffuse_syntax::profile::LangProfile;

use crate::parse::{AddLine, DelLine, Row};

/// Whether a Ctx pair is coincidental trivia that must not anchor a block
/// boundary: split unless both sides carry salient tokens.
///
/// This is the salient-evidence half of the pair-eligibility gate with
/// deliberately no identity fast-path (contrast `is_admissible_mod_pair`):
/// an identical `}`-vs-`}` pair carries no anchoring evidence and must
/// split. Blank lines never anchor. A Ctx row's two sides are the same
/// line by construction, so shared context is automatic — only the
/// presence of salient tokens on both sides keeps the row.
pub fn should_split_ctx(old_text: &str, new_text: &str, profile: Option<&LangProfile>) -> bool {
    if old_text.trim().is_empty() || new_text.trim().is_empty() {
        return true;
    }
    let old_salient = crate::lcs::line_has_salient_token(old_text, profile);
    let new_salient = crate::lcs::line_has_salient_token(new_text, profile);
    !(old_salient && new_salient)
}

/// Rewrite trivial Ctx rows to Del+Add in place, preserving order.
///
/// The Del(old) immediately followed by Add(new) preserves positional
/// monotonicity, so flanking clusters merge with no explicit cluster
/// surgery; partially split runs leave residual Ctx barriers in place.
/// Line numbers are untouched, so total line coverage is unchanged.
///
/// Returns the rewritten rows plus the split home pairs: each entry is
/// the `(del_row_idx, add_row_idx)` of one split Ctx in the *output*
/// indexing. The pairing stage uses them to restore no-gain splits
/// (both halves unpaired or identically re-paired) to git's alignment:
/// an identical reshuffle carries no evidence, so the original anchor
/// stands unless a half found a strictly better (non-identical) partner.
pub fn split_trivial_ctx_rows(
    rows: Vec<Row>,
    profile: Option<&LangProfile>,
) -> (Vec<Row>, Vec<(usize, usize)>) {
    let mut out = Vec::with_capacity(rows.len());
    let mut homes = Vec::new();
    for r in rows {
        match r {
            Row::Ctx(c) if should_split_ctx(&c.text, &c.text, profile) => {
                let di = out.len();
                out.push(Row::del(DelLine::new(c.old_pos, c.text.clone())));
                out.push(Row::add(AddLine::new(c.new_pos, c.text)));
                homes.push((di, di + 1));
            }
            other => out.push(other),
        }
    }
    (out, homes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffuse_syntax::registry::lang_profile;

    use crate::parse::{AddLine, CtxLine, DelLine};

    #[test]
    fn test_lone_brace_ctx_splits() {
        let profile = lang_profile("rs");
        assert!(should_split_ctx(
            "                                }",
            "                                }",
            profile
        ));
    }

    #[test]
    fn test_blank_ctx_splits() {
        let profile = lang_profile("rs");
        assert!(should_split_ctx("", "", profile));
        assert!(should_split_ctx("   ", "   ", profile));
    }

    #[test]
    fn test_break_ctx_splits() {
        let profile = lang_profile("rs");
        assert!(should_split_ctx(
            "                                        break;",
            "                                break;",
            profile
        ));
    }

    #[test]
    fn test_salient_ctx_survives() {
        let profile = lang_profile("rs");
        assert!(!should_split_ctx(
            "                                let prev_row = self.row_idx;",
            "                                let prev_row = self.row_idx;",
            profile
        ));
    }

    #[test]
    fn test_split_rewrite_merges_flanking_clusters() {
        let profile = lang_profile("rs");
        let rows = vec![
            Row::del(DelLine::new(312, "                            }")),
            Row::Ctx(CtxLine::new(307, 307, "                                }")),
            Row::add(AddLine::new(308, "                                }")),
        ];
        let (out, homes) = split_trivial_ctx_rows(rows, profile);
        assert!(
            out.iter().all(|r| !matches!(r, Row::Ctx(_))),
            "trivial Ctx must be rewritten, got: {out:?}"
        );
        assert_eq!(out.len(), 4);
        assert!(matches!(&out[1], Row::Del { line: d, .. } if d.num == 307));
        assert!(matches!(&out[2], Row::Add { line: a, .. } if a.num == 307));
        assert_eq!(homes, vec![(1, 2)]);
    }

    #[test]
    fn test_hunk_edge_split_attaches_to_single_cluster() {
        let profile = lang_profile("rs");
        let rows = vec![
            Row::Ctx(CtxLine::new(307, 307, "                                }")),
            Row::add(AddLine::new(308, "                                }")),
        ];
        let (out, homes) = split_trivial_ctx_rows(rows, profile);
        assert_eq!(out.len(), 3);
        assert!(matches!(&out[0], Row::Del { line: d, .. } if d.num == 307));
        assert!(matches!(&out[1], Row::Add { line: a, .. } if a.num == 307));
        assert_eq!(homes, vec![(0, 1)]);
    }

    #[test]
    fn test_salient_ctx_row_preserved_in_place() {
        let profile = lang_profile("rs");
        let rows = vec![
            Row::del(DelLine::new(
                353,
                "                            let prev_row = self.row_idx;",
            )),
            Row::Ctx(CtxLine::new(
                309,
                345,
                "                                        let seg_off = target_vis - s;",
            )),
            Row::add(AddLine::new(
                349,
                "                                let prev_row = self.row_idx;",
            )),
        ];
        let (out, homes) = split_trivial_ctx_rows(rows, profile);
        assert_eq!(out.len(), 3);
        assert!(matches!(out[1], Row::Ctx(_)));
        assert!(homes.is_empty());
    }

    /// Golden-driven: scrollbar-grab-drag dispatch.rs — old 353–367 must
    /// not strand as a Del run once the coincidental `}` barriers split.
    #[test]
    fn test_dispatch_blank_line_context_preserved() {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let base = manifest.join("../../tests/golden/scrollbar-grab-drag");
        let Ok(patch) = std::fs::read_to_string(base.join("fixture").join("diff.patch")) else {
            return;
        };
        let old = std::fs::read_to_string(
            base.join("fixture")
                .join("old")
                .join("crates/diffuse-tui/src/app/dispatch.rs"),
        )
        .expect("old snapshot must exist");
        let new = std::fs::read_to_string(
            base.join("fixture")
                .join("new")
                .join("crates/diffuse-tui/src/app/dispatch.rs"),
        )
        .expect("new snapshot must exist");

        let files = crate::parse::parse_diff(&patch).unwrap();
        let f = files
            .into_iter()
            .find(|f| f.path.contains("dispatch.rs"))
            .expect("dispatch.rs must parse");
        let out = crate::pipeline::process_single_diff_file(
            f,
            Some(new.as_str()),
            Some(old.as_str()),
            None,
            &crate::pipeline::PipelineOptions::default(),
        );

        let rows: Vec<_> = out
            .hunks
            .iter()
            .flat_map(|h| h.rows.iter())
            .filter(|r| r.old_line == Some(262) || r.new_line == Some(257))
            .collect();

        assert_eq!(
            rows.len(),
            1,
            "should be exactly one row for line 262/257, got: {rows:?}"
        );
        assert_eq!(
            rows[0].kind,
            crate::parse::RowKind::Ctx,
            "blank line must be Ctx"
        );
        assert_eq!(rows[0].old_line, Some(262));
        assert_eq!(rows[0].new_line, Some(257));
    }

    #[test]
    fn test_dispatch_reindent_block_pairs() {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let base = manifest.join("../../tests/golden/scrollbar-grab-drag");
        let Ok(patch) = std::fs::read_to_string(base.join("fixture").join("diff.patch")) else {
            return;
        };
        let old = std::fs::read_to_string(
            base.join("fixture")
                .join("old")
                .join("crates/diffuse-tui/src/app/dispatch.rs"),
        )
        .expect("old snapshot must exist");
        let new = std::fs::read_to_string(
            base.join("fixture")
                .join("new")
                .join("crates/diffuse-tui/src/app/dispatch.rs"),
        )
        .expect("new snapshot must exist");

        let files = crate::parse::parse_diff(&patch).unwrap();
        let f = files
            .into_iter()
            .find(|f| f.path.contains("dispatch.rs"))
            .expect("dispatch.rs must parse");
        let out = crate::pipeline::process_single_diff_file(
            f,
            Some(new.as_str()),
            Some(old.as_str()),
            None,
            &crate::pipeline::PipelineOptions::default(),
        );

        let dels_in_region: Vec<u32> = out
            .hunks
            .iter()
            .flat_map(|h| h.rows.iter())
            .filter(|r| {
                matches!(r.kind, crate::parse::RowKind::Del)
                    && r.old_line.is_some_and(|n| (353..=367).contains(&n))
            })
            .filter_map(|r| r.old_line)
            .collect();
        assert!(
            dels_in_region.is_empty(),
            "reindent block stranded as Del: {dels_in_region:?}"
        );
    }
}
