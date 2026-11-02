use diffuse_parse::{DiffFile, FileStatus};

use super::{hunk::diff_buffers, line::WhitespaceMode};

/// Parameters for generating a unified diff and AST for a single file.
#[derive(Debug, Clone)]
pub struct FileDiffParams<'a> {
    pub path: &'a str,
    pub old_path: Option<&'a str>,
    pub old_bytes: Option<&'a [u8]>,
    pub new_bytes: Option<&'a [u8]>,
    pub status: FileStatus,
    pub old_mode: Option<&'a str>,
    pub new_mode: Option<&'a str>,
    pub is_binary: bool,
    pub context: u32,
    pub mode: WhitespaceMode,
}

/// The result of diffing a single file: parsed AST and raw patch text.
#[derive(Debug, Clone)]
pub struct FileDiffResult {
    pub file: DiffFile,
    pub patch: String,
}

/// Generate structured [`DiffFile`] AST and unified diff patch
/// text for a single file.
pub fn generate_file_diff(params: FileDiffParams<'_>) -> FileDiffResult {
    let effective_old = params.old_path.unwrap_or(params.path);
    let mut patch = format!("diff --git a/{effective_old} b/{}\n", params.path);

    match params.status {
        FileStatus::Deleted => {
            patch.push_str(&format!(
                "deleted file mode {}\n",
                params.old_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- a/{effective_old}\n+++ /dev/null\n"));
        }
        FileStatus::Added => {
            patch.push_str(&format!(
                "new file mode {}\n",
                params.new_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- /dev/null\n+++ b/{}\n", params.path));
        }
        FileStatus::Untracked => {
            patch.push_str(&format!(
                "untracked file mode {}\n",
                params.new_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- /dev/null\n+++ b/{}\n", params.path));
        }
        FileStatus::Renamed => {
            patch.push_str(&format!(
                "rename from {effective_old}\nrename to {}\n",
                params.path
            ));
            patch.push_str(&format!("--- a/{effective_old}\n+++ b/{}\n", params.path));
        }
        FileStatus::Modified | FileStatus::ModeChanged | FileStatus::Unmodified => {
            if let (Some(old_m), Some(new_m)) = (params.old_mode, params.new_mode)
                && old_m != new_m
            {
                patch.push_str(&format!("old mode {old_m}\nnew mode {new_m}\n"));
            }
            patch.push_str(&format!("--- a/{effective_old}\n+++ b/{}\n", params.path));
        }
    }

    if params.is_binary {
        patch.push_str(&format!(
            "Binary files a/{effective_old} and b/{} differ\n",
            params.path
        ));
        let mut file = DiffFile::new(params.path);
        file.old_path = params.old_path.map(ToString::to_string);
        file.status = params.status;
        file.old_mode = params.old_mode.map(ToString::to_string);
        file.new_mode = params.new_mode.map(ToString::to_string);
        file.is_binary = true;
        file.has_file_headers = true;
        file.old_bytes = params.old_bytes.map(|b| b.len());
        file.new_bytes = params.new_bytes.map(|b| b.len());
        return FileDiffResult { file, patch };
    }

    let (old_buf, new_buf): (&[u8], &[u8]) = match params.status {
        FileStatus::Added | FileStatus::Untracked => (b"", params.new_bytes.unwrap_or(b"")),
        FileStatus::Deleted => (params.old_bytes.unwrap_or(b""), b""),
        _ => (
            params.old_bytes.unwrap_or(b""),
            params.new_bytes.unwrap_or(b""),
        ),
    };

    let (hunks, hunk_patch) = diff_buffers(old_buf, new_buf, params.context, params.mode);
    patch.push_str(&hunk_patch);

    let mut file = DiffFile::new(params.path);
    file.old_path = params.old_path.map(ToString::to_string);
    file.status = params.status;
    file.old_mode = params.old_mode.map(ToString::to_string);
    file.new_mode = params.new_mode.map(ToString::to_string);
    file.is_binary = false;
    file.has_file_headers = true;
    file.old_bytes = params.old_bytes.map(|b| b.len());
    file.new_bytes = params.new_bytes.map(|b| b.len());
    file.raw_hunks = hunks.clone();
    file.hunks = hunks;

    FileDiffResult { file, patch }
}
