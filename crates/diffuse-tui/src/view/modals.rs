use crate::{core::wire::PipelineOutput, finder::FileFinderState};

/// Render the `?` key-bindings modal as full-width overlay lines.
pub fn render_help_modal(width: usize) -> Vec<String> {
    let entries: &[(&str, &str)] = &[
        ("j / k / ↑ / ↓", "move"),
        ("J / K / } / {", "prev / next hunk"),
        ("Space · PgDn/PgUp · Ctrl+d f u b", "page up/down"),
        ("g / G / Home / End", "top / bottom"),
        ("n / p / ] / [", "next / prev file"),
        ("@", "find file in diff"),
        ("h / l / ← / →", "horizontal scroll"),
        ("w", "toggle word diff"),
        ("y", "toggle syntax highlighting"),
        ("R", "toggle raw unified-diff view"),
        ("a / +", "expand all hidden context lines"),
        ("c / C", "reset hunk / all context to default"),
        ("m", "new note (ctrl+enter save, esc cancel)"),
        ("?", "toggle help"),
        ("q / Ctrl+c", "quit"),
    ];
    let mut boxed = vec![String::from(" KEY BINDINGS ")];
    for (key, desc) in entries {
        boxed.push(format!("   {key:<32} {desc}"));
    }
    boxed.push(String::from("   ? or Esc to close"));
    let w = width.saturating_sub(2).min(64);
    boxed
        .into_iter()
        .map(|l| format!("\x1b[48;5;236m\x1b[38;5;255m{:<w$}\x1b[0m", l))
        .collect()
}

/// Render the fuzzy file finder popup dialog.
pub fn render_finder_modal(
    output: &PipelineOutput,
    finder: &FileFinderState,
    width: usize,
) -> Vec<String> {
    let w = width.saturating_sub(4).clamp(36, 76);
    let mut lines = Vec::new();

    let prompt = format!(" @ {}█", finder.query);
    lines.push(format!(
        "\x1b[48;5;238m\x1b[1m\x1b[38;5;231m {:<w$}\x1b[0m",
        prompt,
        w = w.saturating_sub(1)
    ));

    let header_info = format!(
        " FIND FILE ({}/{}) · ↑/↓ nav · Enter jump · Esc close",
        if finder.matches.is_empty() {
            0
        } else {
            finder.selected_idx + 1
        },
        finder.matches.len()
    );
    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;244m {:<w$}\x1b[0m",
        header_info,
        w = w.saturating_sub(1)
    ));

    if finder.matches.is_empty() {
        lines.push(format!(
            "\x1b[48;5;235m\x1b[38;5;240m {:<w$}\x1b[0m",
            "  No matching files in diff",
            w = w.saturating_sub(1)
        ));
    } else {
        let max_visible = 8;
        let total = finder.matches.len();
        let start = if total <= max_visible || finder.selected_idx < max_visible / 2 {
            0
        } else if finder.selected_idx + max_visible / 2 >= total {
            total - max_visible
        } else {
            finder.selected_idx - max_visible / 2
        };
        let end = (start + max_visible).min(total);

        for (i, m) in finder.matches[start..end].iter().enumerate() {
            let actual_idx = start + i;
            let is_sel = actual_idx == finder.selected_idx;
            let f = &output.files[m.file_idx];
            let prefix = if is_sel { "▶ " } else { "  " };
            let stats = format!("+{} -{} ~{}", f.counts.adds, f.counts.dels, f.counts.mods);

            let path_display = highlight_path_matches(&m.path, &m.match_indices, is_sel);
            let bg_code = if is_sel {
                "\x1b[48;5;239m"
            } else {
                "\x1b[48;5;235m"
            };
            let stats_color = if is_sel {
                "\x1b[38;5;248m"
            } else {
                "\x1b[38;5;242m"
            };
            let prefix_color = if is_sel {
                "\x1b[1m\x1b[38;5;120m"
            } else {
                "\x1b[38;5;244m"
            };

            lines.push(format!(
                "{bg_code}{prefix_color}{prefix}\x1b[0m{bg_code}{path_display} {stats_color}{stats:<8}\x1b[0m"
            ));
        }
    }

    lines
}

fn highlight_path_matches(path: &str, indices: &[usize], selected: bool) -> String {
    let mut out = String::new();
    let default_color = if selected {
        "\x1b[1m\x1b[38;5;231m"
    } else {
        "\x1b[38;5;250m"
    };
    let match_color = "\x1b[1m\x1b[38;5;51m";

    for (i, ch) in path.char_indices() {
        if indices.contains(&i) {
            out.push_str(match_color);
        } else {
            out.push_str(default_color);
        }
        out.push(ch);
    }
    out.push_str("\x1b[0m");
    out
}
