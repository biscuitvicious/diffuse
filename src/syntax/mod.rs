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
    let profile = match ext {
        "rs" => LangProfile {
            keywords: kw(KEYWORDS_RUST),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
        },
        "ts" | "tsx" | "js" | "jsx" | "mjs" => LangProfile {
            keywords: kw(KEYWORDS_TS),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
        },
        "py" => LangProfile {
            keywords: kw(KEYWORDS_PY),
            hash_comment: true,
            line_comment: false,
            types_by_case: true,
            block_comment: false,
        },
        "sh" | "bash" => LangProfile {
            keywords: kw(KEYWORDS_SH),
            hash_comment: true,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
        },
        "c" | "h" | "cpp" | "hpp" | "java" => LangProfile {
            keywords: kw(KEYWORDS_C),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
        },
        "go" => LangProfile {
            keywords: kw(KEYWORDS_GO),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
        },
        "md" | "markdown" | "toml" | "yaml" | "yml" => LangProfile {
            keywords: HashSet::new(),
            hash_comment: true,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
        },
        "json" => LangProfile {
            keywords: HashSet::new(),
            hash_comment: false,
            line_comment: false,
            types_by_case: false,
            block_comment: false,
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
    // (The TS caches per profile; the profile set is small and fixed — a
    // per-lang code key works the same.)
    let _ = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
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
    Regex::new(&format!("({})", parts.join("|"))).expect("token regex")
}

/// A classified token: the semantic color name + its text.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedToken {
    pub color: String,
    pub text: String,
}

/// Classify a run of text into semantic tokens (plain `text` when there is
/// no profile).
pub fn classify_tokens(text: &str, profile: Option<&LangProfile>) -> Vec<ClassifiedToken> {
    let Some(profile) = profile else {
        return vec![ClassifiedToken {
            color: "text".into(),
            text: text.into(),
        }];
    };
    if text.is_empty() {
        return vec![ClassifiedToken {
            color: "text".into(),
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
            text: tok.into(),
        });
        last = m.end();
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "text".into(),
            text: text[last..].into(),
        });
    }
    out
}

#[cfg(test)]
mod tests;
