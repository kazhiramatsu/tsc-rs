use tsc_store::claimed::{slice_tagged, ClaimedRecords, Tagged, DECLARATION_ITEM_REGION};
use tsc_syntax::NodeId;

/// A declaration list's element in the store (region 37).
pub type DeclarationItem = Tagged<NodeId, DECLARATION_ITEM_REGION>;

/// A symbol's declarations, with a single node held in the header and longer
/// lists in a [`DeclarationPool`]'s store span. Eight bytes and `Copy`: a
/// symbol carrying another's declarations copies this value. A longer list
/// stays valid while the pool that issued it lives.
#[derive(Clone, Copy, Default)]
pub struct Declarations {
    node_or_start: Option<NodeId>,
    len: u32,
}

const _: () = assert!(std::mem::size_of::<Declarations>() == 8);

impl Declarations {
    pub const fn new() -> Self {
        Self {
            node_or_start: None,
            len: 0,
        }
    }

    #[inline]
    fn single(node: NodeId) -> Self {
        Self {
            node_or_start: Some(node),
            len: 1,
        }
    }

    fn span(start: u32, len: u32) -> Self {
        Self {
            node_or_start: Some(NodeId::new(start)),
            len,
        }
    }

    #[inline]
    fn node_or_start(&self) -> NodeId {
        self.node_or_start.expect("nonempty declaration list")
    }
}

impl std::ops::Deref for Declarations {
    type Target = [NodeId];
    #[inline]
    fn deref(&self) -> &[NodeId] {
        match self.len {
            0 => &[],
            1 => std::slice::from_ref(self.node_or_start.as_ref().expect("single declaration")),
            _ => slice_tagged::<NodeId, DECLARATION_ITEM_REGION>(
                self.node_or_start().index(),
                self.len,
            ),
        }
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
    type IntoIter = DeclarationIter;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        let (single, nodes): (_, &'static [NodeId]) = match self.len {
            0 => (None, &[]),
            1 => (self.node_or_start, &[]),
            _ => (
                None,
                slice_tagged::<NodeId, DECLARATION_ITEM_REGION>(
                    self.node_or_start().index(),
                    self.len,
                ),
            ),
        };
        DeclarationIter {
            single,
            multiple: nodes.iter().copied(),
        }
    }
}

/// An owned declaration-list iterator. The singleton is kept here because
/// its list header can be moved or dropped after creating the iterator.
#[derive(Clone, Debug)]
pub struct DeclarationIter {
    single: Option<NodeId>,
    multiple: std::iter::Copied<std::slice::Iter<'static, NodeId>>,
}

impl Iterator for DeclarationIter {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        self.single.take().or_else(|| self.multiple.next())
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    #[inline]
    fn count(self) -> usize {
        self.len()
    }

    #[inline]
    fn last(self) -> Option<NodeId> {
        self.multiple.last().or(self.single)
    }

    #[inline]
    fn nth(&mut self, mut n: usize) -> Option<NodeId> {
        if let Some(node) = self.single.take() {
            if n == 0 {
                return Some(node);
            }
            n -= 1;
        }
        self.multiple.nth(n)
    }
}

impl DoubleEndedIterator for DeclarationIter {
    #[inline]
    fn next_back(&mut self) -> Option<NodeId> {
        self.multiple.next_back().or_else(|| self.single.take())
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<NodeId> {
        let len = self.multiple.len();
        if n < len {
            return self.multiple.nth_back(n);
        }
        self.multiple = [].iter().copied();
        let single = self.single.take();
        if n == len {
            single
        } else {
            None
        }
    }
}

impl ExactSizeIterator for DeclarationIter {
    #[inline]
    fn len(&self) -> usize {
        usize::from(self.single.is_some()) + self.multiple.len()
    }
}

impl std::iter::FusedIterator for DeclarationIter {}

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
        let len = u32::try_from(nodes.len()).expect("declaration list length exceeds u32");
        if len <= 1 {
            return nodes
                .first()
                .copied()
                .map_or_else(Declarations::new, Declarations::single);
        }
        let start = self.push_nodes(nodes);
        self.end = start + len;
        Declarations::span(start, len)
    }

    /// `list` with `node` appended (addDeclarationToSymbol): in place when the
    /// list is the last one issued and its room has space, else a copy.
    pub fn append(&mut self, list: Declarations, node: NodeId) -> Declarations {
        if list.len == 0 {
            return Declarations::single(node);
        }
        if list.len == 1 {
            return self.alloc(&[list.node_or_start(), node]);
        }
        let start = list.node_or_start().index();
        if start + list.len == self.end {
            let position = self.items.push(Tagged(node));
            if position == self.end {
                self.end += 1;
                return Declarations::span(start, list.len + 1);
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
