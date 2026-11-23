//! Structured Run Data Model & Compact Run Serialization.
//!
//! Encodes syntax highlighting and intra-line diff highlights as a
//! sequence of uniform character slices carrying run kind (add/del/ctx),
//! color classification (syntax/theme token), optional markdown SGR attribute
//! modifier, and text.

use std::{borrow::Cow, fmt};

pub use diffuse_syntax::color::TokenColor;
use serde::{Deserialize, Serialize};

use crate::merge::RunKind;

/// An individual colored text run within an [`OutputRow`](super::OutputRow).
///
/// Encodes syntax highlighting and intra-line diff highlights as a
/// sequence of uniform character slices carrying run kind (add/del/ctx),
/// color classification (syntax/theme token), optional markdown SGR attribute
/// modifier, and text.
///
/// Serialized compactly on the wire:
/// - 1 element: `"+text"` for unstyled runs (`TokenColor::Default`, no SGR).
/// - 2 elements: `["+text", "kw"]` for styled runs without SGR.
/// - 3 elements: `["+text", null, 4]` or `["+text", "kw", 1]` when SGR is
///   present.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct OutputRun {
    pub kind: RunKind,
    pub color: TokenColor,
    /// The SGR attribute (the markdown emphasis — bold/italic/strike/
    /// underline) the renderer applies with the color.
    pub sgr: Option<u32>,
    pub text: Cow<'static, str>,
}

impl OutputRun {
    #[inline]
    pub fn new(kind: RunKind, text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            color: TokenColor::Default,
            sgr: None,
            text: text.into(),
        }
    }

    #[inline]
    pub fn sign_char(&self) -> char {
        self.kind.sign_char()
    }

    pub fn reverse(mut self) -> Self {
        self.kind = self.kind.reverse();
        self
    }
}

impl Serialize for OutputRun {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut baked = String::with_capacity(1 + self.text.len());
        baked.push(self.kind.sign_char());
        baked.push_str(&self.text);

        if let Some(sgr) = self.sgr {
            let mut seq = serializer.serialize_seq(Some(3))?;
            seq.serialize_element(&baked)?;
            seq.serialize_element(&self.color)?;
            seq.serialize_element(&sgr)?;
            seq.end()
        } else if !self.color.is_default() {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&baked)?;
            seq.serialize_element(&self.color)?;
            seq.end()
        } else {
            serializer.serialize_str(&baked)
        }
    }
}

impl<'de> Deserialize<'de> for OutputRun {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OutputRunVisitor;

        impl<'de> serde::de::Visitor<'de> for OutputRunVisitor {
            type Value = OutputRun;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a compact run string or an array [baked_text, color, sgr?]")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut chars = v.chars();
                let sign = chars.next().ok_or_else(|| E::custom("empty run string"))?;
                let kind = RunKind::from_sign_char(sign)
                    .ok_or_else(|| E::custom(format!("invalid run sign char '{sign}'")))?;
                let text = Cow::Owned(chars.as_str().to_string());
                Ok(OutputRun {
                    kind,
                    color: TokenColor::Default,
                    sgr: None,
                    text,
                })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                use serde::de::Error;
                let baked: String = seq
                    .next_element()?
                    .ok_or_else(|| Error::custom("missing baked run string"))?;
                let mut chars = baked.chars();
                let sign = chars
                    .next()
                    .ok_or_else(|| Error::custom("empty baked run string"))?;
                let kind = RunKind::from_sign_char(sign)
                    .ok_or_else(|| Error::custom(format!("invalid run sign char '{sign}'")))?;
                let text = Cow::Owned(chars.as_str().to_string());

                let color: TokenColor = seq.next_element()?.unwrap_or_default();
                let sgr_opt: Option<Option<u32>> = seq.next_element()?;
                let sgr = sgr_opt.flatten();

                Ok(OutputRun {
                    kind,
                    color,
                    sgr,
                    text,
                })
            }
        }

        deserializer.deserialize_any(OutputRunVisitor)
    }
}

/// Project the forward/new text from an iterator of [`OutputRun`]s (Ctx +
/// Add; del-like runs — struck or moved-out — are old-side only).
pub fn runs_new_text<'a, I>(runs: I) -> String
where
    I: IntoIterator<Item = &'a OutputRun>,
{
    runs.into_iter()
        .filter(|r| !r.kind.is_del_like())
        .map(|r| r.text.as_ref())
        .collect()
}

/// Project the reverse/old text from an iterator of [`OutputRun`]s (Ctx + Del
/// + moved-out source; new-side-only runs are excluded).
pub fn runs_old_text<'a, I>(runs: I) -> String
where
    I: IntoIterator<Item = &'a OutputRun>,
{
    runs.into_iter()
        .filter(|r| !r.kind.is_add_like())
        .map(|r| r.text.as_ref())
        .collect()
}

/// A single styled syntax token run within a diff batch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyntaxRun {
    pub text: String,
    pub color: TokenColor,
    pub sgr: Option<u32>,
}

impl Serialize for SyntaxRun {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        if let Some(sgr) = self.sgr {
            let mut seq = serializer.serialize_seq(Some(3))?;
            seq.serialize_element(&self.text)?;
            if self.color.is_default() {
                seq.serialize_element(&Option::<&str>::None)?;
            } else {
                seq.serialize_element(&self.color)?;
            }
            seq.serialize_element(&sgr)?;
            seq.end()
        } else if !self.color.is_default() {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&self.text)?;
            seq.serialize_element(&self.color)?;
            seq.end()
        } else {
            serializer.serialize_str(&self.text)
        }
    }
}

/// A batch of syntax token runs sharing the same diff change category.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Batch {
    pub kind: RunKind,
    pub runs: Vec<SyntaxRun>,
}

struct BatchRef<'a> {
    kind: RunKind,
    runs: &'a [SyntaxRun],
}

impl Serialize for BatchRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(1 + self.runs.len()))?;
        let sign = self.kind.sign_char().to_string();
        seq.serialize_element(&sign)?;
        for run in self.runs {
            seq.serialize_element(run)?;
        }
        seq.end()
    }
}

/// Groups contiguous runs by [`RunKind`], strips empty runs, coalesces
/// adjacent runs with identical styling, and absorbs delimiter whitespace into
/// tokens.
pub fn compress_and_batch_runs(runs: &[OutputRun]) -> Vec<Batch> {
    let mut raw_batches: Vec<(RunKind, Vec<&OutputRun>)> = Vec::new();
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        if let Some(last) = raw_batches.last_mut()
            && last.0 == r.kind
        {
            last.1.push(r);
        } else {
            raw_batches.push((r.kind, vec![r]));
        }
    }

    let mut batches = Vec::with_capacity(raw_batches.len());
    for (batch_idx, (kind, group)) in raw_batches.into_iter().enumerate() {
        let mut syntax_runs: Vec<SyntaxRun> = Vec::with_capacity(group.len());
        let is_first_batch = batch_idx == 0;

        for r in group {
            let cur = SyntaxRun {
                text: r.text.to_string(),
                color: r.color,
                sgr: r.sgr,
            };

            if let Some(last) = syntax_runs.last_mut() {
                // Rule 1: Coalesce adjacent runs with identical color and SGR.
                if last.color == cur.color && last.sgr == cur.sgr {
                    last.text.push_str(&cur.text);
                    continue;
                }

                // Rule 2: Absorb trailing whitespace into preceding run if neither has SGR
                // and the preceding run's syntax color allows whitespace absorption.
                if cur.sgr.is_none()
                    && last.sgr.is_none()
                    && cur.text.chars().all(|c| c.is_whitespace())
                    && last.color.allows_trailing_whitespace()
                {
                    last.text.push_str(&cur.text);
                    continue;
                }
            }

            // Rule 3: If this run has no SGR and allows leading whitespace, and the
            // preceding run in this batch was whitespace-only (and not line
            // indentation), absorb the leading whitespace.
            if syntax_runs.len() == 1
                && !is_first_batch
                && cur.sgr.is_none()
                && syntax_runs[0].sgr.is_none()
                && syntax_runs[0].text.chars().all(|c| c.is_whitespace())
                && cur.color.allows_leading_whitespace()
            {
                let ws = syntax_runs.pop().unwrap();
                let mut merged_text = ws.text;
                merged_text.push_str(&cur.text);
                syntax_runs.push(SyntaxRun {
                    text: merged_text,
                    color: cur.color,
                    sgr: cur.sgr,
                });
                continue;
            }

            syntax_runs.push(cur);
        }

        if !syntax_runs.is_empty() {
            batches.push(Batch {
                kind,
                runs: syntax_runs,
            });
        }
    }

    batches
}

/// Canonicalizes a slice of runs by applying batching compression: dropping
/// empty runs, coalescing adjacent runs with identical styling, and absorbing
/// delimiter whitespace.
pub fn canonicalize_runs(runs: &[OutputRun]) -> Vec<OutputRun> {
    let batches = compress_and_batch_runs(runs);
    let mut out = Vec::new();
    for b in batches {
        for r in b.runs {
            out.push(OutputRun {
                kind: b.kind,
                color: r.color,
                sgr: r.sgr,
                text: Cow::Owned(r.text),
            });
        }
    }
    out
}

/// Wrapper to serialize a slice of [`OutputRun`]s as an array of batched runs.
pub struct BatchedRunsRef<'a>(pub &'a [OutputRun]);

impl Serialize for BatchedRunsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let batches = compress_and_batch_runs(self.0);
        let mut seq = serializer.serialize_seq(Some(batches.len()))?;
        for b in &batches {
            seq.serialize_element(&BatchRef {
                kind: b.kind,
                runs: &b.runs,
            })?;
        }
        seq.end()
    }
}

/// Wrapper to deserialize an array of batched runs into `Vec<OutputRun>`.
pub struct BatchedRuns(pub Vec<OutputRun>);

struct SyntaxRunVisitor;

impl<'de> serde::de::Visitor<'de> for SyntaxRunVisitor {
    type Value = (String, TokenColor, Option<u32>);

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a syntax run string or array [text, color?, sgr?]")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok((v.to_string(), TokenColor::Default, None))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let text: String = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::custom("missing syntax run text"))?;
        let color: Option<TokenColor> = seq.next_element()?;
        let sgr: Option<u32> = seq.next_element()?;
        Ok((text, color.unwrap_or_default(), sgr))
    }
}

struct BatchVisitor;

impl<'de> serde::de::Visitor<'de> for BatchVisitor {
    type Value = Vec<OutputRun>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a batch array [sign, ...syntax_runs]")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let Some(sign_str) = seq.next_element::<String>()? else {
            return Ok(Vec::new());
        };
        let sign_char = sign_str
            .chars()
            .next()
            .ok_or_else(|| serde::de::Error::custom("empty sign string in batch"))?;
        let kind = RunKind::from_sign_char(sign_char)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid sign char '{sign_char}'")))?;

        let mut runs = Vec::new();
        struct SyntaxRunSeed;
        impl<'de> serde::de::DeserializeSeed<'de> for SyntaxRunSeed {
            type Value = (String, TokenColor, Option<u32>);
            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_any(SyntaxRunVisitor)
            }
        }

        while let Some((text, color, sgr)) = seq.next_element_seed(SyntaxRunSeed)? {
            if !text.is_empty() {
                runs.push(OutputRun {
                    kind,
                    color,
                    sgr,
                    text: Cow::Owned(text),
                });
            }
        }
        Ok(runs)
    }
}

struct BatchedRunsVisitor;

impl<'de> serde::de::Visitor<'de> for BatchedRunsVisitor {
    type Value = Vec<OutputRun>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an array of batches [[sign, ...syntax_runs], ...]")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut all_runs = Vec::new();
        struct BatchSeed;
        impl<'de> serde::de::DeserializeSeed<'de> for BatchSeed {
            type Value = Vec<OutputRun>;
            fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_seq(BatchVisitor)
            }
        }

        while let Some(batch_runs) = seq.next_element_seed(BatchSeed)? {
            all_runs.extend(batch_runs);
        }
        Ok(all_runs)
    }
}

impl<'de> Deserialize<'de> for BatchedRuns {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer
            .deserialize_seq(BatchedRunsVisitor)
            .map(BatchedRuns)
    }
}

/// Helper function to serialize batched runs for serde `serialize_with`.
pub fn serialize_batched_runs<S>(runs: &[OutputRun], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    BatchedRunsRef(runs).serialize(serializer)
}

/// Helper function to deserialize batched runs for serde `deserialize_with`.
pub fn deserialize_batched_runs<'de, D>(deserializer: D) -> Result<Vec<OutputRun>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    BatchedRuns::deserialize(deserializer).map(|b| b.0)
}
