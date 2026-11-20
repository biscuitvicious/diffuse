//! Unified diff row definitions, classifications, coordinates, and span models.

use std::fmt;

use diffuse_algo::require;
use serde::{Deserialize, Serialize};

/// The semantic classification of a token run in a diff row.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RunKind {
    Ctx,
    Del,
    Add,
    /// Reflow-moved source text: characters that left this line for peer
    /// rows in the same reflow group. Old-side content, like [`RunKind::Del`],
    /// but explicitly *not* deleted — it survives verbatim elsewhere in the
    /// hunk. Renderers skip it by default (the peer rows show where it went);
    /// the strike is reserved for genuinely deleted text. See `is_del_like`.
    MovedSrc,
    /// Reflow-moved target text: characters a peer-grouped row shares with
    /// its group — moved in, explicitly not novel. Emitted forward for
    /// peer-grouped additions (the mirror of [`RunKind::MovedSrc`]); also
    /// the image of `MovedSrc` under reversal, which keeps wire reversal
    /// an exact involution. Renders exactly as context. See `is_add_like`.
    MovedTgt,
}

impl RunKind {
    #[inline]
    pub fn sign_char(self) -> char {
        match self {
            RunKind::Ctx => '_',
            RunKind::Del => '-',
            RunKind::Add => '+',
            RunKind::MovedSrc => '<',
            RunKind::MovedTgt => '>',
        }
    }

    #[inline]
    pub fn from_sign_char(c: char) -> Option<Self> {
        match c {
            '_' => Some(RunKind::Ctx),
            '-' => Some(RunKind::Del),
            '+' => Some(RunKind::Add),
            '<' => Some(RunKind::MovedSrc),
            '>' => Some(RunKind::MovedTgt),
            _ => None,
        }
    }

    /// True for old-side change marking: struck-deleted or struck-moved.
    /// Span derivation, wholesale coverage, and forward-projection filters
    /// must treat both identically (moved text still left this line); only
    /// renderers distinguish them.
    #[inline]
    pub fn is_del_like(self) -> bool {
        matches!(self, RunKind::Del | RunKind::MovedSrc)
    }

    /// True for new-side-only content: filled-novel or moved-in. Reverse
    /// (old-side) projections must exclude both.
    #[inline]
    pub fn is_add_like(self) -> bool {
        matches!(self, RunKind::Add | RunKind::MovedTgt)
    }

    pub const fn reverse(self) -> Self {
        match self {
            RunKind::Ctx => RunKind::Ctx,
            RunKind::Del => RunKind::Add,
            RunKind::Add => RunKind::Del,
            RunKind::MovedSrc => RunKind::MovedTgt,
            RunKind::MovedTgt => RunKind::MovedSrc,
        }
    }
}

/// A diff row kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Add,
    Del,
    Ctx,
    File,
    Mod,
    Mov,
    #[serde(skip_serializing)]
    Sep,
}

impl RowKind {
    pub const fn reverse(self) -> RowKind {
        match self {
            RowKind::Add => RowKind::Del,
            RowKind::Del => RowKind::Add,
            RowKind::Mod => RowKind::Mod,
            RowKind::Mov => RowKind::Mov,
            RowKind::Ctx | RowKind::Sep => RowKind::Ctx,
            RowKind::File => RowKind::File,
        }
    }
}

/// Unique identifier for an irreducible (N -> M) peer reflow group within a
/// hunk.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub u32);

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for GroupId {
    type Target = u32;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Symmetric group metadata describing an N -> M transition.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerGroupMeta {
    pub id: GroupId,
    pub old_count: usize, // N
    pub new_count: usize, // M
}

impl PeerGroupMeta {
    #[inline]
    pub const fn new(id: GroupId, old_count: usize, new_count: usize) -> Self {
        Self {
            id,
            old_count,
            new_count,
        }
    }

    #[inline]
    pub const fn max_slots(&self) -> usize {
        if self.old_count > self.new_count {
            self.old_count
        } else {
            self.new_count
        }
    }

    #[inline]
    pub const fn is_pure_del(&self) -> bool {
        self.old_count > 0 && self.new_count == 0
    }

    #[inline]
    pub const fn is_pure_add(&self) -> bool {
        self.old_count == 0 && self.new_count > 0
    }

    #[inline]
    pub const fn is_single_mod(&self) -> bool {
        self.old_count == 1 && self.new_count == 1
    }

    #[inline]
    pub const fn is_compound(&self) -> bool {
        self.old_count > 1 || self.new_count > 1
    }
}

/// Identifies the peer reflow group a diff row belongs to.
///
/// A row carries `Some(Peer)` when slot alignment placed it in an (N -> M)
/// peer group with a strong token link (a paired `Mod` slot or a salient
/// moved-token relationship); otherwise it carries `None` and renders as a
/// standalone row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum RowGroup {
    /// A row belonging to an (N -> M) peer reflow group.
    Peer {
        id: GroupId,
        slot: usize,
        n: usize,
        m: usize,
    },
}

impl RowGroup {
    #[inline]
    pub fn is_peer(self) -> bool {
        matches!(self, RowGroup::Peer { .. })
    }

    #[inline]
    pub fn peer_id(self) -> Option<GroupId> {
        match self {
            RowGroup::Peer { id, .. } => Some(id),
        }
    }

    #[inline]
    pub fn is_multiline(self) -> bool {
        match self {
            RowGroup::Peer { n, m, .. } => n > 1 || m > 1,
        }
    }

    #[inline]
    pub fn reverse(self) -> Self {
        match self {
            RowGroup::Peer { id, slot, n, m } => RowGroup::Peer {
                id,
                slot,
                n: m,
                m: n,
            },
        }
    }
}

/// A line deletion: (`line_num`, text, moved).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DelLine {
    pub num: u32,
    pub text: String,
}

impl DelLine {
    #[inline]
    pub fn new(num: u32, text: impl Into<String>) -> Self {
        require!(num > 0, "line number must be non-zero (1-based)");
        Self {
            num,
            text: text.into(),
        }
    }
}

/// A line addition: (`line_num`, text).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AddLine {
    pub num: u32,
    pub text: String,
}

impl AddLine {
    #[inline]
    pub fn new(num: u32, text: impl Into<String>) -> Self {
        require!(num > 0, "line number must be non-zero (1-based)");
        Self {
            num,
            text: text.into(),
        }
    }
}

/// A context line: (`old_pos`, `new_pos`, text).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CtxLine {
    pub old_pos: u32,
    pub new_pos: u32,
    pub text: String,
}

impl CtxLine {
    #[inline]
    pub fn new(old_pos: u32, new_pos: u32, text: impl Into<String>) -> Self {
        require!(
            old_pos > 0,
            "old_pos line number must be non-zero (1-based)"
        );
        require!(
            new_pos > 0,
            "new_pos line number must be non-zero (1-based)"
        );
        Self {
            old_pos,
            new_pos,
            text: text.into(),
        }
    }
}

/// Change spans between old and new lines.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Spans {
    pub del: Vec<(usize, usize)>,
    pub add: Vec<(usize, usize)>,
}

impl Spans {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.del.is_empty() && self.add.is_empty()
    }

    #[inline]
    pub fn del_char_len(&self) -> usize {
        self.del.iter().map(|(s, e)| e.saturating_sub(*s)).sum()
    }

    #[inline]
    pub fn add_char_len(&self) -> usize {
        self.add.iter().map(|(s, e)| e.saturating_sub(*s)).sum()
    }

    pub fn reverse(mut self) -> Self {
        std::mem::swap(&mut self.del, &mut self.add);
        self
    }
}

/// A diff row.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Row {
    Ctx(CtxLine),
    Del {
        line: DelLine,
        #[serde(default, skip_serializing_if = "Spans::is_empty")]
        spans: Spans,
        #[serde(skip)]
        cached_runs: Vec<(RunKind, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<RowGroup>,
    },
    Add {
        line: AddLine,
        #[serde(default, skip_serializing_if = "Spans::is_empty")]
        spans: Spans,
        #[serde(skip)]
        cached_runs: Vec<(RunKind, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<RowGroup>,
    },
    Mod {
        old_line: DelLine,
        new_line: AddLine,
        #[serde(default, skip_serializing_if = "Spans::is_empty")]
        spans: Spans,
        #[serde(skip)]
        cached_runs: Vec<(RunKind, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<RowGroup>,
    },
    MovSrc {
        line: DelLine,
        #[serde(default, skip_serializing_if = "Spans::is_empty")]
        spans: Spans,
        #[serde(skip)]
        cached_runs: Vec<(RunKind, String)>,
    },
    MovTgt {
        line: AddLine,
        #[serde(default, skip_serializing_if = "Spans::is_empty")]
        spans: Spans,
        #[serde(skip)]
        cached_runs: Vec<(RunKind, String)>,
    },
    #[serde(skip_serializing)]
    Sep(String),
}

impl Row {
    #[inline]
    pub fn mod_row(old_line: DelLine, new_line: AddLine) -> Self {
        Self::Mod {
            old_line,
            new_line,
            spans: Spans::default(),
            cached_runs: Vec::new(),
            group: None,
        }
    }

    #[inline]
    pub fn mod_with_spans(old_line: DelLine, new_line: AddLine, spans: Spans) -> Self {
        Self::Mod {
            old_line,
            new_line,
            spans,
            cached_runs: Vec::new(),
            group: None,
        }
    }

    #[inline]
    pub fn mod_with_alignment(
        old_line: DelLine,
        new_line: AddLine,
        spans: Spans,
        cached_runs: Vec<(RunKind, String)>,
    ) -> Self {
        Self::Mod {
            old_line,
            new_line,
            spans,
            cached_runs,
            group: None,
        }
    }

    #[inline]
    pub fn del(line: DelLine) -> Self {
        Self::Del {
            line,
            spans: Spans::default(),
            cached_runs: Vec::new(),
            group: None,
        }
    }

    #[inline]
    pub fn add(line: AddLine) -> Self {
        Self::Add {
            line,
            spans: Spans::default(),
            cached_runs: Vec::new(),
            group: None,
        }
    }

    #[inline]
    pub fn mov_src(line: DelLine) -> Self {
        Self::MovSrc {
            line,
            spans: Spans::default(),
            cached_runs: Vec::new(),
        }
    }

    #[inline]
    pub fn mov_tgt(line: AddLine) -> Self {
        Self::MovTgt {
            line,
            spans: Spans::default(),
            cached_runs: Vec::new(),
        }
    }

    #[inline]
    pub fn kind(&self) -> RowKind {
        match self {
            Row::Ctx(_) | Row::Sep(_) => RowKind::Ctx,
            Row::Del { .. } => RowKind::Del,
            Row::Add { .. } => RowKind::Add,
            Row::Mod { .. } => RowKind::Mod,
            Row::MovSrc { .. } | Row::MovTgt { .. } => RowKind::Mov,
        }
    }

    #[inline]
    pub fn text(&self) -> &str {
        match self {
            Row::Ctx(ctx_line) => &ctx_line.text,
            Row::Del { line, .. } => &line.text,
            Row::Add { line, .. } => &line.text,
            Row::Mod { new_line, .. } => &new_line.text,
            Row::MovSrc { line, .. } => &line.text,
            Row::MovTgt { line, .. } => &line.text,
            Row::Sep(s) => s.as_str(),
        }
    }

    #[inline]
    pub fn old_text(&self) -> Option<&str> {
        match self {
            Row::Ctx(ctx_line) => Some(&ctx_line.text),
            Row::Del { line, .. } => Some(&line.text),
            Row::Mod { old_line, .. } => Some(&old_line.text),
            Row::MovSrc { line, .. } => Some(&line.text),
            Row::Add { .. } | Row::MovTgt { .. } | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn new_text(&self) -> Option<&str> {
        match self {
            Row::Ctx(ctx_line) => Some(&ctx_line.text),
            Row::Add { line, .. } => Some(&line.text),
            Row::Mod { new_line, .. } => Some(&new_line.text),
            Row::MovTgt { line, .. } => Some(&line.text),
            Row::Del { .. } | Row::MovSrc { .. } | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn old_line(&self) -> Option<u32> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.old_pos),
            Row::Del { line, .. } => Some(line.num),
            Row::Mod { old_line, .. } => Some(old_line.num),
            Row::MovSrc { line, .. } => Some(line.num),
            Row::Add { .. } | Row::MovTgt { .. } | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn new_line(&self) -> Option<u32> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.new_pos),
            Row::Add { line, .. } => Some(line.num),
            Row::Mod { new_line, .. } => Some(new_line.num),
            Row::MovTgt { line, .. } => Some(line.num),
            Row::Del { .. } | Row::MovSrc { .. } | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn old_line_num(&self) -> Option<u32> {
        self.old_line()
    }

    #[inline]
    pub fn new_line_num(&self) -> Option<u32> {
        self.new_line()
    }

    #[inline]
    pub fn spans(&self) -> Option<&Spans> {
        match self {
            Row::Mod { spans, .. } => Some(spans),
            Row::MovTgt { spans, .. } if !spans.is_empty() => Some(spans),
            Row::MovSrc { spans, .. } if !spans.is_empty() => Some(spans),
            Row::Add { spans, .. } if !spans.is_empty() => Some(spans),
            Row::Del { spans, .. } if !spans.is_empty() => Some(spans),
            _ => None,
        }
    }

    #[inline]
    pub fn is_moved(&self) -> bool {
        matches!(self, Row::MovSrc { .. } | Row::MovTgt { .. })
    }

    /// Profiled-tape merged runs handed from assemble (`cached_runs`).
    #[inline]
    pub fn cached_runs(&self) -> &[(RunKind, String)] {
        match self {
            Row::Mod { cached_runs, .. } => cached_runs,
            Row::MovTgt { cached_runs, .. } => cached_runs,
            Row::MovSrc { cached_runs, .. } => cached_runs,
            Row::Add { cached_runs, .. } => cached_runs,
            Row::Del { cached_runs, .. } => cached_runs,
            _ => &[],
        }
    }

    /// Peer group membership if slot alignment placed this row in an (N -> M)
    /// peer reflow group.
    #[inline]
    pub fn group(&self) -> Option<RowGroup> {
        match self {
            Row::Mod { group, .. } => *group,
            Row::Del { group, .. } => *group,
            Row::Add { group, .. } => *group,
            _ => None,
        }
    }

    #[inline]
    pub fn set_group(&mut self, new_group: Option<RowGroup>) {
        match self {
            Row::Mod { group, .. } => *group = new_group,
            Row::Del { group, .. } => *group = new_group,
            Row::Add { group, .. } => *group = new_group,
            _ => {}
        }
    }

    pub fn reverse(self) -> Self {
        match self {
            Row::Ctx(ctx_line) => Row::Ctx(CtxLine {
                old_pos: ctx_line.new_pos,
                new_pos: ctx_line.old_pos,
                text: ctx_line.text,
            }),
            Row::Del {
                line,
                spans,
                cached_runs,
                group,
            } => Row::Add {
                line: AddLine::new(line.num, line.text),
                spans: spans.reverse(),
                cached_runs: cached_runs
                    .into_iter()
                    .map(|(k, t)| (k.reverse(), t))
                    .collect(),
                group: group.map(RowGroup::reverse),
            },
            Row::Add {
                line,
                spans,
                cached_runs,
                group,
            } => Row::Del {
                line: DelLine::new(line.num, line.text),
                spans: spans.reverse(),
                cached_runs: cached_runs
                    .into_iter()
                    .map(|(k, t)| (k.reverse(), t))
                    .collect(),
                group: group.map(RowGroup::reverse),
            },
            Row::Mod {
                old_line,
                new_line,
                spans,
                cached_runs,
                group,
            } => Row::Mod {
                old_line: DelLine::new(new_line.num, new_line.text),
                new_line: AddLine::new(old_line.num, old_line.text),
                spans: spans.reverse(),
                cached_runs: cached_runs
                    .into_iter()
                    .map(|(k, t)| (k.reverse(), t))
                    .collect(),
                group: group.map(RowGroup::reverse),
            },
            Row::MovSrc {
                line,
                spans,
                cached_runs,
            } => Row::MovTgt {
                line: AddLine::new(line.num, line.text),
                spans: spans.reverse(),
                cached_runs: cached_runs
                    .into_iter()
                    .map(|(k, t)| (k.reverse(), t))
                    .collect(),
            },
            Row::MovTgt {
                line,
                spans,
                cached_runs,
            } => Row::MovSrc {
                line: DelLine::new(line.num, line.text),
                spans: spans.reverse(),
                cached_runs: cached_runs
                    .into_iter()
                    .map(|(k, t)| (k.reverse(), t))
                    .collect(),
            },
            Row::Sep(s) => Row::Sep(s),
        }
    }
}
