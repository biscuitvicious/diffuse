//! Thread-local scratch arena for zero-allocation cluster path alignment.

use std::cell::RefCell;

/// Reusable scratch buffers for the single-path cluster DP solver.
#[derive(Default)]
pub struct DpScratchArena {
    pub(crate) prev: Vec<i64>,
    pub(crate) cur: Vec<i64>,
}

impl DpScratchArena {
    /// Prepares the two DP score stripes for a cluster with `m` new rows,
    /// filling both with `fill` while reusing existing capacity.
    pub(crate) fn prepare_stripes(&mut self, len: usize, fill: i64) {
        self.prev.clear();
        self.prev.resize(len, fill);
        self.cur.clear();
        self.cur.resize(len, fill);
    }

    /// Clears scratch buffers while retaining allocated capacity.
    pub fn clear(&mut self) {
        self.prev.clear();
        self.cur.clear();
    }
}

thread_local! {
    static ARENA: RefCell<DpScratchArena> = RefCell::new(DpScratchArena::default());
}

/// Run `f` with this thread's reusable scratch arena so DP stripes keep their
/// capacity across clusters instead of reallocating on every solve.
pub fn with_tls_arena<R>(f: impl FnOnce(&mut DpScratchArena) -> R) -> R {
    ARENA.with_borrow_mut(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_arena_reuses_and_clears_stripes() {
        with_tls_arena(|arena| {
            arena.prepare_stripes(16, -1);
            assert_eq!(arena.prev.len(), 16);
            assert_eq!(arena.cur.len(), 16);
            let cap = arena.prev.capacity();
            arena.clear();
            assert!(arena.prev.is_empty());
            assert!(arena.cur.is_empty());
            assert_eq!(arena.prev.capacity(), cap);
        });
    }
}
