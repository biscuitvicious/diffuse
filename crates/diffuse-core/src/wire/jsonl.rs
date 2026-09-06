//! Streaming JSONL Protocol & Record Serialization.
//!
//! Provides granular record types (`FileHeader`, `HunkHeader`, `Line`,
//! `FileFooter`) for low-latency line-by-line streaming, streaming
//! verification, and progressive wire deserialization.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::{
    counts::FileCounts,
    wire::{OutputFence, OutputFile, OutputHunk, OutputNote, OutputRow},
};

/// A single JSONL streaming record line.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum WireRecord {
    /// Beginning of a file diff.
    FileHeader(FileHeader),
    /// Beginning of a diff hunk.
    HunkHeader(HunkHeader),
    /// A single line/row within a hunk.
    Line(OutputRow),
    /// End of a file diff with aggregate metrics and notes.
    FileFooter(FileFooter),
}

/// Metadata header for a file diff in the JSONL stream.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHeader {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_binary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bytes: Option<usize>,

    pub new_line_count: usize,
    pub old_line_count: usize,
    pub hunk_count: usize,
}

/// Header for a diff hunk with coordinate ranges and expected row count.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkHeader {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub row_count: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section: String,
}

/// Footer terminating a file diff, containing final file counts, fences, and
/// notes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFooter {
    pub path: String,
    pub counts: FileCounts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fences: Vec<OutputFence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<OutputNote>,
}

/// Emits a single `OutputFile` as a sequence of line-by-line JSONL records.
pub fn emit_file_jsonl<W: Write>(file: &OutputFile, writer: &mut W) -> std::io::Result<()> {
    let header = WireRecord::FileHeader(FileHeader {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        old_mode: file.old_mode.clone(),
        new_mode: file.new_mode.clone(),
        status: file.status.clone(),
        is_binary: file.is_binary,
        old_bytes: file.old_bytes,
        new_bytes: file.new_bytes,
        new_line_count: file.new_line_count,
        old_line_count: file.old_line_count,
        hunk_count: file.hunks.len(),
    });
    serde_json::to_writer(&mut *writer, &header)?;
    writer.write_all(b"\n")?;

    for hunk in &file.hunks {
        let hunk_header = WireRecord::HunkHeader(HunkHeader {
            old_start: hunk.old_start,
            old_count: hunk.old_count,
            new_start: hunk.new_start,
            new_count: hunk.new_count,
            row_count: hunk.rows.len(),
            section: String::new(),
        });
        serde_json::to_writer(&mut *writer, &hunk_header)?;
        writer.write_all(b"\n")?;

        for row in &hunk.rows {
            let line_rec = WireRecord::Line(row.clone());
            serde_json::to_writer(&mut *writer, &line_rec)?;
            writer.write_all(b"\n")?;
        }
    }

    let footer = WireRecord::FileFooter(FileFooter {
        path: file.path.clone(),
        counts: file.counts,
        fences: file.fences.clone(),
        notes: file.notes.clone(),
    });
    serde_json::to_writer(&mut *writer, &footer)?;
    writer.write_all(b"\n")?;

    Ok(())
}

/// Decodes an iterator of `WireRecord`s back into full `OutputFile`
/// representations.
pub fn decode_jsonl_records<I: IntoIterator<Item = WireRecord>>(
    records: I,
) -> Result<Vec<OutputFile>, String> {
    let mut files = Vec::new();
    let mut current_file: Option<OutputFile> = None;
    let mut current_hunk: Option<OutputHunk> = None;

    for record in records {
        match record {
            WireRecord::FileHeader(h) => {
                if let Some(f) = current_file.take() {
                    files.push(f);
                }
                current_file = Some(OutputFile {
                    path: h.path,
                    old_path: h.old_path,
                    old_mode: h.old_mode,
                    new_mode: h.new_mode,
                    status: h.status,
                    is_binary: h.is_binary,
                    old_bytes: h.old_bytes,
                    new_bytes: h.new_bytes,
                    new_line_count: h.new_line_count,
                    old_line_count: h.old_line_count,
                    new_content: vec![],
                    old_content: vec![],
                    hunks: Vec::with_capacity(h.hunk_count),
                    raw_hunks: vec![],
                    counts: FileCounts::default(),
                    fences: vec![],
                    notes: vec![],
                    content_runs: vec![],
                    rows: vec![],
                });
            }
            WireRecord::HunkHeader(hh) => {
                let file = current_file
                    .as_mut()
                    .ok_or_else(|| "HunkHeader encountered without prior FileHeader".to_string())?;
                if let Some(h) = current_hunk.take() {
                    file.hunks.push(h);
                }
                current_hunk = Some(OutputHunk {
                    old_start: hh.old_start,
                    old_count: hh.old_count,
                    new_start: hh.new_start,
                    new_count: hh.new_count,
                    rows: Vec::with_capacity(hh.row_count),
                });
            }
            WireRecord::Line(row) => {
                let hunk = current_hunk
                    .as_mut()
                    .ok_or_else(|| "Line encountered without prior HunkHeader".to_string())?;
                hunk.rows.push(row);
            }
            WireRecord::FileFooter(f) => {
                let mut file = current_file
                    .take()
                    .ok_or_else(|| "FileFooter encountered without prior FileHeader".to_string())?;
                if let Some(h) = current_hunk.take() {
                    file.hunks.push(h);
                }
                if file.path != f.path {
                    return Err(format!(
                        "FileFooter path {:?} does not match FileHeader path {:?}",
                        f.path, file.path
                    ));
                }
                file.counts = f.counts;
                file.fences = f.fences;
                file.notes = f.notes;
                files.push(file);
            }
        }
    }

    if let Some(mut f) = current_file {
        if let Some(h) = current_hunk {
            f.hunks.push(h);
        }
        files.push(f);
    }

    Ok(files)
}

/// Decodes JSONL from any `BufRead` source into `Vec<OutputFile>`.
pub fn decode_jsonl_reader<R: BufRead>(mut reader: R) -> std::io::Result<Vec<OutputFile>> {
    let mut line_buf = String::new();
    let mut records = Vec::new();

    while reader.read_line(&mut line_buf)? > 0 {
        let trimmed = line_buf.trim();
        if !trimmed.is_empty() {
            let record: WireRecord = serde_json::from_str(trimmed)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            records.push(record);
        }
        line_buf.clear();
    }

    decode_jsonl_records(records)
        .map_err(|err_msg| std::io::Error::new(std::io::ErrorKind::InvalidData, err_msg))
}

/// Decodes a full JSONL string into `Vec<OutputFile>`.
pub fn decode_jsonl_str(s: &str) -> std::io::Result<Vec<OutputFile>> {
    decode_jsonl_reader(s.as_bytes())
}

#[cfg(test)]
mod tests;
