//! Profile registry — `lang_profile` lookup and caching, `fence_lang_ext`
//! normalization, and `token_re_for` regex construction with its hand-rolled
//! character-class scanner.
//!
//! The scanner (`syntax_token_spans`) is the performance-critical path: the
//! regex crate's NFA on the ~12-alternative token alternation took ~47µs/call;
//! the hand-rolled scan is ~ns/char (~50× faster) and behavior-identical.

pub mod map;
pub mod regex;
pub mod scanner;
pub mod unicode;
pub mod word;

pub use map::{REGISTERED_EXTENSIONS, all_registered_profiles, fence_lang_ext, lang_profile};
pub use regex::token_re_for;
pub use scanner::{for_each_syntax_token_span, syntax_token_spans};
pub use word::{first_word, is_word_char, word_prefix, word_prefix_len};
