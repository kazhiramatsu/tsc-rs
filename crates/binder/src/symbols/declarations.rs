use tsc_store::claimed::{slice_tagged, ClaimedRecords, Tagged, DECLARATION_ITEM_REGION};
use tsc_syntax::NodeId;

/// A declaration list's element in the store (region 37).
pub type DeclarationItem = Tagged<NodeId, DECLARATION_ITEM_REGION>;

/// A symbol's declaration list: a span of [`DeclarationItem`]s that a
/// [`DeclarationPool`] issued (tsgo's `Declarations` slice header). Eight
/// bytes and `Copy`: a symbol that carries another's declarations copies the
/// span. It reads through the store, so it stays valid while the pool that
/// issued it lives (the symbol arena that owns the symbol).
#[derive(Clone, Copy, Default)]
pub struct Declarations {
    start: u32,
    len: u32,
}

const _: () = assert!(std::mem::size_of::<Declarations>() == 8);

impl Declarations {
    pub const fn new() -> Self {
        Self { start: 0, len: 0 }
    }
}

impl std::ops::Deref for Declarations {
    type Target = [NodeId];
    #[inline]
    fn deref(&self) -> &[NodeId] {
        if self.len == 0 {
            return &[];
        }
        slice_tagged::<NodeId, DECLARATION_ITEM_REGION>(self.start, self.len)
    }
}

impl PartialEq for Declarations {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Declarations {}

impl std::fmt::Debug for Declarations {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<const N: usize> PartialEq<[NodeId; N]> for Declarations {
    fn eq(&self, other: &[NodeId; N]) -> bool {
        **self == other[..]
    }
}

/// The declarations by value, in order.
impl IntoIterator for Declarations {
    type Item = NodeId;
    type IntoIter = std::iter::Copied<std::slice::Iter<'static, NodeId>>;
    fn into_iter(self) -> Self::IntoIter {
        let nodes: &'static [NodeId] = if self.len == 0 {
            &[]
        } else {
            slice_tagged::<NodeId, DECLARATION_ITEM_REGION>(self.start, self.len)
        };
        nodes.iter().copied()
    }
}

impl<'a> IntoIterator for &'a Declarations {
    type Item = &'a NodeId;
    type IntoIter = std::slice::Iter<'a, NodeId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// The owner of the declaration lists of one symbol arena: a document's
/// binder symbols or a checker's symbols. Its rooms, and with them every span
/// it issued, go when it drops.
pub struct DeclarationPool {
    items: ClaimedRecords<DeclarationItem>,
    /// The position after the last list issued: a list that ends there grows
    /// in place.
    end: u32,
}

impl DeclarationPool {
    /// `chunk` items per room: a document's lists share small rooms (so that
    /// rooms of different documents share pages), a checker's one larger.
    pub fn with_chunk_capacity(chunk: u32) -> Self {
        Self {
            items: ClaimedRecords::with_chunk_capacity(chunk),
            end: 0,
        }
    }

    /// A list of `nodes` (empty without claiming anything).
    pub fn alloc(&mut self, nodes: &[NodeId]) -> Declarations {
        let _census =
            tsc_types::perf::census::scope(tsc_types::perf::census::Site::SymbolDeclarations);
        tsc_types::perf::census::note(
            tsc_types::perf::census::Site::SymbolDeclarations,
            1,
            nodes.len(),
            std::mem::size_of_val(nodes),
        );
        let len = u32::try_from(nodes.len()).expect("declaration list length exceeds u32");
        if len == 0 {
            return Declarations::new();
        }
        let start = self.push_nodes(nodes);
        self.end = start + len;
        Declarations { start, len }
    }

    /// `list` with `node` appended (addDeclarationToSymbol): in place when the
    /// list is the last one issued and its room has space, else a copy.
    pub fn append(&mut self, list: Declarations, node: NodeId) -> Declarations {
        if list.len != 0 && list.start + list.len == self.end {
            let position = self.items.push(Tagged(node));
            if position == self.end {
                self.end += 1;
                return Declarations {
                    start: list.start,
                    len: list.len + 1,
                };
            }
            // The room was full and `position` starts the next one: copy the
            // list there (one slot stays unused).
        }
        self.extend(list, &[node])
    }

    /// `list` followed by `nodes`, as a new list.
    pub fn extend(&mut self, list: Declarations, nodes: &[NodeId]) -> Declarations {
        if list.len == 0 {
            return self.alloc(nodes);
        }
        let mut joined = Vec::with_capacity(list.len as usize + nodes.len());
        joined.extend_from_slice(&list);
        joined.extend_from_slice(nodes);
        self.alloc(&joined)
    }

    fn push_nodes(&mut self, nodes: &[NodeId]) -> u32 {
        const SMALL: usize = 8;
        if nodes.len() <= SMALL {
            let mut items = [Tagged(NodeId::new(0)); SMALL];
            for (item, &node) in items.iter_mut().zip(nodes) {
                *item = Tagged(node);
            }
            return self.items.push_slice(&items[..nodes.len()]);
        }
        let items: Vec<DeclarationItem> = nodes.iter().map(|&node| Tagged(node)).collect();
        self.items.push_slice(&items)
    }

    /// Bytes of the issued items (the live lists and the copies they left).
    pub fn live_bytes(&self) -> usize {
        self.items.stats().live_bytes
    }
}

impl std::fmt::Debug for DeclarationPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeclarationPool")
            .field("live_bytes", &self.live_bytes())
            .finish()
    }
}
