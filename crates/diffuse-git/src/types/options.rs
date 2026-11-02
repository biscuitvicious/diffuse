//! Git diff options and blob object models.

/// Options configuring git diff generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffOptions {
    /// Number of context lines surrounding changes.
    pub context_lines: usize,
    /// Whether to detect renames and copies.
    pub find_renames: bool,
    /// Similarity threshold for rename detection (0..=100).
    pub rename_threshold: u32,
    /// Whether to ignore indentation changes (exact trim-equality diffing).
    pub ignore_indentation: bool,
}

impl Default for GitDiffOptions {
    fn default() -> Self {
        Self {
            context_lines: 4,
            find_renames: true,
            rename_threshold: 50,
            ignore_indentation: false,
        }
    }
}

/// A git blob retrieved from the object database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlob {
    /// Object ID (hex string).
    pub id: String,
    /// Raw blob bytes.
    pub data: Vec<u8>,
    /// Whether the blob is considered binary by gitattributes, entry mode, or
    /// content inspection.
    pub is_binary: bool,
}

impl GitBlob {
    /// Construct a new blob.
    pub fn new(id: impl Into<String>, data: Vec<u8>, is_binary: bool) -> Self {
        Self {
            id: id.into(),
            data,
            is_binary,
        }
    }

    /// View blob data as UTF-8 string if valid and not binary.
    pub fn as_str(&self) -> Option<&str> {
        if self.is_binary {
            None
        } else {
            std::str::from_utf8(&self.data).ok()
        }
    }

    /// Size of blob data in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Whether this blob is binary.
    pub fn is_binary(&self) -> bool {
        self.is_binary
    }
}
