//! One-for-one port of `review.ts`'s syntax classification (PLAN.md §3):
//! `langProfile`, `tokenReFor`, `classifyTokens`. The output carries the
//! SEMANTIC colors (`syntaxKeyword`, `syntaxString`, …) — the renderer
//! maps them to theme colors.

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// A per-language syntax profile.
#[derive(Clone, Debug)]
pub struct LangProfile {
    pub keywords: HashSet<String>,
    /// `#` to end of line is a comment (python/shell/yaml/toml/markdown).
    pub hash_comment: bool,
    /// `//` to end of line.
    pub line_comment: bool,
    /// `/* ... */` block comment.
    pub block_comment: bool,
    /// Capitalized identifiers are types (Rust/TS/C/Go...).
    pub types_by_case: bool,
    /// The FENCE: a full-line pattern (the last capture group = the region
    /// language, possibly empty) that toggles a SYNTAX REGION — the span of
    /// lines between two matching lines classifies with a different
    /// language than the file's (md's `` ```lang ``, rust's doc-comment
    /// `` /// ```lang `` examples). None = the profile has no regions.
    pub fence: Option<Regex>,
    /// The language an UNLABELED fence (` ``` ` with no capture) defaults to.
    /// None = the plain "code" style (no syntax, no highlighting) — md's
    /// tree diagrams stay unhighlighted; rust's unlabeled doc examples
    /// default to the file's own rust.
    pub unlabeled_fence_lang: Option<&'static str>,
    /// A leading per-line pattern to strip before classifying the region's
    /// content: a rust doc fence's code lines are comment-prefixed
    /// (`/// let x = 5;`) — the prefix classifies as a comment, the
    /// remainder as the region language.
    pub fence_strip: Option<Regex>,
    /// The default region is the markdown PROSE (headings, emphasis — the
    /// `markdown_tokens` classifier). True only for the md family proper —
    /// toml/yaml share the profile's token set but not the prose tokens.
    pub markdown: bool,
}

const KEYWORDS_RUST: &str = "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while macro_rules yield";
const KEYWORDS_TS: &str = "as async await break case catch class const continue debugger default delete do else enum export extends false finally for from function get if import in instanceof interface let new null of return set static super switch this throw true try typeof var void while with yield";
const KEYWORDS_PY: &str = "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield";
const KEYWORDS_SH: &str = "if then else elif fi case esac for while until do done function in return exit local export echo set unset true false break continue";
const KEYWORDS_C: &str = "auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while";
const KEYWORDS_GO: &str = "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var";

fn kw(set: &str) -> HashSet<String> {
    set.split_whitespace().map(String::from).collect()
}

pub fn lang_profile(ext: &str) -> Option<LangProfile> {
    // Fence ids are LANGUAGE names (` ```rust `); the profiles are keyed by
    // the file EXTENSIONS (`rs`). Map the common aliases so a labeled fence
    // actually highlights.
    let ext = match ext {
        "rust" => "rs",
        "javascript" | "jsx" => "js",
        "typescript" | "tsx" => "ts",
        "python" => "py",
        "shell" | "bash" => "sh",
        "c++" | "cpp" => "cpp",
        "markdown" => "md",
        other => other,
    };
    let profile = match ext {
        "rs" => LangProfile {
            keywords: kw(KEYWORDS_RUST),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
            // Doc-comment examples: `/// ```rust` opens a rust-syntax region.
            fence: Some(
                Regex::new(r"^\s*(///|//!)\s*```([a-z0-9_+-]*)\s*$").expect("the rust doc fence"),
            ),
            // An unlabeled doc fence is a rust example.
            unlabeled_fence_lang: Some("rust"),
            // The region's code lines are comment-prefixed (`/// let x = 5;`)
            // — strip the prefix, classify the remainder.
            fence_strip: Some(Regex::new(r"^\s*(///|//!)?\s*").expect("the doc prefix")),
            markdown: false,
        },
        "ts" | "tsx" | "js" | "jsx" | "mjs" => LangProfile {
            keywords: kw(KEYWORDS_TS),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "py" => LangProfile {
            keywords: kw(KEYWORDS_PY),
            hash_comment: true,
            line_comment: false,
            types_by_case: true,
            block_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "sh" | "bash" => LangProfile {
            keywords: kw(KEYWORDS_SH),
            hash_comment: true,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "c" | "h" | "cpp" | "hpp" | "java" => LangProfile {
            keywords: kw(KEYWORDS_C),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "go" => LangProfile {
            keywords: kw(KEYWORDS_GO),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "md" | "markdown" => LangProfile {
            keywords: HashSet::new(),
            hash_comment: true,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
            // The md fence: ` ```lang ` — group 1 = the region language.
            fence: Some(Regex::new(r"^\s*```([a-z0-9_+-]*)\s*$").expect("the md fence")),
            // An unlabeled fence = the plain "code" style (no syntax).
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: true,
        },
        // toml/yaml share the token set but not the prose classifier.
        "toml" | "yaml" | "yml" => LangProfile {
            keywords: HashSet::new(),
            hash_comment: true,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        "json" => LangProfile {
            keywords: HashSet::new(),
            hash_comment: false,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
            fence: None,
            unlabeled_fence_lang: None,
            fence_strip: None,
            markdown: false,
        },
        _ => return None,
    };
    Some(profile)
}

// Regex literal parts; the per-language alternation is composed from
// `re.source` (regex escapes, no string-escaping pitfalls).
const RE_HASH: &str = r"#[^\n]*";
const RE_LINE: &str = r"//[^\n]*";
const RE_BLOCK: &str = r"/\*[\s\S]*?\*/";
const RE_RAW: &str = r##"r#*"(?:[^"\\]|\\.)*"#*"##;
const RE_DSTR: &str = r##""(?:[^"\\\n]|\\.)*""##;
const RE_SSTR: &str = r"'(?:[^'\\\n]|\\.)*'";
const RE_BT: &str = r"`(?:[^`\\\n]|\\.)*`";
const RE_HEX: &str = r"\b0x[0-9a-fA-F_]+\b";
const RE_NUM: &str = r"\b\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?\b";
const RE_ID: &str = r"[A-Za-z_][A-Za-z0-9_]*";
const RE_WS: &str = r"\s+";
const RE_OTHER: &str = ".";

/// The token regex for a profile: the comment/string/number/id/ws/other
/// alternation in the TS's fixed order.
pub fn token_re_for(profile: &LangProfile) -> Regex {
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<u8, Regex>>> =
        OnceLock::new();
    // The key: the profile's feature flags (the regex depends on them —
    // comment kinds). The TS caches per profile; a per-flags code works the
    // same. THE CACHE MUST BE USED — the regex compile is expensive and the
    // classify runs per row (and per merged fragment).
    let key = (u8::from(profile.hash_comment))
        | (u8::from(profile.line_comment) << 1)
        | (u8::from(profile.block_comment) << 2);
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    {
        let guard = cache.lock().unwrap();
        if let Some(re) = guard.get(&key) {
            return re.clone();
        }
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
    cache.lock().unwrap().insert(key, re.clone());
    re
}

/// A classified token: the semantic color name + its text (+ an optional
/// SGR attribute — the markdown emphasis: bold/italic/underline/strike).
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedToken {
    pub color: String,
    pub sgr: Option<u32>,
    pub text: String,
}

/// The markdown SGR attributes (the renderer's `MD_SGR`).
pub const MD_SGR_BOLD: u32 = 1;
pub const MD_SGR_ITALIC: u32 = 3;
pub const MD_SGR_UNDERLINE: u32 = 4;
pub const MD_SGR_STRIKE: u32 = 9;

/// Classify a run of text into semantic tokens (plain `text` when there is
/// no profile).
pub fn classify_tokens(text: &str, profile: Option<&LangProfile>) -> Vec<ClassifiedToken> {
    let Some(profile) = profile else {
        return vec![ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text.into(),
        }];
    };
    if text.is_empty() {
        return vec![ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text.into(),
        }];
    }
    let re = token_re_for(profile);
    let mut out: Vec<ClassifiedToken> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: text[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        let next = text[m.end()..].chars().next();
        let color = if (profile.hash_comment && tok.starts_with('#'))
            || tok.starts_with("//")
            || tok.starts_with("/*")
        {
            "syntaxComment"
        } else if tok.starts_with('"')
            || tok.starts_with('\'')
            || tok.starts_with('`')
            || tok.starts_with("r\"")
        {
            "syntaxString"
        } else if tok.starts_with("0x")
            || tok.starts_with("0X")
            || tok.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            "syntaxNumber"
        } else if tok
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            if profile.keywords.contains(tok) {
                "syntaxKeyword"
            } else if profile.types_by_case
                && tok.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            {
                "syntaxType"
            } else if next == Some('(') {
                "syntaxFunction"
            } else {
                "text"
            }
        } else if tok.chars().all(|c| c.is_whitespace()) {
            "text"
        } else {
            "syntaxOperator"
        };
        out.push(ClassifiedToken {
            color: color.into(),
            sgr: None,
            text: tok.into(),
        });
        last = m.end();
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text[last..].into(),
        });
    }
    out
}

#[cfg(test)]
mod tests;

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
pub fn fence_regions(lines: &[String], profile: Option<&LangProfile>) -> Vec<FenceRegion> {
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
        let n = (i + 1) as u32;
        if fence.is_match(line) {
            match state {
                None => {
                    let lang = fence
                        .captures(line)
                        .and_then(|c| c.iter().last().flatten())
                        .map(|m| m.as_str().to_string())
                        .filter(|l| !l.is_empty())
                        .or_else(|| profile.unlabeled_fence_lang.map(String::from));
                    state = Some(lang);
                    region_start = n + 1;
                }
                Some(_) => {
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

/// The markdown tokens — the md default region's classifier (the renderer's
/// `markdownTokens` ported): `#`-headings (the marker muted, the title
/// accent + bold), inline code (syntax-string), `**bold**`, `*italic*`,
/// `~~strike~~`, `[text](url)` — the SGR rides the token.
pub fn markdown_tokens(text: &str) -> Vec<ClassifiedToken> {
    // The md regexes are FIXED — compile ONCE (the classify runs per row /
    // per merged fragment; a recompile per call was the classify's cost for
    // the markdown corpus).
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static MD_INLINE: OnceLock<Regex> = OnceLock::new();
    let heading =
        HEADING.get_or_init(|| Regex::new(r"^(#{1,6})(\s+)(.*)$").expect("the md heading"));
    if let Some(h) = heading.captures(text) {
        return vec![
            ClassifiedToken {
                color: "muted".into(),
                sgr: None,
                text: h[1].to_string() + &h[2],
            },
            ClassifiedToken {
                color: "accent".into(),
                sgr: Some(MD_SGR_BOLD),
                text: h[3].to_string(),
            },
        ];
    }
    let re = MD_INLINE.get_or_init(|| {
        Regex::new(
            r"(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))",
        )
        .expect("the md inline")
    });
    let mut out: Vec<ClassifiedToken> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: text[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        let (color, sgr) = if tok.starts_with('`') {
            ("syntaxString", None)
        } else if tok.starts_with("~~") {
            ("muted", Some(MD_SGR_STRIKE))
        } else if tok.starts_with("**") {
            ("accent", Some(MD_SGR_BOLD))
        } else if tok.starts_with('[') {
            // The TS splits the link: the label + the URL, both underlined.
            let close = tok.find("](").unwrap_or(0);
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: Some(MD_SGR_UNDERLINE),
                text: tok[..close + 1].into(),
            });
            out.push(ClassifiedToken {
                color: "muted".into(),
                sgr: Some(MD_SGR_UNDERLINE),
                text: tok[close + 1..].into(),
            });
            last = m.end();
            continue;
        } else {
            ("muted", Some(MD_SGR_ITALIC))
        };
        out.push(ClassifiedToken {
            color: color.into(),
            sgr,
            text: tok.into(),
        });
        last = m.end();
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text[last..].into(),
        });
    }
    out
}

/// The line's region (if its line number is inside a fence region's code
/// span) — the renderer's ctx/tail rows look up their line's language here.
pub fn region_at(regions: &[FenceRegion], line: u32) -> Option<&FenceRegion> {
    regions.iter().find(|r| r.start <= line && line <= r.end)
}

/// The full-line classifier: the file's default region (md → the markdown
/// tokens, else the profile), or a fence region's language (with the file's
/// comment prefix stripped first — `/// let x = 5;` → `let x = 5;`).
pub fn classify_line(
    text: &str,
    file_profile: Option<&LangProfile>,
    region: Option<&FenceRegion>,
) -> Vec<ClassifiedToken> {
    match region {
        Some(r) => {
            let profile = r.lang.as_deref().and_then(lang_profile);
            match (
                profile.as_ref(),
                file_profile.and_then(|p| p.fence_strip.as_ref()),
            ) {
                (Some(p), Some(strip)) => {
                    let mut out: Vec<ClassifiedToken> = Vec::new();
                    if let Some(m) = strip.find(text)
                        && m.start() == 0
                        && m.end() > 0
                    {
                        out.push(ClassifiedToken {
                            color: "syntaxComment".into(),
                            sgr: None,
                            text: text[..m.end()].into(),
                        });
                        out.extend(classify_tokens(&text[m.end()..], Some(p)));
                        return out;
                    }
                    classify_tokens(text, Some(p))
                }
                _ => classify_tokens(text, profile.as_ref()),
            }
        }
        None => {
            if file_profile.is_some_and(|p| p.markdown) {
                // The md default region is the markdown prose.
                markdown_tokens(text)
            } else {
                classify_tokens(text, file_profile)
            }
        }
    }
}
