use super::*;

#[test]
fn test_magic_theme_token_colors() {
    let theme = DiffuseTheme::MAGIC;
    let kw_add = theme.render_token("syntaxKeyword", "fn", "add", None, None);
    assert_eq!(kw_add, "\x1b[38;5;147mfn\x1b[39m");

    let kw_del = theme.render_token("syntaxKeyword", "fn", "del", None, None);
    assert_eq!(kw_del, "\x1b[38;5;183mfn\x1b[39m");

    let kw_ctx = theme.render_token("syntaxKeyword", "fn", "ctx", None, None);
    assert_eq!(kw_ctx, "\x1b[38;5;141mfn\x1b[39m");
}

#[test]
fn test_sgr_modifiers_application_and_reset() {
    let theme = DiffuseTheme::MAGIC;
    let bold_tok = theme.render_token("mdBold", "heading", "ctx", None, Some(1));
    assert!(bold_tok.starts_with("\x1b[1m"));
    assert!(bold_tok.ends_with("\x1b[22m"));

    let strike_tok = theme.render_token("mdStrike", "removed", "ctx", None, Some(9));
    assert!(strike_tok.starts_with("\x1b[9m"));
    assert!(strike_tok.ends_with("\x1b[29m"));
}

#[test]
fn test_encode_decode_sgr() {
    let raw = "\x1b[38;5;231mReview: mirage\x1b[39m";
    let encoded = encode_sgr(raw);
    assert_eq!(encoded, "{38;5;231}Review: mirage{39}");
    let decoded = decode_sgr(&encoded);
    assert_eq!(decoded, raw);
}
