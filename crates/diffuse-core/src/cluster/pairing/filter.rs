//! Fragment pair validation and scope boundaries.

use diffuse_syntax::profile::LangProfile;

use crate::{parse::Hunk, scope::fn_of_exists};

use super::survival::{FnScopeCache, memo_fn_of_str};

pub(crate) fn is_fragment_pair_rejected<'a>(
    gd: usize,
    ga: usize,
    h: &Hunk,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    fn_cache: &mut FnScopeCache<'a>,
) -> bool {
    let r = &h.rows[gd];
    let a = &h.rows[ga];

    let del_fn = memo_fn_of_str(fn_cache, true, old_content, r.old_line(), profile);
    let add_fn = memo_fn_of_str(fn_cache, false, new_content, a.new_line(), profile);
    if let (Some(dfn), Some(afn)) = (del_fn, add_fn)
        && dfn != afn
        && (fn_of_exists(new_content, dfn, profile) || fn_of_exists(old_content, afn, profile))
    {
        return true;
    }

    false
}
