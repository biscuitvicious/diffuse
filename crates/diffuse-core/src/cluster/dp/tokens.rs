//! Tokenized line representations and token multiset helpers for DP cluster
//! matching.

use diffuse_syntax::profile::LangProfile;
use smallvec::SmallVec;

use crate::similarity::TokenInterner;

/// The per-line tokenized form the DP works on: interned token ids, char
/// lengths, and each content token's byte offset within the raw line (for
/// later subword-span mapping in Phase 4).
#[derive(Default)]
pub struct DpTokens {
    pub(crate) ids: SmallVec<[u32; 16]>,
    pub(crate) chars: usize,
    pub(crate) token_chars: SmallVec<[usize; 16]>,
    pub(crate) token_mask: [u64; 4],
    pub(crate) token_multiset: SmallVec<[(u32, u16); 16]>,
    pub(crate) salient_chars: usize,
    pub(crate) salient_token_chars: SmallVec<[usize; 16]>,
    pub(crate) salient_mask: [u64; 4],
    pub(crate) salient_multiset: SmallVec<[(u32, u16); 16]>,
    /// Salient occurrences that ARE their identifier's full extent on this
    /// line (e.g. `Comment` in `TokenColor::Comment`); `(id, count)` pairs
    /// sorted by id.
    pub(crate) salient_anchored_multiset: SmallVec<[(u32, u16); 16]>,
    /// Buried occurrences keyed by containing identifier: `(salient_id,
    /// container_id, count)` sorted by `(salient_id, container_id)`. Burial
    /// inside the SAME identifier on both lines (`Count` inside `CountKind`
    /// twice) is shared vocabulary and earns full credit; burial inside
    /// different identifiers (`scrollbar` in `is_dragging_scrollbar` vs
    /// `on_scrollbar_click`) earns the discount.
    pub(crate) salient_buried_containers: SmallVec<[(u32, u32, u16); 16]>,
}

/// Tokenizes line text into interned `DpTokens` for fast DP multiset scoring.
pub fn dp_tokens(
    interner: &mut TokenInterner,
    text: &str,
    profile: Option<&LangProfile>,
) -> DpTokens {
    let mut ids: SmallVec<[u32; 16]> = SmallVec::new();
    let mut token_chars: SmallVec<[usize; 16]> = SmallVec::new();
    let mut token_mask = [0u64; 4];
    let mut chars = 0;

    let mut salient_chars = 0;
    let mut salient_token_chars: SmallVec<[usize; 16]> = SmallVec::new();
    let mut salient_mask = [0u64; 4];
    let mut salient_anchored_ids: SmallVec<[u32; 16]> = SmallVec::new();
    let mut salient_buried_container_pairs: SmallVec<[(u32, u32); 16]> = SmallVec::new();

    // Token slices tile the whole line — every byte belongs to exactly one
    // token — so consumed length is the next token's start offset.
    let mut offset = 0usize;
    let continues_ident = |c: char| c.is_alphanumeric() || c == '_';

    crate::lcs::for_each_token(text, |t| {
        let start = offset;
        offset += t.len();
        if !t.trim().is_empty() {
            let id = interner.intern_with_profile(t, profile);
            ids.push(id);
            let tc = crate::lcs::char_len(t);
            token_chars.push(tc);
            chars += tc;
            if !interner.ws[id as usize] {
                let bit = (id as usize) % 256;
                token_mask[bit / 64] |= 1u64 << (bit % 64);
            }
            if interner.salient[id as usize] {
                salient_chars += tc;
                salient_token_chars.push(tc);
                let bit = (id as usize) % 256;
                salient_mask[bit / 64] |= 1u64 << (bit % 64);
                // Identifier-extent anchor: a salient token that IS the
                // maximal identifier run around it (Unicode alphanumerics
                // plus `_`, mirroring the tokenizer's word runs) is an
                // identifier of its own; one sharing that run with
                // characters on either side is buried inside a longer
                // identifier (snake/camel interior, stem, or suffix).
                let end = start + t.len();
                let mut ext_start = start;
                while let Some(c) = text[..ext_start].chars().next_back() {
                    if !continues_ident(c) {
                        break;
                    }
                    ext_start -= c.len_utf8();
                }
                let mut ext_end = end;
                while let Some(c) = text[ext_end..].chars().next() {
                    if !continues_ident(c) {
                        break;
                    }
                    ext_end += c.len_utf8();
                }
                if ext_start == start && ext_end == end {
                    salient_anchored_ids.push(id);
                } else {
                    salient_buried_container_pairs
                        .push((id, interner.intern(&text[ext_start..ext_end])));
                }
            } else {
                salient_token_chars.push(0);
            }
        }
    });
    let mut non_ws_ids: SmallVec<[u32; 16]> = ids
        .iter()
        .copied()
        .filter(|&id| !interner.ws[id as usize])
        .collect();
    non_ws_ids.sort_unstable();
    let mut token_multiset: SmallVec<[(u32, u16); 16]> = SmallVec::new();
    for &id in &non_ws_ids {
        if let Some(last) = token_multiset.last_mut()
            && last.0 == id
        {
            last.1 = last.1.saturating_add(1);
        } else {
            token_multiset.push((id, 1));
        }
    }

    let mut salient_ids: SmallVec<[u32; 16]> = ids
        .iter()
        .copied()
        .filter(|&id| interner.salient[id as usize])
        .collect();
    salient_ids.sort_unstable();
    let mut salient_multiset: SmallVec<[(u32, u16); 16]> = SmallVec::new();
    for &id in &salient_ids {
        if let Some(last) = salient_multiset.last_mut()
            && last.0 == id
        {
            last.1 = last.1.saturating_add(1);
        } else {
            salient_multiset.push((id, 1));
        }
    }

    let mut salient_anchored_multiset: SmallVec<[(u32, u16); 16]> = SmallVec::new();
    salient_anchored_ids.sort_unstable();
    for &id in &salient_anchored_ids {
        if let Some(last) = salient_anchored_multiset.last_mut()
            && last.0 == id
        {
            last.1 = last.1.saturating_add(1);
        } else {
            salient_anchored_multiset.push((id, 1));
        }
    }
    let mut salient_buried_containers: SmallVec<[(u32, u32, u16); 16]> = SmallVec::new();
    salient_buried_container_pairs.sort_unstable();
    for &(id, container) in &salient_buried_container_pairs {
        if let Some(last) = salient_buried_containers.last_mut()
            && last.0 == id
            && last.1 == container
        {
            last.2 = last.2.saturating_add(1);
        } else {
            salient_buried_containers.push((id, container, 1));
        }
    }

    DpTokens {
        ids,
        chars,
        token_chars,
        token_mask,
        token_multiset,
        salient_chars,
        salient_token_chars,
        salient_mask,
        salient_multiset,
        salient_anchored_multiset,
        salient_buried_containers,
    }
}

#[inline(always)]
pub(crate) fn merge_multisets(
    a: &[(u32, u16)],
    b: &[(u32, u16)],
    out: &mut SmallVec<[(u32, u16); 32]>,
) {
    out.clear();
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        let (id_a, c_a) = a[i];
        let (id_b, c_b) = b[j];
        if id_a == id_b {
            out.push((id_a, c_a.saturating_add(c_b)));
            i += 1;
            j += 1;
        } else if id_a < id_b {
            out.push((id_a, c_a));
            i += 1;
        } else {
            out.push((id_b, c_b));
            j += 1;
        }
    }
    while i < a.len() {
        out.push(a[i]);
        i += 1;
    }
    while j < b.len() {
        out.push(b[j]);
        j += 1;
    }
}

/// Strip trailing delimiters for prefix-containment checks.
pub(crate) fn trim_trailing_delims<'a>(t: &'a str, profile: Option<&LangProfile>) -> &'a str {
    if let Some(p) = profile {
        p.trim_trailing_delimiters(t)
    } else {
        let mut s = t.trim_end();
        while let Some(last) = s.chars().next_back() {
            if matches!(last, '{' | '}' | ';' | ':' | ',' | ')' | ']') {
                if last == ':' && s.ends_with("::") {
                    break;
                }
                s = &s[..s.len() - last.len_utf8()];
                s = s.trim_end();
            } else {
                break;
            }
        }
        s
    }
}

/// Calculate count of common leading whitespace characters between two lines.
#[inline]
pub(crate) fn calc_common_indent(old_text: &str, new_text: &str) -> usize {
    old_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .zip(new_text.chars().take_while(|c| c.is_whitespace()))
        .take_while(|(c1, c2)| c1 == c2)
        .count()
}
