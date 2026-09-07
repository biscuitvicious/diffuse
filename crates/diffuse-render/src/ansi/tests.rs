use super::*;

#[test]
fn test_ansi_color_formatting() {
    assert_eq!(AnsiColor::Reset.fg_code(), "\x1b[39m");
    assert_eq!(AnsiColor::Reset.bg_code(), "\x1b[49m");

    assert_eq!(AnsiColor::Index(120).fg_code(), "\x1b[38;5;120m");
    assert_eq!(AnsiColor::Index(28).bg_code(), "\x1b[48;5;28m");

    assert_eq!(
        AnsiColor::Rgb(255, 128, 64).fg_code(),
        "\x1b[38;2;255;128;64m"
    );
    assert_eq!(AnsiColor::Rgb(10, 20, 30).bg_code(), "\x1b[48;2;10;20;30m");
}

#[test]
fn test_sgr_modifiers_and_reset() {
    assert_eq!(sgr_on(1), "\x1b[1m");
    assert_eq!(sgr_off(1), Some("\x1b[22m"));

    assert_eq!(sgr_on(3), "\x1b[3m");
    assert_eq!(sgr_off(3), Some("\x1b[23m"));

    assert_eq!(sgr_on(4), "\x1b[4m");
    assert_eq!(sgr_off(4), Some("\x1b[24m"));

    assert_eq!(sgr_on(9), "\x1b[9m");
    assert_eq!(sgr_off(9), Some("\x1b[29m"));

    assert_eq!(sgr_off(99), None);
}

#[test]
fn test_strip_ansi() {
    let styled = "\x1b[1m\x1b[38;5;120m+fn main() {\x1b[39m\x1b[22m";
    assert_eq!(strip_ansi(styled), "+fn main() {");

    let plain = "plain text without escapes";
    assert_eq!(strip_ansi(plain), "plain text without escapes");
}

#[test]
fn test_ansi_style_render() {
    let style = AnsiStyle {
        fg: Some(AnsiColor::Index(120)),
        bg: Some(AnsiColor::Index(22)),
        bold: true,
        italic: false,
        underline: false,
        strikethrough: false,
    };

    let rendered = style.render("added line");
    assert!(rendered.starts_with("\x1b[1m"));
    assert!(rendered.contains("\x1b[38;5;120m"));
    assert!(rendered.contains("\x1b[48;5;22m"));
    assert!(rendered.contains("added line"));
    assert!(rendered.ends_with("\x1b[0m"));
}

#[test]
fn test_encode_sgr_markup() {
    let input = "\x1b[38;5;120mhello\x1b[39m";
    let encoded = encode_sgr(input);
    assert_eq!(encoded, "{38;5;120}hello{39}");

    let decoded = decode_sgr(&encoded);
    assert_eq!(decoded, input);
}

#[test]
fn test_yansi_color_roundtrip() {
    assert_eq!(yansi::Color::from(AnsiColor::Reset), yansi::Color::Primary);
    assert_eq!(AnsiColor::from(yansi::Color::Primary), AnsiColor::Reset);

    assert_eq!(
        yansi::Color::from(AnsiColor::Index(120)),
        yansi::Color::Fixed(120)
    );
    assert_eq!(
        AnsiColor::from(yansi::Color::Fixed(120)),
        AnsiColor::Index(120)
    );

    assert_eq!(
        yansi::Color::from(AnsiColor::Rgb(10, 20, 30)),
        yansi::Color::Rgb(10, 20, 30)
    );
    assert_eq!(
        AnsiColor::from(yansi::Color::Rgb(10, 20, 30)),
        AnsiColor::Rgb(10, 20, 30)
    );
}

#[test]
fn test_yansi_conditions() {
    yansi::whenever(Condition::ALWAYS);
    assert!(yansi::is_enabled());
    yansi::whenever(Condition::NEVER);
    assert!(!yansi::is_enabled());
    yansi::whenever(Condition::DEFAULT);

    yansi::enable();
    assert!(yansi::is_enabled());
    yansi::disable();
    assert!(!yansi::is_enabled());

    let style = yansi::Style::new().fg(yansi::Color::Red).bold();
    assert_eq!(format!("{}", "hello".paint(style)), "hello");

    yansi::whenever(Condition::ALWAYS);
    assert!(yansi::is_enabled());
    assert_ne!(format!("{}", style.prefix()), "");
    assert_ne!(format!("{}", "hello".paint(style)), "hello");

    yansi::whenever(Condition::DEFAULT);
}
