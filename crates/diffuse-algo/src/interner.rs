//! Generic high-performance symbol table and string/token interner.
//!
//! Provides bidirectional mapping between arbitrary keys `T` and compact
//! 32-bit `SymbolId` integers.

use std::{borrow::Borrow, hash::Hash};

use rustc_hash::FxHashMap;

/// A compact 32-bit symbol identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SymbolId(pub u32);

impl SymbolId {
    /// Creates a new symbol identifier.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the symbol identifier as a `u32`.
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Returns the symbol identifier as a `usize`.
    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for SymbolId {
    #[inline]
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<SymbolId> for u32 {
    #[inline]
    fn from(id: SymbolId) -> Self {
        id.0
    }
}

/// A high-performance bidirectional symbol table.
#[derive(Clone, Debug)]
pub struct SymbolTable<T> {
    item_to_id: FxHashMap<T, SymbolId>,
    id_to_item: Vec<T>,
}

impl<T> Default for SymbolTable<T> {
    fn default() -> Self {
        Self {
            item_to_id: FxHashMap::default(),
            id_to_item: Vec::new(),
        }
    }
}

impl<T> SymbolTable<T>
where
    T: Eq + Hash,
{
    /// Creates an empty symbol table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty symbol table with preallocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            item_to_id: FxHashMap::with_capacity_and_hasher(capacity, Default::default()),
            id_to_item: Vec::with_capacity(capacity),
        }
    }

    /// Returns the number of unique symbols recorded.
    #[inline]
    pub fn len(&self) -> usize {
        self.id_to_item.len()
    }

    /// Returns true if the symbol table contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.id_to_item.is_empty()
    }

    /// Clears all recorded symbols while retaining allocated capacity.
    pub fn clear(&mut self) {
        self.item_to_id.clear();
        self.id_to_item.clear();
    }

    /// Resolves a symbol identifier back to its item reference.
    #[inline]
    pub fn resolve(&self, id: SymbolId) -> Option<&T> {
        self.id_to_item.get(id.as_usize())
    }

    /// Queries the symbol identifier for an item if it has already been
    /// interned.
    #[inline]
    pub fn get_id<Q>(&self, item: &Q) -> Option<SymbolId>
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.item_to_id.get(item).copied()
    }

    /// Returns true if the item has already been interned.
    #[inline]
    pub fn contains<Q>(&self, item: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.item_to_id.contains_key(item)
    }

    /// Returns all registered symbols in registration order.
    pub fn items(&self) -> &[T] {
        &self.id_to_item
    }
}

impl<T> SymbolTable<T>
where
    T: Eq + Hash + Clone,
{
    /// Interns an item, returning its unique `SymbolId`.
    pub fn intern(&mut self, item: T) -> SymbolId {
        if let Some(&id) = self.item_to_id.get(&item) {
            return id;
        }
        let id = SymbolId(self.id_to_item.len() as u32);
        self.item_to_id.insert(item.clone(), id);
        self.id_to_item.push(item);
        id
    }

    /// Interns a borrowed representation of an item, allocating only if the
    /// item is novel.
    pub fn intern_borrowed<Q>(&mut self, item: &Q) -> SymbolId
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwned<Owned = T>,
    {
        if let Some(&id) = self.item_to_id.get(item) {
            return id;
        }
        let owned = item.to_owned();
        let id = SymbolId(self.id_to_item.len() as u32);
        self.item_to_id.insert(owned.clone(), id);
        self.id_to_item.push(owned);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_id_conversions() {
        let sym = SymbolId::new(42);
        assert_eq!(sym.as_u32(), 42);
        assert_eq!(sym.as_usize(), 42);
        assert_eq!(u32::from(sym), 42);
        assert_eq!(SymbolId::from(42u32), sym);
    }

    #[test]
    fn test_intern_and_resolve_basic() {
        let mut table: SymbolTable<String> = SymbolTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);

        let id1 = table.intern_borrowed("apple");
        let id2 = table.intern_borrowed("banana");
        let id3 = table.intern_borrowed("apple");

        assert_eq!(id1, id3, "identical strings must share SymbolId");
        assert_ne!(id1, id2, "distinct strings must receive distinct SymbolIds");
        assert_eq!(table.resolve(id1), Some(&"apple".to_string()));
        assert_eq!(table.resolve(id2), Some(&"banana".to_string()));
        assert_eq!(table.resolve(SymbolId::new(999)), None);

        assert!(table.contains("apple"));
        assert!(!table.contains("orange"));
        assert_eq!(table.get_id("apple"), Some(id1));
        assert_eq!(table.get_id("orange"), None);

        assert_eq!(table.len(), 2);
        assert_eq!(table.items(), &["apple".to_string(), "banana".to_string()]);
    }

    #[test]
    fn test_intern_owned_and_clear() {
        let mut table: SymbolTable<i32> = SymbolTable::with_capacity(16);
        let id10 = table.intern(10);
        let id20 = table.intern(20);
        let id10_dup = table.intern(10);

        assert_eq!(id10, id10_dup);
        assert_ne!(id10, id20);
        assert_eq!(table.resolve(id10), Some(&10));
        assert_eq!(table.resolve(id20), Some(&20));

        table.clear();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.resolve(id10), None);
    }
}
