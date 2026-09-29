//! Node and node-array identities.
//!
//! An identity is a program-unique index into the node (or node-array)
//! arenas of a Program's files. It is stored as `index + 1` in a
//! `NonZeroU32`, so `Option<NodeId>` is four bytes like `NodeId` itself: the
//! syntax tree, the binder and the checker keep tens of millions of optional
//! node references for a large Program. Hashing, ordering, `Debug` and
//! `Default` all use the index, exactly as the former plain `u32` did.

use std::num::NonZeroU32;

macro_rules! identity {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The identity of arena index `index` (below `u32::MAX`).
            #[inline]
            pub const fn new(index: u32) -> Self {
                match NonZeroU32::new(index.wrapping_add(1)) {
                    Some(value) => Self(value),
                    None => panic!("identity index out of range"),
                }
            }

            /// The arena index this identity names.
            #[inline]
            pub const fn index(self) -> u32 {
                self.0.get() - 1
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new(0)
            }
        }

        impl std::hash::Hash for $name {
            #[inline]
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.index().hash(state);
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.index()).finish()
            }
        }
    };
}

identity!(
    /// A syntax node.
    NodeId
);

identity!(
    /// A node array (tsc `NodeArray`).
    NodeArrayId
);

const _: () = assert!(std::mem::size_of::<Option<NodeId>>() == 4);
