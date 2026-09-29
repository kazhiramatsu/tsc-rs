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
