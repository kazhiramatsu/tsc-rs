//! Compact identities.
//!
//! An identity names a slot of an arena (a node, a symbol, a type, ...) by
//! its index. [`id_type!`](crate::id_type) stores `index + 1` in a
//! `NonZeroU32`, so `Option<Id>` is four bytes like the identity itself: the
//! syntax trees, the binder and the checker keep tens of millions of
//! optional references for a large Program. Hashing, ordering, `Debug` and
//! `Default` all use the index, exactly as a plain `u32` field would.

/// Define `pub struct $name` as a compact identity (see the module docs)
/// with `$name::new(index)` and `id.index()`. Indices range below
/// `u32::MAX`.
#[macro_export]
macro_rules! id_type {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
        pub struct $name(::std::num::NonZeroU32);

        impl $name {
            /// The identity of index `index` (below `u32::MAX`).
            #[inline]
            pub const fn new(index: u32) -> Self {
                match Self::checked_new(index) {
                    Some(id) => id,
                    None => panic!("identity index out of range"),
                }
            }

            /// The identity of index `index`, or `None` for `u32::MAX`, which
            /// no identity names: for indices from outside the compiler.
            #[inline]
            pub const fn checked_new(index: u32) -> Option<Self> {
                match ::std::num::NonZeroU32::new(index.wrapping_add(1)) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// The index this identity names.
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

        impl ::std::hash::Hash for $name {
            #[inline]
            fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
                self.index().hash(state);
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.index()).finish()
            }
        }

        const _: () = assert!(::std::mem::size_of::<Option<$name>>() == 4);
    };
}
