use tsc_syntax::NodeId;

/// Declarations held without a heap allocation.
const INLINE: usize = 3;

/// A symbol's declaration list. Almost every symbol has one or two, and the
/// checker copies the list into every property symbol it instantiates
/// (millions in a large program), so up to three stay inline in the same
/// 24 bytes a `Vec` takes and only longer lists allocate.
#[derive(Clone)]
pub struct Declarations(Repr);

#[derive(Clone)]
enum Repr {
    Inline { len: u8, nodes: [NodeId; INLINE] },
    Heap(Vec<NodeId>),
}

const _: () = assert!(std::mem::size_of::<Declarations>() == std::mem::size_of::<Vec<NodeId>>());

impl Declarations {
    /// Bytes these declarations own on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        match &self.0 {
            Repr::Inline { .. } => 0,
            Repr::Heap(nodes) => nodes.capacity() * std::mem::size_of::<NodeId>(),
        }
    }

    pub const fn new() -> Self {
        Self(Repr::Inline {
            len: 0,
            nodes: [NodeId(0); INLINE],
        })
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn push(&mut self, node: NodeId) {
        match &mut self.0 {
            Repr::Inline { len, nodes } if usize::from(*len) < INLINE => {
                nodes[usize::from(*len)] = node;
                *len += 1;
            }
            Repr::Inline { nodes, .. } => {
                let mut heap = Vec::with_capacity(INLINE * 2);
                heap.extend_from_slice(nodes);
                heap.push(node);
                self.0 = Repr::Heap(heap);
            }
            Repr::Heap(heap) => heap.push(node),
        }
    }
}

impl Default for Declarations {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Declarations {
    type Target = [NodeId];
    fn deref(&self) -> &[NodeId] {
        match &self.0 {
            Repr::Inline { len, nodes } => &nodes[..usize::from(*len)],
            Repr::Heap(heap) => heap,
        }
    }
}

impl std::ops::DerefMut for Declarations {
    fn deref_mut(&mut self) -> &mut [NodeId] {
        match &mut self.0 {
            Repr::Inline { len, nodes } => &mut nodes[..usize::from(*len)],
            Repr::Heap(heap) => heap,
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

impl From<Vec<NodeId>> for Declarations {
    fn from(nodes: Vec<NodeId>) -> Self {
        if nodes.len() <= INLINE {
            nodes.into_iter().collect()
        } else {
            Self(Repr::Heap(nodes))
        }
    }
}

impl From<&[NodeId]> for Declarations {
    fn from(nodes: &[NodeId]) -> Self {
        nodes.iter().copied().collect()
    }
}

impl FromIterator<NodeId> for Declarations {
    fn from_iter<T: IntoIterator<Item = NodeId>>(iter: T) -> Self {
        let mut declarations = Self::new();
        declarations.extend(iter);
        declarations
    }
}

impl Extend<NodeId> for Declarations {
    fn extend<T: IntoIterator<Item = NodeId>>(&mut self, iter: T) {
        for node in iter {
            self.push(node);
        }
    }
}

impl<const N: usize> PartialEq<[NodeId; N]> for Declarations {
    fn eq(&self, other: &[NodeId; N]) -> bool {
        **self == other[..]
    }
}

impl PartialEq<Vec<NodeId>> for Declarations {
    fn eq(&self, other: &Vec<NodeId>) -> bool {
        **self == other[..]
    }
}

/// The declarations by value, in order.
pub enum IntoIter {
    Inline(std::iter::Take<std::array::IntoIter<NodeId, INLINE>>),
    Heap(std::vec::IntoIter<NodeId>),
}

impl Iterator for IntoIter {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        match self {
            Self::Inline(nodes) => nodes.next(),
            Self::Heap(nodes) => nodes.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Inline(nodes) => nodes.size_hint(),
            Self::Heap(nodes) => nodes.size_hint(),
        }
    }
}

impl DoubleEndedIterator for IntoIter {
    fn next_back(&mut self) -> Option<NodeId> {
        match self {
            Self::Inline(nodes) => nodes.next_back(),
            Self::Heap(nodes) => nodes.next_back(),
        }
    }
}

impl ExactSizeIterator for IntoIter {}

impl IntoIterator for Declarations {
    type Item = NodeId;
    type IntoIter = IntoIter;
    fn into_iter(self) -> IntoIter {
        match self.0 {
            Repr::Inline { len, nodes } => {
                IntoIter::Inline(nodes.into_iter().take(usize::from(len)))
            }
            Repr::Heap(nodes) => IntoIter::Heap(nodes.into_iter()),
        }
    }
}

impl<'a> IntoIterator for &'a Declarations {
    type Item = &'a NodeId;
    type IntoIter = std::slice::Iter<'a, NodeId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
