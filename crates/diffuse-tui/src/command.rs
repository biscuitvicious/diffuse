//! TUI Command Model — declarative representation of user actions and intents.

/// High-level user commands dispatched to update application state.
#[derive(Clone, Debug, PartialEq)]
pub enum TuiCommand {
    /// Exit the application.
    Quit,
    /// Dismiss the active modal, cancel selection, or clear transient state
    /// (Esc).
    Dismiss,

    // --- Navigation ---
    MoveUp(usize),
    MoveDown(usize),
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    MoveToTop,
    MoveToBottom,
    ScrollLeft(usize),
    ScrollRight(usize),
    JumpHunkPrev,
    JumpHunkNext,
    JumpFilePrev,
    JumpFileNext,
    JumpToLine(u32),
    JumpOpposingSide,

    // --- Mode Toggles ---
    ToggleSyntax,
    ToggleWordDiff,
    ToggleRawDiff,
    ToggleOldLineGutter,
    ToggleHelp,
    ToggleTheme,
    ToggleStaged,
    ForceReload,

    ToggleCollapseFile(Option<usize>),
    ToggleExpandAllMods,
    ExpandAllHidden,
    ResetCurrentHunkContext,
    ResetAllContext,

    // --- Modals & Dialogs ---
    OpenFileFinder,
    CloseFileFinder,
    ConfirmFileFinder,
    UpdateFileFinderQuery(String),

    // --- Selection & Editing ---
    StartSelection,
    ConfirmSelection,
    ClearSelection,
    OpenEditor,

    // --- Git Actions ---
    Stage,
    Unstage,
    Discard,

    // --- Notes ---
    StartNoteDraft,
    CommitNoteDraft,
    CancelNoteDraft,
    DeleteNote,
    CycleNoteStatus,
    ToggleShowResolvedNotes,
    ToggleShowNoteSource,

    // --- Mouse & Layout Interaction ---
    MouseClick {
        row: usize,
        col: usize,
        shift: bool,
    },
    MouseScrollUp {
        n: usize,
        row: usize,
        col: usize,
    },
    MouseScrollDown {
        n: usize,
        row: usize,
        col: usize,
    },
    MouseDrag {
        row: usize,
        col: usize,
        shift: bool,
    },
    EndMouseDrag,
}
