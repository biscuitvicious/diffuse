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

/// Normalize a fence tag (or file extension) to a profile key.
///
/// Language aliases map to their profile (`rust`→`rs`, `typescript`→`ts`…).
/// A Rust doc-comment fence carries a comma-separated **doctest attribute
/// list** — `rust,no_run`, `should_panic`, `edition2021`, `ignore_linux`, or
/// combos like `rust,no_run,edition2021`. Any `rust` token ⇒ a Rust example;
/// any modifier (`no_run`, `ignore`, `should_panic`, `compile_fail`,
/// `edition*`, `ignore_*`) ⇒ Rust is implied (the default lang in a `.rs`
/// doc fence is rust). A named language alias embedded in a rust crate's
/// doc (` ```python `) takes precedence and highlights as that language.
pub fn fence_lang_ext(tag: &str) -> &str {
    let mut rust_implied = false;
    for part in tag.split(',') {
        let p = part.trim();
        match p {
            "rust" => return "rs",
            "javascript" | "jsx" => return "js",
            "typescript" | "tsx" => return "ts",
            "python" => return "py",
            "shell" | "bash" => return "sh",
            "c++" | "cpp" => return "cpp",
            "markdown" | "md" => return "md",
            "ignore" | "no_run" | "should_panic" | "compile_fail" => rust_implied = true,
            _ if p.starts_with("edition") || p.starts_with("ignore_") => rust_implied = true,
            _ => {}
        }
    }
    if rust_implied { "rs" } else { tag }
}

pub fn lang_profile(ext: &str) -> Option<&'static LangProfile> {
    // Fence ids are LANGUAGE names (` ```rust `); the profiles are keyed by
    // the file EXTENSIONS (`rs`). Map the common aliases — and the rust
    // doctest attribute lists — so a labeled fence actually highlights.
    let ext = fence_lang_ext(ext);
    let profile = match ext {
        "rs" => LangProfile {
            keywords: kw(KEYWORDS_RUST),
            line_comment: true,
            block_comment: true,
            types_by_case: true,
            hash_comment: false,
            // Doc-comment examples: `/// ```rust` opens a rust-syntax region.
            fence: Some(
                // The capture allows commas: a rust doc fence carries a
                // comma-separated doctest attribute list —
                // `/// ```rust,no_run`, `/// ```should_panic`, `/// ```edition2021`.
                Regex::new(r"^\s*(///|//!)\s*```([a-z0-9_+,-]*)\s*$").expect("the rust doc fence"),
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
            // The md fence: ` ```lang ` — group 1 = the region language
            // (commas allowed for ` ```rust,no_run ` inside an .md file).
            fence: Some(Regex::new(r"^\s*```([a-z0-9_+,-]*)\s*$").expect("the md fence")),
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
    // The profile rebuilds a keyword HashSet + 2 regexes — and
    // lang_profile is called PER ROW inside fence regions (classify_line
    // / classify_frag resolve the region's language). Cache by the
    // canonical ext: build once, leak the Box, hand out a 'static ref so
    // no caller clones or rebuilds (the fence-region classify was the
    // remaining classify cost).
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, &'static LangProfile>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(p) = guard.get(ext) {
        return Some(*p);
    }
    let boxed: &'static LangProfile = Box::leak(Box::new(profile));
    guard.insert(ext.to_string(), boxed);
    Some(boxed)
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
/// The token spans of the syntax regex's `find_iter` — a hand-rolled
/// char-class scanner that produces the SAME byte-offset spans as
/// `token_re_for(profile).find_iter(text)`, in the same alternation order
/// (first match wins). The regex crate takes the SLOW NFA path on this
/// alternation (the `\b` word boundaries + non-greedy block comments +
/// ~12 alternatives defeat the DFA) — ~47µs/call, the whole classify cost.
/// This scanner is ~ns/char: a ~50× cut. The color logic in
/// `classify_tokens` runs unchanged on these spans (behavior-identical,
/// golden-gated).
fn syntax_token_spans(text: &str, profile: &LangProfile) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let end_off = |i: usize| if i < n { chars[i].0 } else { text.len() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let start_off = chars[i].0;
        let c = chars[i].1;
        // Try the alternation in the TS's fixed order; first match wins.
        // The `#`-comment and `/`-comment both scan to EOL — one merged arm.
        let next = if (profile.hash_comment && c == '#')
            || (profile.line_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '/')
        {
            scan_to_eol(&chars, i)
        } else if profile.block_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '*' {
            scan_block(&chars, i).unwrap_or(i + 1)
        } else if c == 'r' {
            scan_raw(&chars, i).unwrap_or_else(|| scan_ident(&chars, i))
        } else if c == '"' {
            scan_dstr(&chars, i)
        } else if c == '\'' {
            scan_sstr(&chars, i)
        } else if c == '`' {
            scan_bt(&chars, i)
        } else if c == '0' && i + 1 < n && chars[i + 1].1 == 'x' {
            // \b0x[0-9a-fA-F_]+\b. The leading \b is satisfied (a '0'
            // start never follows a word char — idents consume those).
            scan_hex(&chars, i + 2).unwrap_or_else(|| scan_num_or_other(&chars, i))
        } else if c.is_ascii_digit() {
            scan_num_or_other(&chars, i)
        } else if c.is_ascii_alphabetic() || c == '_' {
            scan_ident(&chars, i)
        } else if c.is_whitespace() {
            scan_ws(&chars, i)
        } else {
            i + 1 // RE_OTHER: one char (. does not match \n, but \n is ws)
        };
        out.push((start_off, end_off(next)));
        i = next;
    }
    out
}

// `#[^\n]*` / `//[^\n]*` — to end of line (the char before the next `\n`).
fn scan_to_eol(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 != '\n' {
        j += 1;
    }
    j
}

// `/*[\s\S]*?*/` — the FIRST `*/` (non-greedy). None if unterminated
// (the regex requires the closing `*/`; unterminated → no block match).
fn scan_block(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 2;
    while j < n {
        if chars[j].1 == '*' && j + 1 < n && chars[j + 1].1 == '/' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

// `r#*"(?:[^"\\]|\\.)*"#*` — the raw string. Opening `#*` is the run before
// the `"`; trailing `#*` is an independent run after the closing `"`
// (the regex does NOT tie the counts). None if malformed.
fn scan_raw(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    if j >= n || chars[j].1 != '"' {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 != '"' {
        // `\\.` skips one escaped char.
        j += if chars[j].1 == '\\' { 2 } else { 1 };
    }
    if j >= n {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    Some(j)
}

// `"(?:[^"\\\n]|\\.)*"` / `'...'` / `` `...` `` — to the closing quote,
// no raw newline. The string ends at EOF if no closing quote (the regex
// `*` allows no close — the `[...]*` matches to end and the `"` is…
// actually required: the pattern is `"(?:[^"\\\n]|\\.)*"` — the trailing
// `"` IS required. No close → no match → `.` for the quote char.
fn scan_quoted(chars: &[(usize, char)], i: usize, quote: char) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c == '\\' {
            j += 2;
        } else if c == quote {
            return Some(j + 1);
        } else if c == '\n' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

fn scan_dstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '"').unwrap_or(i + 1)
}
fn scan_sstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '\'').unwrap_or(i + 1)
}
fn scan_bt(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '`').unwrap_or(i + 1)
}

// `[0-9a-fA-F_]+` then `\b`: trailing word boundary. None if no hex digit
// or the trailing char is a word char (the `+` needs ≥1 hex digit, and the
// `\b` needs a non-word after — a trailing word char fails the whole hex).
fn scan_hex(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i;
    while j < n {
        let c = chars[j].1;
        if c.is_ascii_hexdigit() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    if j == i {
        return None; // `+` needs ≥1 hex digit
    }
    // trailing \b: next char non-word (or EOF).
    if j < n && is_word_char(chars[j].1) {
        return None;
    }
    Some(j)
}

// `\b\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?\b`. Scan the full greedy num
// (digits, optional decimal, optional exponent), then verify the trailing
// `\b`. If it fails, the regex backtracks: drop the exponent, then the
// decimal, down to the bare digit run — the first drop whose end sits
// before a non-word char wins. If even the bare digit run fails the
// trailing `\b` (a word char immediately follows, e.g. `5x`), the whole
// num fails → the single digit is `.` (RE_OTHER).
fn scan_num_or_other(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    // bare digit run `\d[\d_]*`
    let mut digits_end = i;
    while digits_end < n {
        let c = chars[digits_end].1;
        if c.is_ascii_digit() || c == '_' {
            digits_end += 1;
        } else {
            break;
        }
    }
    // optional decimal `\.[\d_]+`
    let mut after_dec = digits_end;
    if digits_end < n && chars[digits_end].1 == '.' {
        let mut j = digits_end + 1;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > digits_end + 1 {
            after_dec = j;
        }
    }
    // optional exponent `[eE][+-]?\d+`
    let mut after_exp = after_dec;
    if after_dec < n && (chars[after_dec].1 == 'e' || chars[after_dec].1 == 'E') {
        let mut j = after_dec + 1;
        if j < n && (chars[j].1 == '+' || chars[j].1 == '-') {
            j += 1;
        }
        let start_digits = j;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > start_digits {
            after_exp = j;
        }
    }
    // Backtrack ladder (longest first): full, without exp, without decimal.
    for &end in &[after_exp, after_dec, digits_end] {
        let tail_ok = end >= n || !is_word_char(chars[end].1);
        if tail_ok {
            return end;
        }
    }
    i + 1 // bare digit run followed by a word char → num fails → RE_OTHER `.`
}

// `[A-Za-z_][A-Za-z0-9_]*`
fn scan_ident(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c.is_ascii_alphanumeric() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    j
}

// `\s+` (Unicode whitespace, like the regex crate's `\s`).
fn scan_ws(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1.is_whitespace() {
        j += 1;
    }
    j
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

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
    // The hand-rolled scanner produces the same spans as the regex's
    // find_iter (see `syntax_token_spans`) — the regex crate's NFA path
    // on this alternation was the whole classify cost (~50× slower).
    let spans = syntax_token_spans(text, profile);
    let mut out: Vec<ClassifiedToken> = Vec::with_capacity(spans.len());
    let mut last = 0usize;
    for (start, end) in spans {
        if start > last {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: text[last..start].into(),
            });
        }
        let tok = &text[start..end];
        let next = text[end..].chars().next();
        let is_comment = (profile.hash_comment && tok.starts_with('#'))
            || tok.starts_with("//")
            || tok.starts_with("/*");
        if is_comment {
            out.extend(tokenize_comment_with_backticks(tok, profile));
            last = end;
            continue;
        }

        let color = if tok.starts_with('"')
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
        last = end;
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

fn tokenize_comment_with_backticks(tok: &str, profile: &LangProfile) -> Vec<ClassifiedToken> {
    // Doc comments (`///`, `//!`) are MARKDOWN — rustdoc processes them, so
    // `**bold**`, `*italic*`, `~~strike~~`, `[link](url)`, and `` `code` ``
    // all render. Plain comments (`//`, `/*`, `#`) are NOT markdown; the
    // convention (rust-analyzer, GitHub) is a uniform comment color. The one
    // exception: inline `` `code` `` spans in a plain comment still render as
    // Rust (accent backticks + classified inner) — they read as code refs,
    // not prose. No markdown emphasis/link/strike on plain comments.
    let is_doc = tok.starts_with("///") || tok.starts_with("//!");
    // Doc: any markdown trigger opens the splitter. Plain: only a backtick.
    let has_markdown = is_doc
        && (tok.contains('`') || tok.contains("**") || tok.contains("~~") || tok.contains('['));
    let has_backtick = tok.contains('`');
    if !has_markdown && !has_backtick {
        return vec![ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: tok.into(),
        }];
    }

    let rust_prof = lang_profile("rs").unwrap_or(profile);
    let prefix_len = if is_doc {
        3
    } else if tok.starts_with("//") || tok.starts_with("/*") {
        2
    } else if tok.starts_with('#') {
        1
    } else {
        0
    };

    let mut out = Vec::new();
    if prefix_len > 0 {
        out.push(ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: tok[..prefix_len].into(),
        });
    }

    let body = &tok[prefix_len..];
    if is_doc {
        // Doc comments: full markdown (emphasis, links, code) — the text
        // gaps between markdown spans are comment-colored.
        for mut t in markdown_tokens(body) {
            if t.color == "text" {
                t.color = "syntaxComment".into();
            } else if t.color == "syntaxString" {
                let parent_sgr = t.sgr;
                let mut inner_toks = classify_tokens(&t.text, Some(rust_prof));
                for it in &mut inner_toks {
                    if it.sgr.is_none() {
                        it.sgr = parent_sgr;
                    }
                }
                out.extend(inner_toks);
                continue;
            }
            out.push(t);
        }
    } else {
        // Plain comments: backtick-only. Everything outside `` `code` `` is
        // comment-color; inside, the Rust classification.
        out.extend(backtick_tokens(body, rust_prof));
    }
    out
}

/// Backtick-only extraction for plain (`//`, `/*`, `#`) comments: only
/// inline `` `code` `` spans render — accent backticks + Rust-classified
/// inner content; the surrounding comment text stays `syntaxComment`. No
/// markdown emphasis/link/strike (plain comments aren't markdown).
fn backtick_tokens(body: &str, rust_prof: &LangProfile) -> Vec<ClassifiedToken> {
    static BACKTICK: OnceLock<Regex> = OnceLock::new();
    let re = BACKTICK.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the comment backtick"));
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(body) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "syntaxComment".into(),
                sgr: None,
                text: body[last..m.start()].into(),
            });
        }
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        let inner = &m.as_str()[1..m.as_str().len() - 1];
        out.extend(classify_tokens(inner, Some(rust_prof)));
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        last = m.end();
    }
    if last < body.len() {
        out.push(ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: body[last..].into(),
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
                        // previous fence was unclosed, close it at n-1 and start a new region at n+1.
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
                        // A bare fence (`/// ````) closes the open region.
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

/// Parse the inner content of a bold/italic/link span, stacking the parent
/// SGR on top of any inline code children. The child's *color* wins; the
/// parent's *SGR* (bold, italic, underline) is inherited — so e.g.
/// `**`code`**` renders as code-color + bold rather than mdBold-color + bold.
pub(crate) fn md_inner_tokens(
    inner: &str,
    parent_sgr: u32,
    parent_color: &str,
) -> Vec<ClassifiedToken> {
    static INNER_CODE: OnceLock<Regex> = OnceLock::new();
    let re = INNER_CODE.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the inner code"));
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(inner) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: parent_color.into(),
                sgr: Some(parent_sgr),
                text: inner[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        // `code` inside bold/italic: code color wins, parent SGR stacks.
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        out.push(ClassifiedToken {
            color: "syntaxString".into(),
            sgr: Some(parent_sgr),
            text: tok[1..tok.len() - 1].into(),
        });
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        last = m.end();
    }
    if last < inner.len() {
        out.push(ClassifiedToken {
            color: parent_color.into(),
            sgr: Some(parent_sgr),
            text: inner[last..].into(),
        });
    }
    out
}

/// The markdown tokens — the md default region's classifier (the renderer's
/// `markdownTokens` ported): `#`-headings (the marker muted, the title
/// accent + bold), inline code (syntax-string), `**bold**`, `*italic*`,
/// `~~strike~~`, `[text](url)` — the SGR rides the token.
pub fn markdown_tokens(text: &str) -> Vec<ClassifiedToken> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static MD_INLINE: OnceLock<Regex> = OnceLock::new();
    let heading =
        HEADING.get_or_init(|| Regex::new(r"^(#{1,6})(\s+)(.*)$").expect("the md heading"));
    if let Some(h) = heading.captures(text) {
        return vec![
            ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: h[1].to_string(),
            },
            ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: h[2].to_string(),
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
            r"(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\](?:\([^)\n]+\))?)",
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
        if tok.starts_with('`') {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
            out.push(ClassifiedToken {
                color: "syntaxString".into(),
                sgr: None,
                text: tok[1..tok.len() - 1].into(),
            });
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
        } else if tok.starts_with("~~") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_STRIKE,
                "mdStrike",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
        } else if tok.starts_with("**") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_BOLD,
                "mdBold",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
        } else if tok.starts_with('[') {
            if let Some(close) = tok.find("](") {
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "[".into(),
                });
                out.extend(md_inner_tokens(&tok[1..close], MD_SGR_UNDERLINE, "mdLink"));
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "](".into(),
                });
                out.push(ClassifiedToken {
                    color: "mdUrl".into(),
                    sgr: Some(MD_SGR_UNDERLINE),
                    text: tok[close + 2..tok.len() - 1].into(),
                });
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: ")".into(),
                });
            } else {
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "[".into(),
                });
                out.extend(md_inner_tokens(
                    &tok[1..tok.len() - 1],
                    MD_SGR_UNDERLINE,
                    "mdLink",
                ));
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "]".into(),
                });
            }
        } else {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
            out.extend(md_inner_tokens(
                &tok[1..tok.len() - 1],
                MD_SGR_ITALIC,
                "mdItalic",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
        }

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
                _ => classify_tokens(text, profile),
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
