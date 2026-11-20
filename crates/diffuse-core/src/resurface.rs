//! Whitespace-suppressed change surfacing.
//!
//! The raw diff is generated with whitespace-insensitive line matching
//! (`git diff -w`): a line pair that differs only in whitespace is *matched*
//! by git — its blocks stay coherently aligned, so the duplicate-verbatim
//! tangle class (a reindented block anchored onto a similar neighboring
//! block) cannot arise — but the change itself is suppressed from the tape:
//! inside a hunk the pair prints as one context row (git prints the new
//! side's text), and away from any content change the pair is not taped at
//! all.
//!
//! The engine's Mov predicate declares the leading/trailing-whitespace
//! subset of those pairs "the same line" ([`str::trim`] equality) and
//! renders them as Mov rows. This pass closes the gap between the two
//! layers: given the parsed tape and both content snapshots it surfaces
//! every whitespace-suppressed pair as ordinary `Del`+`Add` rows so the
//! cluster DP re-pairs them exactly as it pairs any other change:
//!
//! - **In-hunk suppression** — a `Ctx` row whose snapshot texts differ is
//!   replaced by `Del(old) + Add(new)` at that tape position.
//! - **Untaped suppression** — matched regions between hunks (and files whose
//!   only changes are suppressed) are walked pairwise; differing pairs become
//!   synthesized hunks (a `Del` block + `Add` block per change run,
//!   ±`ENGINE_CONTEXT` context padding, runs within twice the band merged),
//!   matching a `-U4` emission's shape.
//!
//! The suppression predicate is *differs only in whitespace* — git's `-w`
//! equality (identical non-whitespace content), which is wider than the
//! Mov predicate: a pair differing by interior whitespace
//! (`foo (x);` → `foo(x);`) is suppressed by git and must surface here even
//! though the DP pairs it as a Mod rather than a Mov.
//!
//! Fail-safe by construction: the pass is a strict no-op on
//! default-generated tapes (byte-equal context, truly-equal gaps), requires
//! both content snapshots, skips any region whose pairing would contradict
//! the tape (differences beyond whitespace, mismatched row counts), and
//! never touches a row it cannot prove suppressed.

use crate::{
    git::ENGINE_CONTEXT,
    parse::{AddLine, CtxLine, DelLine, Hunk, Row},
};

/// Surfaces whitespace-suppressed changes in `hunks` (see the module docs).
///
/// Returns whether the tape changed. `old_lines`/`new_lines` are the
/// 1-based content snapshots (`old_lines[n - 1]` is line `n`); with either
/// side empty the pass is a no-op.
pub fn resurface_hunks(hunks: &mut Vec<Hunk>, old_lines: &[&str], new_lines: &[&str]) -> bool {
    if old_lines.is_empty() || new_lines.is_empty() {
        return false;
    }
    let mut changed = false;
    for h in hunks.iter_mut() {
        changed |= resurface_ctx_rows(h, old_lines, new_lines);
    }
    changed |= synthesize_gap_hunks(hunks, old_lines, new_lines);
    changed
}

fn line_at<'a>(lines: &[&'a str], pos: u32) -> Option<&'a str> {
    lines.get((pos as usize).checked_sub(1)?).copied()
}

/// Whether two texts differ only in whitespace — git's `-w` suppression
/// equality: the non-whitespace content is identical. Wider than the
/// engine's Mov predicate (`str::trim`), so interior-whitespace changes
/// (`foo (x);` → `foo(x);`) surface too; the DP pairs them as Mods.
fn ws_only_diff(a: &str, b: &str) -> bool {
    a != b
        && a.chars()
            .filter(|c| !c.is_whitespace())
            .eq(b.chars().filter(|c| !c.is_whitespace()))
}

/// In-hunk suppression: a `Ctx` row proven to be a whitespace-suppressed
/// pair (snapshot texts differ only in whitespace, and the taped text
/// matches one side) is replaced by `Del(old) + Add(new)` at the same
/// position.
fn resurface_ctx_rows(h: &mut Hunk, old_lines: &[&str], new_lines: &[&str]) -> bool {
    let mut out = Vec::with_capacity(h.rows.len());
    let mut changed = false;
    for r in &h.rows {
        if let Row::Ctx(c) = r
            && let (Some(ot), Some(nt)) =
                (line_at(old_lines, c.old_pos), line_at(new_lines, c.new_pos))
            && ws_only_diff(ot, nt)
            && (*c.text == *ot || *c.text == *nt)
        {
            out.push(Row::del(DelLine::new(c.old_pos, ot)));
            out.push(Row::add(AddLine::new(c.new_pos, nt)));
            changed = true;
        } else {
            out.push(r.clone());
        }
    }
    if changed {
        h.rows = out;
    }
    changed
}

/// Untaped suppression: matched regions between (and around) the hunks are
/// walked pairwise; whitespace-suppressed pairs become synthesized hunks in
/// the shape of a `-U{ENGINE_CONTEXT}` emission.
fn synthesize_gap_hunks(hunks: &mut Vec<Hunk>, old_lines: &[&str], new_lines: &[&str]) -> bool {
    // Hunk coverage is header-derived (half-open old/new ranges). Tapes
    // carrying row kinds the pass does not model, out-of-bounds headers, or
    // non-disjoint hunks disable synthesis wholesale — the pass never
    // invents structure it cannot place.
    let valid = hunks.iter().all(|h| {
        h.rows
            .iter()
            .all(|r| matches!(r, Row::Ctx(_) | Row::Del { .. } | Row::Add { .. }))
            && h.old_start + h.old_count <= old_lines.len() as u32 + 1
            && h.new_start + h.new_count <= new_lines.len() as u32 + 1
    }) && hunks.windows(2).all(|w| {
        w[0].old_start + w[0].old_count <= w[1].old_start
            && w[0].new_start + w[0].new_count <= w[1].new_start
    });
    if !valid {
        return false;
    }

    let mut synthesized: Vec<Hunk> = Vec::new();
    let mut cursor_old = 1;
    let mut cursor_new = 1;
    for h in hunks.iter() {
        if let Some(hs) = synthesize_gap(
            cursor_old,
            h.old_start,
            cursor_new,
            h.new_start,
            old_lines,
            new_lines,
        ) {
            synthesized.extend(hs);
        }
        cursor_old = h.old_start + h.old_count;
        cursor_new = h.new_start + h.new_count;
    }
    if let Some(hs) = synthesize_gap(
        cursor_old,
        old_lines.len() as u32 + 1,
        cursor_new,
        new_lines.len() as u32 + 1,
        old_lines,
        new_lines,
    ) {
        synthesized.extend(hs);
    }

    if synthesized.is_empty() {
        return false;
    }
    // Both sides are positionally ordered and disjoint; parsed hunks win
    // ties (a pure-add hunk occupies its old-side insertion point).
    let mut merged: Vec<Hunk> = Vec::with_capacity(hunks.len() + synthesized.len());
    let mut i = 0;
    let mut j = 0;
    while i < hunks.len() && j < synthesized.len() {
        if hunks[i].old_start <= synthesized[j].old_start {
            merged.push(hunks[i].clone());
            i += 1;
        } else {
            merged.push(synthesized[j].clone());
            j += 1;
        }
    }
    merged.extend(hunks[i..].iter().cloned());
    merged.extend(synthesized[j..].iter().cloned());
    *hunks = merged;
    true
}

/// Synthesizes hunks for one matched region: old lines `[old_a, old_b)`
/// paired positionally with new lines `[new_a, new_b)` (exclusive bounds).
///
/// `None` marks a region that cannot be a product of whitespace-insensitive
/// matching (unpaired lengths, or a differing pair beyond whitespace); the
/// whole region is then left untouched.
fn synthesize_gap(
    old_a: u32,
    old_b: u32,
    new_a: u32,
    new_b: u32,
    old_lines: &[&str],
    new_lines: &[&str],
) -> Option<Vec<Hunk>> {
    if old_b < old_a || new_b < new_a {
        return None;
    }
    let count = old_b - old_a;
    if count != new_b - new_a {
        return None;
    }
    if count == 0 {
        return Some(Vec::new());
    }

    let mut differing: Vec<u32> = Vec::new();
    for k in 0..count {
        let ot = old_lines[(old_a + k - 1) as usize];
        let nt = new_lines[(new_a + k - 1) as usize];
        if ot != nt {
            if !ws_only_diff(ot, nt) {
                return None;
            }
            differing.push(k);
        }
    }
    if differing.is_empty() {
        return Some(Vec::new());
    }

    // Group runs a `-U{ENGINE_CONTEXT}` emission would merge: their windows
    // touch when the equal pairs between them span at most twice the band.
    let band = 2 * ENGINE_CONTEXT as u32 + 1;
    let mut groups: Vec<(u32, u32)> = Vec::new(); // [k0, k1)
    for &k in &differing {
        match groups.last_mut() {
            Some((_, k1)) if k - (*k1 - 1) <= band => *k1 = k + 1,
            _ => groups.push((k, k + 1)),
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for &(k0, k1) in &groups {
        let lead = k0.min(ENGINE_CONTEXT as u32);
        let trail = (count - k1).min(ENGINE_CONTEXT as u32);
        let is_diff = |k: u32| differing.binary_search(&k).is_ok();
        let mut rows: Vec<Row> = Vec::new();
        let mut run: Vec<u32> = Vec::new();
        for k in (k0 - lead)..(k1 + trail) {
            if k >= k0 && k < k1 && is_diff(k) {
                run.push(k);
            } else {
                flush_run(&mut run, &mut rows, old_a, new_a, old_lines, new_lines);
                rows.push(Row::Ctx(CtxLine::new(
                    old_a + k,
                    new_a + k,
                    old_lines[(old_a + k - 1) as usize],
                )));
            }
        }
        flush_run(&mut run, &mut rows, old_a, new_a, old_lines, new_lines);
        let start = k0 - lead;
        let extent = (k1 + trail) - start;
        out.push(Hunk {
            old_start: old_a + start,
            old_count: extent,
            new_start: new_a + start,
            new_count: extent,
            section: String::new(),
            rows,
            above_ctx: 0,
            below_ctx: 0,
        });
    }
    Some(out)
}

/// Emits one change run as a `Del` block followed by an `Add` block — the
/// replace-region shape every downstream stage expects — then clears the
/// accumulated run.
fn flush_run(
    run: &mut Vec<u32>,
    rows: &mut Vec<Row>,
    old_a: u32,
    new_a: u32,
    old_lines: &[&str],
    new_lines: &[&str],
) {
    for &k in run.iter() {
        rows.push(Row::del(DelLine::new(
            old_a + k,
            old_lines[(old_a + k - 1) as usize],
        )));
    }
    for &k in run.iter() {
        rows.push(Row::add(AddLine::new(
            new_a + k,
            new_lines[(new_a + k - 1) as usize],
        )));
    }
    run.clear();
}

#[cfg(test)]
mod tests;
