//! Node and node-array identities: compact indices into the node (or
//! node-array) arenas of a Program's files (see `tsc_types::id_type!`).

tsc_types::id_type!(
    /// A syntax node.
    NodeId
);

tsc_types::id_type!(
    /// A node array (tsc `NodeArray`).
    NodeArrayId
);

tsc_types::id_type!(
    /// A literal text of a tree: the position of its length word in the
    /// tree's text store (see `tsc_store::texts`).
    TextId
);

impl TextId {
    /// The empty text, which takes no words: a missing node's text. The
    /// identity domain never leases its index (`IdentityLimits::node_text_end`).
    pub const EMPTY: Self = Self::new(u32::MAX - 1);
}
