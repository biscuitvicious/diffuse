use yansi::Color;

use super::ThemePalette;

/// Default border color shared by every `*bo`/`*_border` palette key on light
/// backgrounds. Kept at text contrast (not the paler decorative border) so
/// border runs stay legible on light tints.
pub const DEFAULT_BORDER_COLOR_LIGHT: Color = Color::Rgb(101, 109, 118);

/// Light Theme Palette calibrated for light terminal backgrounds with
/// high contrast (WCAG AA >= 4.5:1 for all text tokens).
#[must_use]
pub const fn light_palette() -> ThemePalette {
    ThemePalette {
        bg_base: Color::Rgb(250, 250, 250),
        bg_modal: Color::Rgb(255, 255, 255),
        warning: Color::Rgb(180, 80, 0),
        note: Color::Rgb(150, 95, 0),
        accent: Color::Rgb(9, 105, 218),

        ctx_tint_bg: Color::Rgb(250, 250, 250),
        ctx_cursor_bg: Color::Rgb(232, 235, 240),
        ctx_fg: Color::Rgb(36, 41, 47),
        band_bg: Color::Rgb(243, 245, 248),

        add_tint_bg: Color::Rgb(230, 246, 236),
        add_cursor_bg: Color::Rgb(205, 238, 215),
        add_fill_bg: Color::Rgb(172, 224, 186),
        add_fill_cursor_bg: Color::Rgb(145, 210, 162),
        add_fg: Color::Rgb(26, 127, 55),

        del_tint_bg: Color::Rgb(255, 235, 236),
        del_cursor_bg: Color::Rgb(255, 212, 215),
        del_fill_bg: Color::Rgb(255, 178, 184),
        del_fill_cursor_bg: Color::Rgb(255, 150, 158),
        del_fg: Color::Rgb(207, 34, 46),

        mod_tint_bg: Color::Rgb(245, 245, 247),
        mod_cursor_bg: Color::Rgb(230, 232, 236),
        mod_fill_bg: Color::Rgb(172, 224, 186),
        mod_fill_cursor_bg: Color::Rgb(145, 210, 162),
        mod_fg: Color::Rgb(36, 41, 47),

        mov_tint_bg: Color::Rgb(244, 240, 255),
        mov_cursor_bg: Color::Rgb(230, 222, 252),
        mov_fg: Color::Rgb(90, 45, 160),

        note_tint_bg: Color::Rgb(254, 248, 225),

        ctx_gutter_in_hunk: Color::Rgb(101, 109, 118),
        ctx_gutter_out_hunk: Color::Rgb(175, 184, 193),
        del_gutter_num: Color::Rgb(207, 34, 46),
        del_gutter_icon: Color::Rgb(207, 34, 46),
        add_gutter_num: Color::Rgb(26, 127, 55),
        add_gutter_icon: Color::Rgb(26, 127, 55),
        mod_gutter_num: Color::Rgb(170, 90, 0),
        mod_gutter_icon: Color::Rgb(170, 90, 0),
        mov_gutter_num: Color::Rgb(110, 40, 130),
        mov_gutter_icon: Color::Rgb(110, 40, 130),

        file_header_bg_selected: Color::Rgb(215, 228, 250),
        file_header_bg_current: Color::Rgb(228, 236, 248),
        file_header_bg_rest: Color::Rgb(240, 243, 248),
        file_header_fg_selected: Color::Rgb(15, 60, 140),
        file_header_fg_current: Color::Rgb(25, 75, 160),
        file_header_fg_rest: Color::Rgb(60, 80, 110),
        file_header_path_selected: Color::Rgb(5, 25, 65),
        file_header_path_current: Color::Rgb(15, 35, 75),
        file_header_path_rest: Color::Rgb(35, 50, 80),

        hunk_divider_bg_selected: Color::Rgb(225, 225, 238),
        hunk_divider_bg_current: Color::Rgb(238, 238, 246),
        hunk_divider_bg_rest: Color::Primary,
        hunk_divider_fg: Color::Rgb(110, 40, 130),
        hunk_divider_fg_selected: Color::Rgb(140, 45, 165),

        header_bar_bg: Color::Rgb(235, 238, 242),
        header_bar_fg: Color::Rgb(36, 41, 47),

        scrollbar_thumb: Color::Rgb(180, 185, 195),
        scrollbar_track: Color::Rgb(240, 242, 245),

        keyword: Color::Rgb(170, 20, 120),
        typ: Color::Rgb(15, 110, 130),
        function: Color::Rgb(10, 85, 185),
        number: Color::Rgb(165, 75, 0),
        literal: Color::Rgb(165, 75, 0),
        string: Color::Rgb(20, 115, 40),
        add_string: Color::Rgb(20, 115, 40),
        del_string: Color::Rgb(180, 30, 40),
        char_literal: Color::Rgb(16, 128, 67),
        add_char_literal: Color::Rgb(16, 128, 67),
        del_char_literal: Color::Rgb(190, 45, 55),
        comment: Color::Rgb(105, 115, 130),
        operator: Color::Rgb(65, 75, 90),

        bold: Color::Rgb(180, 75, 0),
        italic: Color::Rgb(125, 60, 150),
        strike: Color::Rgb(130, 135, 145),
        link: Color::Rgb(10, 85, 185),
        url: Color::Rgb(100, 105, 115),
        delim: Color::Rgb(15, 110, 130),
        rule: Color::Rgb(185, 190, 200),
        code: Color::Rgb(12, 100, 125),
        table_border: DEFAULT_BORDER_COLOR_LIGHT,
        table_header: Color::Rgb(125, 60, 150),
        link_border: DEFAULT_BORDER_COLOR_LIGHT,
        quote_border: DEFAULT_BORDER_COLOR_LIGHT,
        fence_border: DEFAULT_BORDER_COLOR_LIGHT,

        // UI & Status Bar
        status_bar_bg: Color::Rgb(235, 238, 242),
        status_bar_fg: Color::Rgb(36, 41, 47),
        status_bar_dim: Color::Rgb(105, 115, 130),
        status_bar_accent: Color::Rgb(170, 90, 0),
        ribbon_divider: Color::Rgb(185, 190, 200),
        ribbon_text: Color::Rgb(105, 115, 130),

        // Header Bar Elements
        header_repo: Color::Rgb(10, 85, 185),
        header_branch: Color::Rgb(36, 41, 47),
        header_dim: Color::Rgb(105, 115, 130),
        header_staged: Color::Rgb(26, 127, 55),
        header_prefix: Color::Rgb(15, 110, 130),

        // Modals & Overlays
        modal_bg: Color::Rgb(255, 255, 255),
        modal_border: Color::Rgb(185, 190, 200),
        modal_title: Color::Rgb(36, 41, 47),
        modal_text: Color::Rgb(36, 41, 47),
        modal_dim: Color::Rgb(105, 115, 130),
        modal_selected_bg: Color::Rgb(220, 232, 252),
        modal_selected_fg: Color::Rgb(10, 85, 185),

        // Count Highlights (theme-specific)
        count_dels: Color::Rgb(207, 34, 46),
        count_adds: Color::Rgb(26, 127, 55),
        count_mods: Color::Rgb(170, 90, 0),
        count_movs: Color::Rgb(110, 40, 130),
        count_notes: Color::Rgb(150, 95, 0),

        // Syntax Overrides for Added lines
        add_keyword: Color::Rgb(170, 20, 120),
        add_typ: Color::Rgb(15, 110, 130),
        add_function: Color::Rgb(10, 85, 185),
        add_number: Color::Rgb(165, 75, 0),
        add_literal: Color::Rgb(165, 75, 0),
        add_comment: Color::Rgb(105, 115, 130),
        add_operator: Color::Rgb(65, 75, 90),
        add_bold: Color::Rgb(180, 75, 0),
        add_italic: Color::Rgb(125, 60, 150),
        add_link: Color::Rgb(10, 85, 185),
        add_url: Color::Rgb(100, 105, 115),
        add_code: Color::Rgb(12, 100, 125),
        add_table_border: DEFAULT_BORDER_COLOR_LIGHT,
        add_table_header: Color::Rgb(125, 60, 150),
        add_link_border: DEFAULT_BORDER_COLOR_LIGHT,
        add_quote_border: DEFAULT_BORDER_COLOR_LIGHT,
        add_fence_border: DEFAULT_BORDER_COLOR_LIGHT,
        add_delim: Color::Rgb(15, 110, 130),

        // Syntax Overrides for Deleted lines
        del_keyword: Color::Rgb(170, 20, 120),
        del_typ: Color::Rgb(15, 110, 130),
        del_function: Color::Rgb(10, 85, 185),
        del_number: Color::Rgb(165, 75, 0),
        del_literal: Color::Rgb(165, 75, 0),
        del_comment: Color::Rgb(105, 115, 130),
        del_operator: Color::Rgb(65, 75, 90),
        del_text: Color::Rgb(207, 34, 46),
        del_bold: Color::Rgb(180, 75, 0),
        del_italic: Color::Rgb(125, 60, 150),
        del_link: Color::Rgb(10, 85, 185),
        del_url: Color::Rgb(100, 105, 115),
        del_code: Color::Rgb(180, 30, 40),
        del_table_border: DEFAULT_BORDER_COLOR_LIGHT,
        del_table_header: Color::Rgb(125, 60, 150),
        del_link_border: DEFAULT_BORDER_COLOR_LIGHT,
        del_quote_border: DEFAULT_BORDER_COLOR_LIGHT,
        del_fence_border: DEFAULT_BORDER_COLOR_LIGHT,
        del_delim: Color::Rgb(15, 110, 130),
        text: Color::Rgb(36, 41, 47),
        add_text: Color::Rgb(36, 41, 47),
    }
}
