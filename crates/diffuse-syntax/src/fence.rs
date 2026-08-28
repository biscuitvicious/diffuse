//! Fence region detection — finds the `FenceRegion` spans (fenced code blocks)
//! inside a file's line sequence, and provides lookup helpers.

use super::{profile::LangProfile, registry::lang_profile};

/// A syntax region: the span of lines (1-indexed, inclusive) whose content
/// classifies with a different language than the file's — the fenced code.
/// The delimiter lines (start-1 / end+1, when within the file) classify in
/// the file's default and render muted. `lang` = the region language (None
/// = the plain "code" style — no syntax, no highlighting).
#[derive(Clone, Debug, PartialEq)]
pub struct FenceRegion {
    pub start: u32,
    pub end: u32,
    pub lang: Option<String>,
}

/// Walk the file's lines and find its fence regions — the generic version of
/// the renderer's old markdown-only fence toggle. A line matching the
/// profile's fence pattern toggles (open on the first match, close on the
/// next — no nesting); the region language = the captured group, or the
/// profile's `unlabeled_fence_lang`, or the plain "code" style. An
/// unterminated fence runs to the file's end.
pub fn fence_regions<S: AsRef<str>>(
    lines: &[S],
    profile: Option<&LangProfile>,
) -> Vec<FenceRegion> {
    let Some(profile) = profile else {
        return Vec::new();
    };
    let Some(fence) = profile.fence.as_ref() else {
        return Vec::new();
    };
    let mut regions: Vec<FenceRegion> = Vec::new();
    // None = closed; Some(None) = open (plain); Some(Some(lang)) = open.
    let mut state: Option<Option<String>> = None;
    let mut region_start: u32 = 0;
    for (i, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let n = (i + 1) as u32;
        if fence.is_match(line) {
            let captured_tag = fence
                .captures(line)
                .and_then(|c| c.iter().last().flatten())
                .map(|m| m.as_str().trim())
                .unwrap_or("");
            match state {
                None => {
                    let lang = if captured_tag.is_empty() {
                        profile.unlabeled_fence_lang.map(String::from)
                    } else {
                        Some(captured_tag.to_string())
                    };
                    state = Some(lang);
                    region_start = n + 1;
                }
                Some(_) => {
                    if !captured_tag.is_empty() {
                        // A tagged fence line (`/// ```ignore`) is an opening fence — if a
                        // previous fence was unclosed, close it at n-1 and start a new region at
                        // n+1.
                        if region_start < n {
                            regions.push(FenceRegion {
                                start: region_start,
                                end: n - 1,
                                lang: state.clone().flatten(),
                            });
                        }
                        state = Some(Some(captured_tag.to_string()));
                        region_start = n + 1;
                    } else {
                        // A bare fence (`/// ```) closes the open region.
                        if region_start < n {
                            regions.push(FenceRegion {
                                start: region_start,
                                end: n - 1,
                                lang: state.clone().flatten(),
                            });
                        }
                        state = None;
                    }
                }
            }
        }
    }
    if let Some(lang) = state {
        let last = lines.len() as u32;
        if region_start <= last {
            regions.push(FenceRegion {
                start: region_start,
                end: last,
                lang,
            });
        }
    }
    regions
}

/// The line's region (if its line number is inside a fence region's code
/// span) — the renderer's ctx/tail rows look up their line's language here.
pub fn region_at(regions: &[FenceRegion], line: u32) -> Option<&FenceRegion> {
    let idx = regions.partition_point(|r| r.end < line);
    if idx < regions.len() && regions[idx].start <= line && line <= regions[idx].end {
        Some(&regions[idx])
    } else {
        None
    }
}

/// Resolve the `LangProfile` for a `FenceRegion` (via its `lang` tag).
pub fn region_profile(region: &FenceRegion) -> Option<&'static LangProfile> {
    region.lang.as_deref().and_then(lang_profile)
}
