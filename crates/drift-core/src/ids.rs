//! Typed runtime handles and the string interner that produces them.
//!
//! Content is authored with namespaced string ids (e.g. `"core:food"`). At load
//! time the mod-loader interns those strings into compact integer handles so the
//! simulation can index by `u32` instead of hashing strings every tick. Defs hold
//! strings; runtime state holds handles.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Declares a `#[repr(transparent)]` newtype over `u32` used as a runtime handle.
macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl $name {
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl From<u32> for $name {
            #[inline]
            fn from(v: u32) -> Self {
                $name(v)
            }
        }
    };
}

typed_id!(
    /// Handle for a commodity definition.
    CommodityId
);
typed_id!(
    /// Handle for a star system.
    SystemId
);
typed_id!(
    /// Handle for a ship definition.
    ShipId
);
typed_id!(
    /// Handle for a production recipe.
    RecipeId
);

/// Bidirectional string <-> `u32` table.
///
/// [`intern`](Interner::intern) inserts-or-returns (used while building content);
/// [`get`](Interner::get) only looks up (used while linking references, so a
/// dangling reference is a lookup miss rather than a silent new id).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Interner {
    to_id: HashMap<String, u32>,
    to_name: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `name` if absent and return its id; otherwise return the existing id.
    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.to_id.get(name) {
            return id;
        }
        let id = self.to_name.len() as u32;
        self.to_name.push(name.to_owned());
        self.to_id.insert(name.to_owned(), id);
        id
    }

    /// Look up an already-interned name. Returns `None` if never interned.
    pub fn get(&self, name: &str) -> Option<u32> {
        self.to_id.get(name).copied()
    }

    /// Reverse lookup: the string an id was interned from.
    pub fn name(&self, id: u32) -> Option<&str> {
        self.to_name.get(id as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.to_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_name.is_empty()
    }

    /// Iterate `(id, name)` in insertion (id) order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.to_name
            .iter()
            .enumerate()
            .map(|(i, n)| (i as u32, n.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_stable_and_bidirectional() {
        let mut it = Interner::new();
        let a = it.intern("core:food");
        let b = it.intern("core:ore");
        let a2 = it.intern("core:food");
        assert_eq!(a, a2, "re-interning yields the same id");
        assert_ne!(a, b);
        assert_eq!(it.name(a), Some("core:food"));
        assert_eq!(it.name(b), Some("core:ore"));
        assert_eq!(it.len(), 2);
    }

    #[test]
    fn get_does_not_insert() {
        let mut it = Interner::new();
        it.intern("core:food");
        assert_eq!(it.get("core:missing"), None);
        assert_eq!(it.len(), 1, "get must not create new ids");
    }

    #[test]
    fn typed_id_index_roundtrip() {
        let id = CommodityId::from(7);
        assert_eq!(id.index(), 7);
        assert_eq!(id, CommodityId(7));
    }
}
