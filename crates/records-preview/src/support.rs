//! Integer codecs used by generated records. All stored representations accept all bits.
use crate::arena_stub::{Arena, ArenaCell, Bytes, List, Record, Ref, Span};
use crate::nodes::Node;
use crate::records::*;
use std::marker::PhantomData;

/// Nullable 32-bit arena identity. Zero is absence, never an invalid Rust value.
#[repr(transparent)]
pub struct Id<T: Record> {
    raw: u32,
    marker: PhantomData<&'static T>,
}
impl<T: Record> Copy for Id<T> {}
impl<T: Record> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> Default for Id<T> {
    fn default() -> Self {
        Self::from_raw(0)
    }
}
impl<T: Record> PartialEq for Id<T> {
    fn eq(&self, b: &Self) -> bool {
        self.raw == b.raw
    }
}
impl<T: Record> Eq for Id<T> {}
impl<T: Record> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.raw.fmt(f)
    }
}
unsafe impl<T: Record> Record for Id<T> {}
impl<T: Record> Id<T> {
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }
    pub fn from_ref(r: Ref<T>) -> Self {
        Self::from_raw(r.raw())
    }
    #[inline]
    pub fn raw(self) -> u32 {
        self.raw
    }
    #[inline]
    pub fn as_ref(self) -> Option<Ref<T>> {
        Ref::from_raw(self.raw)
    }
    pub fn is_none(self) -> bool {
        self.raw == 0
    }
}

/// 0 vacant; MAX resolving; MAX-1 resolved(None); other words resolved handle.
/// The two top raw arena handles cannot enter slots and are rejected on writes.
#[repr(transparent)]
pub struct Slot<T: Record>(Id<T>);
impl<T: Record> Copy for Slot<T> {}
impl<T: Record> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> Default for Slot<T> {
    fn default() -> Self {
        Self(Id::default())
    }
}
impl<T: Record> PartialEq for Slot<T> {
    fn eq(&self, b: &Self) -> bool {
        self.0 == b.0
    }
}
impl<T: Record> Eq for Slot<T> {}
impl<T: Record> std::fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
unsafe impl<T: Record> Record for Slot<T> {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState<T: Record> {
    Vacant,
    Resolving,
    Resolved(Id<T>),
}
impl<T: Record> Slot<T> {
    pub fn encode(state: SlotState<T>) -> Self {
        Self(Id::from_raw(match state {
            SlotState::Vacant => 0,
            SlotState::Resolving => u32::MAX,
            SlotState::Resolved(id) if id.is_none() => u32::MAX - 1,
            SlotState::Resolved(id) => {
                assert!(id.raw() < u32::MAX - 1, "reserved slot handle");
                id.raw()
            }
        }))
    }
    #[inline]
    pub fn state(self) -> SlotState<T> {
        match self.0.raw() {
            0 => SlotState::Vacant,
            u32::MAX => SlotState::Resolving,
            n if n == u32::MAX - 1 => SlotState::Resolved(Id::default()),
            n => SlotState::Resolved(Id::from_raw(n)),
        }
    }
}

/// Two words with explicit optionality: len_plus_one=0 is absent, 1 is present empty.
#[repr(C)]
pub struct OptionalSpan<T: Record> {
    pub start: u32,
    pub len_plus_one: u32,
    marker: PhantomData<&'static T>,
}
impl<T: Record> Copy for OptionalSpan<T> {}
impl<T: Record> Clone for OptionalSpan<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> Default for OptionalSpan<T> {
    fn default() -> Self {
        Self::encode(None)
    }
}
impl<T: Record> PartialEq for OptionalSpan<T> {
    fn eq(&self, b: &Self) -> bool {
        (self.start, self.len_plus_one) == (b.start, b.len_plus_one)
    }
}
impl<T: Record> Eq for OptionalSpan<T> {}
impl<T: Record> std::fmt::Debug for OptionalSpan<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.start, self.len_plus_one).fmt(f)
    }
}
unsafe impl<T: Record> Record for OptionalSpan<T> {}
impl<T: Record> OptionalSpan<T> {
    pub fn encode(value: Option<Span<T>>) -> Self {
        let (start, len_plus_one) = value.map_or((0, 0), |s| {
            (s.start, s.len.checked_add(1).expect("span length overflow"))
        });
        Self {
            start,
            len_plus_one,
            marker: PhantomData,
        }
    }
    #[inline]
    pub fn decode(self) -> Option<Span<T>> {
        self.len_plus_one
            .checked_sub(1)
            .map(|len| Span::from_raw(self.start, len))
    }
}
impl<T: Copy + PartialEq> PartialEq for ArenaCell<T> {
    fn eq(&self, b: &Self) -> bool {
        self.get() == b.get()
    }
}
impl<T: Copy> Clone for ArenaCell<T> {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

/// Tagged literal text reference. Bit31=0 source-span index; bit31=1 arena Bytes.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextRef(pub u32);
unsafe impl Record for TextRef {}
impl TextRef {
    pub const EMPTY: Self = Self(0);
    pub fn source(index: u32) -> Self {
        assert!(index < 1 << 31);
        Self(index)
    }
    pub fn bytes(r: Ref<Bytes>) -> Self {
        assert!(r.raw() < 1 << 31);
        Self((1 << 31) | r.raw())
    }
    pub fn source_index(self) -> Option<u32> {
        (self.0 >> 31 == 0).then_some(self.0)
    }
    pub fn bytes_ref(self) -> Option<Ref<Bytes>> {
        if self.0 >> 31 != 0 {
            Ref::from_raw(self.0 & 0x7fff_ffff)
        } else {
            None
        }
    }
}

pub enum CandidateView<'a> {
    Inline(&'a [Id<Type>]),
    Overflow(std::cell::Ref<'a, [Id<Type>]>),
}
impl std::ops::Deref for CandidateView<'_> {
    type Target = [Id<Type>];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Inline(v) => v,
            Self::Overflow(v) => v,
        }
    }
}
impl CandidatePair {
    pub fn new(arena: &Arena, values: &[Id<Type>]) -> Self {
        assert!(
            values.iter().all(|id| !id.is_none()),
            "candidate handles cannot be absent"
        );
        let mut pair = Self::default();
        for (out, value) in pair.inline.iter_mut().zip(values) {
            *out = *value;
        }
        if values.len() > 2 {
            pair.overflow = OptionalSpan::encode(Some(arena.alloc_slice(values)));
        }
        pair
    }
    #[inline]
    pub fn values<'a>(&'a self, arena: &'a Arena) -> CandidateView<'a> {
        if let Some(span) = self.overflow.decode() {
            CandidateView::Overflow(span.as_slice(arena))
        } else {
            let len = self.inline.iter().take_while(|id| !id.is_none()).count();
            CandidateView::Inline(&self.inline[..len])
        }
    }
    /// Copy the backing too: cloned inference infos must not share mutable candidate lists.
    pub fn deep_clone(&self, arena: &Arena) -> Self {
        let values = self.values(arena).to_vec();
        Self::new(arena, &values)
    }
}
impl TransientSymbol {
    pub const INSTANTIATED: u32 = 1 << 31;
    pub fn name(&self, arena: &Arena) -> u32 {
        arena.get(self.origin.as_ref().expect("origin")).name
    }
    pub fn declarations<'a>(&self, arena: &'a Arena) -> DeclarationView<'a> {
        arena
            .get(self.origin.as_ref().expect("origin"))
            .declarations(arena)
    }
    /// Borrow the original Symbol with no copying of declarations on hot paths.
    #[inline]
    pub fn origin_record<'a>(&self, arena: &'a Arena) -> std::cell::Ref<'a, Symbol> {
        arena.get(self.origin.as_ref().expect("origin"))
    }
}

// Test samples are distinct integer words, independent of record layout offsets.
pub trait Sample {
    fn sample(seed: u32) -> Self;
}
macro_rules! samples { ($($t:ty),*) => { $(impl Sample for $t { fn sample(seed: u32) -> Self { seed as Self } })* }; }
samples!(u8, u16, u32, i32, u64, f64);
impl<T: Record> Sample for Id<T> {
    fn sample(seed: u32) -> Self {
        Self::from_raw(seed)
    }
}
impl<T: Record> Sample for Slot<T> {
    fn sample(seed: u32) -> Self {
        Self::encode(SlotState::Resolved(Id::from_raw(seed)))
    }
}
impl<T: Record> Sample for Span<T> {
    fn sample(seed: u32) -> Self {
        Self::from_raw(seed, seed + 1)
    }
}
impl<T: Record> Sample for OptionalSpan<T> {
    fn sample(seed: u32) -> Self {
        Self::encode(Some(Span::sample(seed)))
    }
}
impl<T: Copy + Sample> Sample for ArenaCell<T> {
    fn sample(seed: u32) -> Self {
        Self::new(T::sample(seed))
    }
}
impl<T: Sample, const N: usize> Sample for [T; N] {
    fn sample(seed: u32) -> Self {
        std::array::from_fn(|i| T::sample(seed + i as u32))
    }
}
const _: () = assert!(std::mem::size_of::<OptionalSpan<u32>>() == 8);
const _: () = assert!(std::mem::size_of::<Slot<u32>>() == 4);
pub type TypeListId = Id<List<Id<Type>>>;

impl<T> Sample for PhantomData<T> {
    fn sample(_: u32) -> Self {
        PhantomData
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MapperData {
    Simple {
        source: Id<Type>,
        target: Id<Type>,
    },
    Array {
        sources: Span<Id<Type>>,
        targets: Option<Span<Id<Type>>>,
    },
    DeferredFixing(Id<InferenceContext>),
    DeferredNonFixing(Id<InferenceContext>),
    DeferredEffective {
        node: Id<Node>,
        sources: Span<Id<Type>>,
    },
    Function(u32),
    Composite(Id<Mapper>, Id<Mapper>),
    Merged(Id<Mapper>, Id<Mapper>),
}
impl Mapper {
    pub fn encode(data: MapperData) -> Self {
        let (kind, a, b, c) = match data {
            MapperData::Simple { source, target } => (0, source.raw(), target.raw(), 0),
            MapperData::Array { sources, targets } => {
                if let Some(targets) = targets {
                    assert_eq!(sources.len, targets.len, "mapper list lengths");
                }
                (
                    1 | if targets.is_some() { 128 } else { 0 },
                    sources.start,
                    targets.map_or(0, |t| t.start),
                    sources.len,
                )
            }
            MapperData::DeferredFixing(id) => (2, id.raw(), 0, 0),
            MapperData::DeferredNonFixing(id) => (2 | 8, id.raw(), 0, 0),
            MapperData::DeferredEffective { node, sources } => {
                (2 | 16, node.raw(), sources.start, sources.len)
            }
            MapperData::Function(tag) => {
                assert!(tag < 5);
                (3, tag, 0, 0)
            }
            MapperData::Composite(a, b) => (4, a.raw(), b.raw(), 0),
            MapperData::Merged(a, b) => (5, a.raw(), b.raw(), 0),
        };
        Self {
            kind,
            padding: [0; 3],
            a,
            b,
            c,
            ..Self::default()
        }
    }
    pub fn decode(&self) -> Option<MapperData> {
        Some(match self.kind {
            0 => MapperData::Simple {
                source: Id::from_raw(self.a),
                target: Id::from_raw(self.b),
            },
            1 | 129 => MapperData::Array {
                sources: Span::from_raw(self.a, self.c),
                targets: if self.kind == 129 {
                    Some(Span::from_raw(self.b, self.c))
                } else {
                    None
                },
            },
            2 => MapperData::DeferredFixing(Id::from_raw(self.a)),
            10 => MapperData::DeferredNonFixing(Id::from_raw(self.a)),
            18 => MapperData::DeferredEffective {
                node: Id::from_raw(self.a),
                sources: Span::from_raw(self.b, self.c),
            },
            3 if self.a < 5 => MapperData::Function(self.a),
            4 => MapperData::Composite(Id::from_raw(self.a), Id::from_raw(self.b)),
            5 => MapperData::Merged(Id::from_raw(self.a), Id::from_raw(self.b)),
            _ => return None,
        })
    }
}

pub struct DeclarationView<'a> {
    first: Id<Node>,
    more: Option<std::cell::Ref<'a, [Id<Node>]>>,
}
impl DeclarationView<'_> {
    pub fn iter(&self) -> impl Iterator<Item = Id<Node>> + '_ {
        std::iter::once(self.first)
            .filter(|id| !id.is_none())
            .chain(self.more.as_deref().unwrap_or(&[]).iter().copied())
    }
}
impl Symbol {
    pub fn declarations<'a>(&self, arena: &'a Arena) -> DeclarationView<'a> {
        DeclarationView {
            first: self.decl0,
            more: self.more.decode().map(|s| s.as_slice(arena)),
        }
    }
}
impl Flow {
    pub fn label(flags: u32, span: Span<Id<Flow>>) -> Self {
        Self {
            flags,
            a: span.start,
            antecedent: Id::default(),
            c: span.len,
        }
    }
    #[inline]
    pub fn label_antecedents(&self) -> Span<Id<Flow>> {
        Span::from_raw(self.a, self.c)
    }
}

impl InferenceInfo {
    pub fn priority(&self) -> Option<i32> {
        (self.flags.get() & 4 != 0).then(|| self.priority.get() as i32)
    }
    pub fn set_priority(&self, value: Option<i32>) {
        self.priority.set(value.unwrap_or(0) as u32);
        self.flags
            .set((self.flags.get() & !4) | if value.is_some() { 4 } else { 0 });
    }
    pub fn implied_arity(&self) -> Option<u32> {
        (self.flags.get() & 8 != 0).then(|| self.implied_arity.get())
    }
    pub fn set_implied_arity(&self, value: Option<usize>) {
        self.implied_arity.set(value.map_or(0, |n| {
            u32::try_from(n).expect("arity exceeds arena length domain")
        }));
        self.flags
            .set((self.flags.get() & !8) | if value.is_some() { 8 } else { 0 });
    }
}
