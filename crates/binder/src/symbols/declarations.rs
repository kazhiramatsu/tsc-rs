use tsc_store::SmallList;
use tsc_syntax::NodeId;

/// A symbol's declaration list. Almost every symbol has one or two, and the
/// checker copies the list into every property symbol it instantiates
/// (millions in a large program), so up to two stay inline and only longer
/// lists allocate.
#[derive(Clone)]
pub struct Declarations(SmallList<NodeId>);

const _: () = assert!(std::mem::size_of::<Declarations>() == 16);

impl Declarations {
    /// Bytes these declarations own on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.0.heap_bytes()
    }

    pub const fn new() -> Self {
        Self(SmallList::new())
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn push(&mut self, node: NodeId) {
        self.0.push(node);
    }
}

impl Default for Declarations {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Declarations {
    type Target = [NodeId];
    #[inline]
    fn deref(&self) -> &[NodeId] {
        &self.0
    }
}

impl std::ops::DerefMut for Declarations {
    #[inline]
    fn deref_mut(&mut self) -> &mut [NodeId] {
        &mut self.0
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
        Self(SmallList::from(nodes))
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
pub type IntoIter = tsc_store::SmallListIter<NodeId>;

impl IntoIterator for Declarations {
    type Item = NodeId;
    type IntoIter = IntoIter;
    fn into_iter(self) -> IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Declarations {
    type Item = &'a NodeId;
    type IntoIter = std::slice::Iter<'a, NodeId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
