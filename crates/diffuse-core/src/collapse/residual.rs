//! Declaration-role compatibility and verbatim-rival predicates shared by
//! pair evidence scoring and slot admissibility.

/// Identifies the declaration role of a code line, stripping common visibility
/// prefixes.
pub(crate) fn declaration_role(line: &str) -> Option<&'static str> {
    let mut words = line.split_whitespace();
    let mut first = words.next()?;
    if matches!(
        first,
        "pub"
            | "pub(crate)"
            | "pub(super)"
            | "export"
            | "declare"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "async"
    ) {
        first = words.next()?;
    }
    match first {
        "struct" => Some("struct"),
        "enum" => Some("enum"),
        "union" => Some("union"),
        "trait" => Some("trait"),
        "interface" => Some("interface"),
        "type" => Some("type"),
        "class" => Some("class"),
        "fn" | "def" | "function" => Some("fn"),
        "const" | "static" => Some("const"),
        _ => None,
    }
}

/// Checks if a trimmed line text appears verbatim anywhere in the snapshot
/// content.
pub(crate) fn has_verbatim_rival(text: &str, content: Option<&[&str]>) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    content.is_some_and(|lines| lines.iter().any(|l| l.trim() == trimmed))
}
