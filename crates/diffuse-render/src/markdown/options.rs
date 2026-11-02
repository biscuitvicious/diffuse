use crate::theme::DiffuseTheme;

/// Formatting and layout options for Markdown diff export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Whether to generate the overview summary metrics and table at the top.
    pub summary_table: bool,
    /// Wrap each file's diff inside collapsible `<details><summary>` blocks.
    pub collapsible: bool,
    /// Whether `<details>` tags should be initially open (`<details open>`).
    pub default_open: bool,
    /// Render attached review notes as GitHub blockquote callouts (`>
    /// [!NOTE]`).
    pub include_notes: bool,
    /// Include resolved review notes in the markdown output (default: show
    /// active only).
    pub show_resolved_notes: bool,
    /// Render original source rows under review notes as a nested diff code
    /// block.
    pub show_source: bool,
    /// Whether to colorize the Markdown output with ANSI escape sequences.
    pub colorize: bool,
    /// Theme used for token colors when `colorize` is enabled.
    pub theme: DiffuseTheme,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            summary_table: true,
            collapsible: true,
            default_open: true,
            include_notes: true,
            show_resolved_notes: false,
            show_source: false,
            colorize: false,
            theme: DiffuseTheme::DEFAULT,
        }
    }
}
