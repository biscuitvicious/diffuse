//! Tier 2 residual evidence matching engine.
//!
//! Evaluates unpaired deletions and additions after Tier 1 DP pairing
//! and move detection have run. Allows calibrated offset drift when
//! supported by declaration role agreement, structural import skeletons,
//! or high whole-line character identity, under a strict zero-rival invariant.

use rustc_hash::FxHashSet;

use diffuse_syntax::profile::LangProfile;

use crate::{
    collapse::PairingResults,
    parse::{Hunk, Row},
};

/// Maximum offset drift allowed between unpaired lines in Tier 2.
pub const MAX_TIER2_OFFSET: usize = 5;

/// Minimum non-whitespace character similarity ratio for Tier 2 fallback.
pub const MIN_TIER2_CHAR_RATIO: f64 = 0.50;

/// Identifies the declaration role of a code line, stripping common visibility
/// prefixes.
pub(crate) fn declaration_role(line: &str) -> Option<&'static str> {
    let mut words = line.split_whitespace();
    let mut first = words.next()?;
    if matches!(
        first,
        "pub"
            | "pub(crate)"
            | "pub(super)"
            | "export"
            | "declare"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "async"
    ) {
        first = words.next()?;
    }
    match first {
        "struct" => Some("struct"),
        "enum" => Some("enum"),
        "union" => Some("union"),
        "trait" => Some("trait"),
        "interface" => Some("interface"),
        "type" => Some("type"),
        "class" => Some("class"),
        "fn" | "def" | "function" => Some("fn"),
        "const" | "static" => Some("const"),
        _ => None,
    }
}

/// Checks whether two declaration roles are semantically compatible for
/// refactoring.
pub(crate) fn are_roles_compatible(role1: &str, role2: &str) -> bool {
    if role1 == role2 {
        return true;
    }
    let is_type_decl = |r: &str| {
        matches!(
            r,
            "struct" | "enum" | "union" | "trait" | "interface" | "type" | "class"
        )
    };
    if is_type_decl(role1) && is_type_decl(role2) {
        return true;
    }
    let is_var_decl = |r: &str| matches!(r, "const" | "static");
    if is_var_decl(role1) && is_var_decl(role2) {
        return true;
    }
    false
}

/// Whether the line is an import / dependency declaration.
pub(crate) fn is_import_statement(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("use ")
        || t.starts_with("import ")
        || t.starts_with("require(")
        || t.starts_with("from ")
        || t.starts_with("include ")
}

/// Computes non-whitespace character multiset similarity between two lines.
pub(crate) fn non_ws_char_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.chars().filter(|c| !c.is_whitespace()).collect();
    let b_chars: Vec<char> = b.chars().filter(|c| !c.is_whitespace()).collect();
    if a_chars.is_empty() || b_chars.is_empty() {
        return 0.0;
    }
    let mut b_pool = b_chars.clone();
    let mut common = 0;
    for ca in &a_chars {
        if let Some(pos) = b_pool.iter().position(|cb| cb == ca) {
            common += 1;
            b_pool.swap_remove(pos);
        }
    }
    (2.0 * common as f64) / (a_chars.len() + b_chars.len()) as f64
}

/// Checks if a trimmed line text appears verbatim anywhere in the snapshot
/// content.
fn has_verbatim_rival(text: &str, content: Option<&[&str]>) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    content.is_some_and(|lines| lines.iter().any(|l| l.trim() == trimmed))
}

/// Executes the Tier 2 residual pairing pass over unpaired deletions and
/// additions.
pub fn pass_residual_pairing(
    h: &Hunk,
    p1: &mut PairingResults,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    _profile: Option<&LangProfile>,
) {
    let paired_dels: FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let in_collapse_blocks: FxHashSet<usize> = p1
        .collapse_blocks
        .values()
        .flat_map(|v| v.iter().copied())
        .collect();
    let in_expand_blocks: FxHashSet<usize> = p1
        .expand_blocks
        .values()
        .flat_map(|v| v.iter().copied())
        .collect();

    // Collect unpaired deletion indices.
    let mut unpaired_dels = Vec::new();
    for (ri, r) in h.rows.iter().enumerate() {
        if let Row::Del(dl) = r
            && !paired_dels.contains(&ri)
            && !in_collapse_blocks.contains(&ri)
            && !r.is_moved()
            && !dl.text.trim().is_empty()
        {
            unpaired_dels.push(ri);
        }
    }

    // Collect unpaired addition indices.
    let mut unpaired_adds = Vec::new();
    for (ri, r) in h.rows.iter().enumerate() {
        if let Row::Add(al) = r
            && !p1.pair_of.contains_key(&ri)
            && !in_expand_blocks.contains(&ri)
            && !r.is_moved()
            && !al.text.trim().is_empty()
        {
            unpaired_adds.push(ri);
        }
    }

    if unpaired_dels.is_empty()
        || unpaired_adds.is_empty()
        || unpaired_dels.len() * unpaired_adds.len() > crate::cluster::dp::cluster_budget()
    {
        return;
    }

    let mut matched_dels = FxHashSet::default();
    let mut matched_adds = FxHashSet::default();

    for &ai in &unpaired_adds {
        if matched_adds.contains(&ai) {
            continue;
        }
        let al = match &h.rows[ai] {
            Row::Add(a) => a,
            _ => continue,
        };
        let add_text = &al.text;

        // Zero-rival check for addition: cannot have a twin in old content (candidate
        // move).
        if has_verbatim_rival(add_text, old_content) {
            continue;
        }

        let add_indent = add_text.len() - add_text.trim_start().len();
        let add_role = declaration_role(add_text);
        let add_is_import = is_import_statement(add_text);

        let mut best_del = None;
        let mut best_score = -1.0;

        for &di in &unpaired_dels {
            if matched_dels.contains(&di) {
                continue;
            }
            let dl = match &h.rows[di] {
                Row::Del(d) => d,
                _ => continue,
            };
            let del_text = &dl.text;

            // Zero-rival check for deletion: cannot have a twin in new content (candidate
            // move).
            if has_verbatim_rival(del_text, new_content) {
                continue;
            }

            let del_indent = del_text.len() - del_text.trim_start().len();
            if del_indent != add_indent {
                continue;
            }

            // Bounding anchor / context support: cannot cross context lines.
            let (min_i, max_i) = if di < ai { (di, ai) } else { (ai, di) };
            if h.rows[min_i..=max_i]
                .iter()
                .any(|r| matches!(r, Row::Ctx(_)))
            {
                continue;
            }

            // Monotonicity constraint: cannot cross existing pairings.
            let crosses_existing = p1
                .pair_of
                .iter()
                .any(|(&a, &d)| (a < ai && d > di) || (a > ai && d < di));
            if crosses_existing {
                continue;
            }

            let offset_diff = (dl.num as isize - al.num as isize).unsigned_abs();
            if offset_diff > MAX_TIER2_OFFSET {
                continue;
            }

            let del_role = declaration_role(del_text);
            let del_is_import = is_import_statement(del_text);

            let evidence_match = match (del_role, add_role) {
                (Some(dr), Some(ar)) if are_roles_compatible(dr, ar) => {
                    // Check Dyck delimiter compatibility (e.g. both open a body with '{' or both
                    // terminate with ';')
                    let d_ends_open = del_text.trim_end().ends_with('{');
                    let a_ends_open = add_text.trim_end().ends_with('{');
                    d_ends_open == a_ends_open
                }
                _ => false,
            };

            let import_match = del_is_import && add_is_import;

            let char_ratio = non_ws_char_similarity(del_text, add_text);
            let high_char_match = char_ratio >= MIN_TIER2_CHAR_RATIO;

            if evidence_match || import_match || high_char_match {
                // Score preference: exact role match > import > char ratio, penalized by offset
                // drift.
                let base_score = if evidence_match {
                    100.0
                } else if import_match {
                    80.0
                } else {
                    char_ratio * 50.0
                };
                let score = base_score - (offset_diff as f64 * 5.0);
                if score > best_score {
                    best_score = score;
                    best_del = Some(di);
                }
            }
        }

        if let Some(di) = best_del {
            matched_adds.insert(ai);
            matched_dels.insert(di);
            p1.pair_of.insert(ai, di);
        }
    }
}
