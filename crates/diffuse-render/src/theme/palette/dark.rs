use yansi::Color;

use super::ThemePalette;

/// Default border color shared by every `*bo`/`*_border` palette key on dark
/// backgrounds.
pub const DEFAULT_BORDER_COLOR_DARK: Color = Color::Fixed(243);

/// Default Theme Palette with deeper background and muted red/green shades.
#[must_use]
pub const fn default_palette() -> ThemePalette {
    ThemePalette {
        bg_base: Color::Fixed(233),
        bg_modal: Color::Fixed(234),
        warning: Color::Fixed(220),
        note: Color::Fixed(220),
        accent: Color::Fixed(51),

        ctx_tint_bg: Color::Fixed(233),
        ctx_cursor_bg: Color::Fixed(237),
        ctx_fg: Color::Primary,
        band_bg: Color::Fixed(235),

        add_tint_bg: Color::Rgb(28, 50, 30),
        add_cursor_bg: Color::Rgb(46, 74, 48),
        add_fill_bg: Color::Rgb(30, 96, 50),
        add_fill_cursor_bg: Color::Rgb(37, 120, 65),
        add_fg: Color::Fixed(120),

        del_tint_bg: Color::Rgb(38, 12, 14),
        del_cursor_bg: Color::Rgb(68, 24, 26),
        del_fill_bg: Color::Rgb(82, 28, 32),
        del_fill_cursor_bg: Color::Rgb(100, 34, 38),
        del_fg: Color::Fixed(203),

        mod_tint_bg: Color::Rgb(38, 38, 38),
        mod_cursor_bg: Color::Rgb(66, 66, 66),
        mod_fill_bg: Color::Rgb(30, 96, 50),
        mod_fill_cursor_bg: Color::Rgb(37, 120, 65),
        mod_fg: Color::Fixed(245),

        mov_tint_bg: Color::Fixed(233),
        mov_cursor_bg: Color::Fixed(237),
        mov_fg: Color::Fixed(245),

        note_tint_bg: Color::Rgb(76, 64, 20),

        ctx_gutter_in_hunk: Color::Fixed(245),
        ctx_gutter_out_hunk: Color::Fixed(238),
        del_gutter_num: Color::Fixed(203),
        del_gutter_icon: Color::Fixed(203),
        add_gutter_num: Color::Fixed(120),
        add_gutter_icon: Color::Fixed(120),
        mod_gutter_num: Color::Fixed(214),
        mod_gutter_icon: Color::Fixed(214),
        mov_gutter_num: Color::Fixed(147),
        mov_gutter_icon: Color::Fixed(117),

        file_header_bg_selected: Color::Fixed(238),
        file_header_bg_current: Color::Fixed(235),
        file_header_bg_rest: Color::Fixed(233),
        file_header_fg_selected: Color::Fixed(221),
        file_header_fg_current: Color::Fixed(220),
        file_header_fg_rest: Color::Fixed(214),
        file_header_path_selected: Color::Fixed(255),
        file_header_path_current: Color::Fixed(254),
        file_header_path_rest: Color::Fixed(251),

        hunk_divider_bg_selected: Color::Fixed(238),
        hunk_divider_bg_current: Color::Fixed(235),
        hunk_divider_bg_rest: Color::Primary,
        hunk_divider_fg: Color::Fixed(213),
        hunk_divider_fg_selected: Color::Rgb(255, 155, 255),

        header_bar_bg: Color::Fixed(234),
        header_bar_fg: Color::Fixed(231),

        scrollbar_thumb: Color::Fixed(244),
        scrollbar_track: Color::Fixed(233),

        keyword: Color::Fixed(147),
        typ: Color::Fixed(220),
        function: Color::Fixed(123),
        number: Color::Fixed(221),
        literal: Color::Fixed(221),
        string: Color::Fixed(120),
        add_string: Color::Fixed(120),
        del_string: Color::Fixed(203),
        char_literal: Color::Fixed(158),
        add_char_literal: Color::Fixed(158),
        del_char_literal: Color::Fixed(210),
        comment: Color::Fixed(150),
        operator: Color::Fixed(117),

        bold: Color::Fixed(214),
        italic: Color::Fixed(229),
        strike: Color::Fixed(250),
        link: Color::Fixed(117),
        url: Color::Fixed(250),
        delim: Color::Fixed(51),
        rule: Color::Fixed(220),
        code: Color::Fixed(81),
        table_border: DEFAULT_BORDER_COLOR_DARK,
        table_header: Color::Fixed(229),
        link_border: DEFAULT_BORDER_COLOR_DARK,
        quote_border: DEFAULT_BORDER_COLOR_DARK,
        fence_border: DEFAULT_BORDER_COLOR_DARK,

        // UI & Status Bar
        status_bar_bg: Color::Fixed(234),
        status_bar_fg: Color::Fixed(252),
        status_bar_dim: Color::Fixed(244),
        status_bar_accent: Color::Fixed(220),
        ribbon_divider: Color::Fixed(244),
        ribbon_text: Color::Fixed(244),

        // Header Bar Elements
        header_repo: Color::Fixed(117),
        header_branch: Color::Fixed(252),
        header_dim: Color::Fixed(244),
        header_staged: Color::Fixed(120),
        header_prefix: Color::Fixed(152),

        // Modals & Overlays
        modal_bg: Color::Fixed(236),
        modal_border: Color::Fixed(240),
        modal_title: Color::Fixed(231),
        modal_text: Color::Fixed(250),
        modal_dim: Color::Fixed(242),
        modal_selected_bg: Color::Fixed(239),
        modal_selected_fg: Color::Fixed(248),

        // Count Highlights (theme-specific)
        count_dels: Color::Fixed(203),
        count_adds: Color::Fixed(120),
        count_mods: Color::Fixed(214),
        count_movs: Color::Fixed(147),
        count_notes: Color::Fixed(220),

        // Syntax Overrides for Added lines
        add_keyword: Color::Fixed(147),
        add_typ: Color::Fixed(220),
        add_function: Color::Fixed(123),
        add_number: Color::Fixed(221),
        add_literal: Color::Fixed(221),
        add_comment: Color::Fixed(150),
        add_operator: Color::Fixed(117),
        add_bold: Color::Fixed(214),
        add_italic: Color::Fixed(229),
        add_link: Color::Fixed(117),
        add_url: Color::Fixed(250),
        add_code: Color::Fixed(81),
        add_table_border: DEFAULT_BORDER_COLOR_DARK,
        add_table_header: Color::Fixed(229),
        add_link_border: DEFAULT_BORDER_COLOR_DARK,
        add_quote_border: DEFAULT_BORDER_COLOR_DARK,
        add_fence_border: DEFAULT_BORDER_COLOR_DARK,
        add_delim: Color::Fixed(51),

        // Syntax Overrides for Deleted lines
        del_keyword: Color::Fixed(183),
        del_typ: Color::Fixed(215),
        del_function: Color::Fixed(159),
        del_number: Color::Fixed(222),
        del_literal: Color::Fixed(222),
        del_comment: Color::Fixed(224),
        del_operator: Color::Fixed(153),
        del_text: Color::Primary,
        del_bold: Color::Fixed(222),
        del_italic: Color::Fixed(224),
        del_link: Color::Fixed(153),
        del_url: Color::Fixed(248),
        del_code: Color::Fixed(203),
        del_table_border: DEFAULT_BORDER_COLOR_DARK,
        del_table_header: Color::Fixed(224),
        del_link_border: DEFAULT_BORDER_COLOR_DARK,
        del_quote_border: DEFAULT_BORDER_COLOR_DARK,
        del_fence_border: DEFAULT_BORDER_COLOR_DARK,
        del_delim: Color::Fixed(51),
        text: Color::Primary,
        add_text: Color::Primary,
    }
}
