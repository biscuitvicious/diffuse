//! In-process Git index mutation, hunk staging/unstaging/discard, and safety
//! stash checkpoints.

mod apply;
mod checkpoint;
mod discard;
mod stage;

pub use apply::apply_hunk_to_buffer;
pub use checkpoint::{create_stash_checkpoint, default_signature};
pub(crate) use checkpoint::{index_mode_to_entry_kind, open_or_create_index};
pub use discard::{discard_file, discard_hunk};
pub use stage::{stage_file, stage_hunk, unstage_file, unstage_hunk};
