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
/// - 2 elements: `["+text", "kwd"]` for styled runs without SGR.
/// - 3 elements: `["+text", null, 4]` or `["+text", "kwd", 1]` when SGR is
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

    pub fn reverse(&self) -> Self {
        OutputRun {
            kind: self.kind.reverse(),
            color: self.color,
            sgr: self.sgr,
            text: self.text.clone(),
        }
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
