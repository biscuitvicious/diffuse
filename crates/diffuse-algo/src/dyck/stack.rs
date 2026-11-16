//! Small-capacity inline stack avoiding heap allocations for typical nesting
//! depths.

/// Fast stack with inline array storage avoiding heap allocations for up to `N`
/// elements, with graceful spillover to `Vec` for extreme depths.
#[derive(Clone, Debug)]
pub struct SmallStack<T: Copy + Default, const N: usize> {
    inline: [T; N],
    len: usize,
    spill: Vec<T>,
}

impl<T: Copy + Default, const N: usize> Default for SmallStack<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> SmallStack<T, N> {
    /// Creates a new empty `SmallStack`.
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [T::default(); N],
            len: 0,
            spill: Vec::new(),
        }
    }

    /// Number of elements in the stack.
    #[inline]
    pub fn len(&self) -> usize {
        self.len + self.spill.len()
    }

    /// Whether the stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.spill.is_empty()
    }

    /// Pushes a value onto the stack.
    #[inline]
    pub fn push(&mut self, val: T) {
        if self.len < N {
            self.inline[self.len] = val;
            self.len += 1;
        } else {
            self.spill.push(val);
        }
    }

    /// Pops a value from the stack.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if let Some(val) = self.spill.pop() {
            Some(val)
        } else if self.len > 0 {
            self.len -= 1;
            Some(self.inline[self.len])
        } else {
            None
        }
    }

    /// Returns a reference to the top element of the stack.
    #[inline]
    pub fn last(&self) -> Option<&T> {
        if let Some(val) = self.spill.last() {
            Some(val)
        } else if self.len > 0 {
            Some(&self.inline[self.len - 1])
        } else {
            None
        }
    }

    /// Returns a reference to the element at the given 0-based index from the
    /// bottom.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.len {
            Some(&self.inline[index])
        } else {
            self.spill.get(index - self.len)
        }
    }

    /// Returns an iterator over the elements in the stack from bottom to top.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.inline[..self.len].iter().chain(self.spill.iter())
    }

    /// Returns true if the stack contains the given element.
    #[inline]
    pub fn contains(&self, val: &T) -> bool
    where
        T: PartialEq,
    {
        self.inline[..self.len].contains(val) || self.spill.contains(val)
    }

    /// Returns the number of elements in the stack matching `predicate`.
    #[inline]
    pub fn count_matches<F>(&self, mut predicate: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        self.iter().filter(|&item| predicate(item)).count()
    }

    /// Clears all elements from the stack.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
        self.spill.clear();
    }
}

impl<T: Copy + Default + PartialEq, const N: usize> PartialEq for SmallStack<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl<T: Copy + Default + Eq, const N: usize> Eq for SmallStack<T, N> {}
