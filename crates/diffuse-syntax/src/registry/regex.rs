//! Token regular expression builder and compilation cache.

use std::sync::OnceLock;

use regex::Regex;

use crate::profile::LangProfile;

// Regex literal parts — the per-language alternation is composed from these.
const RE_HASH: &str = r"#[^\n]*";
const RE_LINE: &str = r"//[^\n]*";
const RE_BLOCK: &str = r"/\*[\s\S]*?\*/";
const RE_RAW: &str = r##"r#*"(?:[^"\\]|\\.)*"#*"##;
const RE_DSTR: &str = r##""(?:[^"\\n]|\\.)*""##;
const RE_SSTR: &str = r"'(?:[^'\\n]|\\.)*'";
const RE_BT: &str = r"`(?:[^`\\n]|\\.)*`";
const RE_HEX: &str = r"\b0x[0-9a-fA-F_]+\b";
const RE_NUM: &str = r"\b\d[\d_]*(?:[\d_]+)?(?:[eE][+-]?\d+)?\b";
const RE_ID: &str = r"[A-Za-z_][A-Za-z0-9_]*";
const RE_WS: &str = r"\s+";
const RE_OTHER: &str = ".";

/// The token regex for a profile: the comment/string/number/id/ws/other
/// alternation in the TS's fixed order.
pub fn token_re_for(profile: &LangProfile) -> Regex {
    // Eight slots (3 flag bits): lock-free after warmup. The old
    // Mutex<HashMap> serialized every classify call across worker threads;
    // per-slot OnceLocks pay one atomic load on the hot path.
    static CACHE: [OnceLock<Regex>; 8] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    // The key: the profile's feature flags (the regex depends on them —
    // comment kinds). The TS caches per profile; a per-flags code works the
    // same. THE CACHE MUST BE USED — the regex compile is expensive and the
    // classify runs per row (and per merged fragment).
    let key = (u8::from(profile.hash_comment))
        | (u8::from(profile.line_comment) << 1)
        | (u8::from(profile.block_comment) << 2);
    if let Some(re) = CACHE[key as usize].get() {
        return re.clone();
    }
    let parts: Vec<&str> = [
        if profile.hash_comment {
            Some(RE_HASH)
        } else {
            None
        },
        if profile.line_comment {
            Some(RE_LINE)
        } else {
            None
        },
        if profile.block_comment {
            Some(RE_BLOCK)
        } else {
            None
        },
        Some(RE_RAW),
        Some(RE_DSTR),
        Some(RE_SSTR),
        Some(RE_BT),
        Some(RE_HEX),
        Some(RE_NUM),
        Some(RE_ID),
        Some(RE_WS),
        Some(RE_OTHER),
    ]
    .into_iter()
    .flatten()
    .collect();
    let re = Regex::new(&format!("({})", parts.join("|"))).expect("token regex");
    let slot = &CACHE[key as usize];
    if let Some(cached) = slot.get() {
        return cached.clone();
    }
    let _ = slot.set(re.clone());
    slot.get().unwrap().clone()
}
