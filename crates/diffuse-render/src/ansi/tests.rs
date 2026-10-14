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

    assert_eq!(sgr_on(9), "\x1b[2;9m");
    assert_eq!(sgr_off(9), Some("\x1b[29m\x1b[22m"));

    assert_eq!(sgr_on(14), "\x1b[1;4m");
    assert_eq!(sgr_off(14), Some("\x1b[24;22m"));

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

#[test]
fn test_optimize_ansi_drops_repeated_bg() {
    // The tripled row background from the goldens: second assertion is a
    // no-op and must go; the color change stays.
    let line = "\x1b[48;2;38;38;38m    assert_eq\x1b[48;2;38;38;38m\x1b[38;5;147mlet\x1b[39m\x1b[48;2;38;38;38m rest";
    assert_eq!(
        optimize_ansi_line(line).as_ref(),
        "\x1b[48;2;38;38;38m    assert_eq\x1b[38;5;147mlet\x1b[39m rest"
    );
}

#[test]
fn test_optimize_ansi_keeps_state_changes() {
    let line = "\x1b[1mBold\x1b[22m plain \x1b[9mstruck\x1b[29m done";
    assert_eq!(optimize_ansi_line(line).as_ref(), line);
}

#[test]
fn test_optimize_ansi_drops_noop_resets() {
    // FG already default: both the stray {39} and the trailing {0} are
    // no-ops. (Trailing reset with live style is kept — see below.)
    assert_eq!(optimize_ansi_line("plain\x1b[39m").as_ref(), "plain");
    assert_eq!(optimize_ansi_line("plain\x1b[0m").as_ref(), "plain");
    assert_eq!(
        optimize_ansi_line("\x1b[38;5;120mx\x1b[39m\x1b[0m").as_ref(),
        "\x1b[38;5;120mx\x1b[39m"
    );
}

#[test]
fn test_optimize_ansi_never_adds_bytes() {
    // Pure deletion machine: no trailing reset is synthesized even with
    // live style — output is always a subsequence of input. (Our emitters
    // terminate every line explicitly, so no leak arises in practice.)
    assert_eq!(
        optimize_ansi_line("\x1b[38;5;120mabc").as_ref(),
        "\x1b[38;5;120mabc"
    );
}

#[test]
fn test_optimize_ansi_tracks_dim_separate_from_bold() {
    // 22 clears both bold and dim: the second 22 is redundant, and bold
    // set while dim is live must still emit.
    assert_eq!(
        optimize_ansi_line("\x1b[2mdim\x1b[22m\x1b[22m").as_ref(),
        "\x1b[2mdim\x1b[22m"
    );
    assert_eq!(
        optimize_ansi_line("\x1b[2m\x1b[1mboth\x1b[22m").as_ref(),
        "\x1b[2m\x1b[1mboth\x1b[22m"
    );
}

#[test]
fn test_optimize_ansi_basic_and_extended_color_equivalence() {
    // 31 and 38;5;1 address the same cell color: the repeat drops.
    assert_eq!(
        optimize_ansi_line("\x1b[38;5;1m\x1b[31mred").as_ref(),
        "\x1b[38;5;1mred"
    );
    assert_eq!(
        optimize_ansi_line("\x1b[41m\x1b[48;5;1mbg").as_ref(),
        "\x1b[41mbg"
    );
}

#[test]
fn test_optimize_ansi_unknown_codes_force_conservative_mode() {
    // 7 (reverse video) is outside the model: keep it, and keep
    // everything after it until a full reset re-establishes knowledge.
    assert_eq!(
        optimize_ansi_line("\x1b[7m\x1b[31m\x1b[31m\x1b[0m\x1b[31mok").as_ref(),
        "\x1b[7m\x1b[31m\x1b[31m\x1b[0m\x1b[31mok"
    );
}

#[test]
fn test_optimize_ansi_passes_through_non_sgr() {
    // Erase-line and friends are not SGR: untouched, state undisturbed.
    assert_eq!(
        optimize_ansi_line("ab\x1b[K\x1b[31mcd").as_ref(),
        "ab\x1b[K\x1b[31mcd"
    );
    assert_eq!(optimize_ansi_line("").as_ref(), "");
    assert_eq!(optimize_ansi_line("plain text").as_ref(), "plain text");
}

#[test]
fn test_optimize_ansi_fused_and_empty_reset() {
    // Fused italic+color kept whole; bare ESC[m resets.
    assert_eq!(
        optimize_ansi_line("\x1b[3;38;5;203mx\x1b[23m\x1b[39m").as_ref(),
        "\x1b[3;38;5;203mx\x1b[23m\x1b[39m"
    );
    assert_eq!(optimize_ansi_line("x\x1b[m").as_ref(), "x");
}

#[test]
fn test_optimize_ansi_multiline_and_fast_paths() {
    // No escapes at all: borrowed, byte-identical.
    let plain = "no escapes\nhere\n";
    assert!(matches!(
        optimize_ansi(plain),
        std::borrow::Cow::Borrowed(_)
    ));
    // Mixed lines: only the redundant line is rewritten, endings kept.
    let doc = "plain\n\x1b[48;5;1m\x1b[48;5;1mbg\x1b[0m\ntail";
    assert_eq!(
        optimize_ansi(doc).as_ref(),
        "plain\n\x1b[48;5;1mbg\x1b[0m\ntail"
    );
    // Fully clean input rebuilds identically through the writer core (the
    // Borrowed fast path covers escape-free input only).
    let clean = "\x1b[31mred\x1b[39m\n";
    assert_eq!(optimize_ansi(clean).as_ref(), clean);
}

#[test]
fn test_write_optimized_ansi_line_matches_cow() {
    // The writer core and the Cow convenience wrapper must agree exactly
    // on every input class: kept, dropped, unknown, and plain.
    let inputs = [
        "",
        "plain text",
        "\x1b[48;2;38;38;38m    assert_eq\x1b[48;2;38;38;38m\x1b[38;5;147mlet\x1b[39m rest",
        "\x1b[1mBold\x1b[22m plain \x1b[9mstruck\x1b[29m done\x1b[0m",
        "\x1b[7m\x1b[31m\x1b[31m\x1b[0m\x1b[31mok",
        "ab\x1b[K\x1b[31mcd",
        "\x1b[3;38;5;203mx\x1b[23m\x1b[39m",
        "\x1b[38;5;1m\x1b[31mred",
    ];
    for input in inputs {
        let mut buf = String::new();
        write_optimized_ansi_line(&mut buf, input).expect("String write");
        assert_eq!(
            buf,
            optimize_ansi_line(input).as_ref(),
            "writer/Cow agree for {input:?}"
        );
    }
}
