//! One word that holds either a small inline payload or a shared, reference-counted value.
//!
//! A record that needs "a few bits for the common small case, a shared structure for the rare large one" can keep both in
//! eight bytes: an odd word carries 63 payload bits, an even word is the pointer of an `Arc` allocation. The `Arc` itself is
//! never handed out, so no `Weak` can exist and a strong count of one means this handle is the only one.

use std::fmt;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::Arc;

/// Sixty-three inline bits, or one handle of a shared `T`.
pub struct InlineOrShared<T> {
    /// Odd address: `(bits << 1) | 1`, without provenance. Even address: `Arc::into_raw` of the shared value.
    word: *const T,
    marker: PhantomData<Arc<T>>,
}

const _: () = {
    assert!(std::mem::size_of::<InlineOrShared<Vec<u32>>>() == 8);
    assert!(std::mem::size_of::<usize>() == 8);
};

// SAFETY: a handle is an `Arc<T>` or plain bits; it has the thread requirements of `Arc<T>`.
unsafe impl<T: Send + Sync> Send for InlineOrShared<T> {}
// SAFETY: shared access exposes `&T` only.
unsafe impl<T: Send + Sync> Sync for InlineOrShared<T> {}

/// What an [`InlineOrShared`] holds.
pub enum InlineOrSharedView<'a, T> {
    Inline(u64),
    Shared(&'a T),
}

impl<T> InlineOrShared<T> {
    /// The inline payload `bits`, which must fit in 63 bits.
    #[inline]
    pub fn inline(bits: u64) -> Self {
        assert!(bits >> 63 == 0, "an inline payload has 63 bits");
        assert!(
            std::mem::align_of::<T>() >= 2,
            "a shared value's address must be even"
        );
        Self {
            word: std::ptr::without_provenance(((bits << 1) | 1) as usize),
            marker: PhantomData,
        }
    }

    /// A new shared value with this one handle.
    #[inline]
    pub fn shared(value: T) -> Self {
        assert!(
            std::mem::align_of::<T>() >= 2,
            "a shared value's address must be even"
        );
        Self {
            word: Arc::into_raw(Arc::new(value)),
            marker: PhantomData,
        }
    }

    #[inline]
    fn is_inline(&self) -> bool {
        self.word.addr() & 1 == 1
    }

    #[inline]
    pub fn view(&self) -> InlineOrSharedView<'_, T> {
        if self.is_inline() {
            InlineOrSharedView::Inline((self.word.addr() >> 1) as u64)
        } else {
            // SAFETY: an even word is the pointer of a live `Arc<T>` allocation that this handle keeps alive.
            InlineOrSharedView::Shared(unsafe { &*self.word })
        }
    }

    /// The inline payload, if this is one.
    #[inline]
    pub fn inline_bits(&self) -> Option<u64> {
        match self.view() {
            InlineOrSharedView::Inline(bits) => Some(bits),
            InlineOrSharedView::Shared(_) => None,
        }
    }

    /// The shared value, if this is a handle of one.
    #[inline]
    pub fn as_shared(&self) -> Option<&T> {
        match self.view() {
            InlineOrSharedView::Inline(_) => None,
            InlineOrSharedView::Shared(value) => Some(value),
        }
    }

    /// Whether both hold the same payload bits or are handles of the same shared value.
    #[inline]
    pub fn same(&self, other: &Self) -> bool {
        self.word.addr() == other.word.addr()
    }

    /// The address of the shared value (its identity for accounting), if this is a handle of one.
    #[inline]
    pub fn shared_address(&self) -> Option<usize> {
        (!self.is_inline()).then(|| self.word.addr())
    }

    /// How many handles the shared value has (zero for an inline payload).
    pub fn handles(&self) -> usize {
        if self.is_inline() {
            return 0;
        }
        // SAFETY: the word is the pointer of a live `Arc<T>`; the temporary is not dropped.
        let arc = ManuallyDrop::new(unsafe { Arc::from_raw(self.word) });
        Arc::strong_count(&arc)
    }

    /// The shared value for writing, when this is its only handle.
    #[inline]
    pub fn unique_mut(&mut self) -> Option<&mut T> {
        if self.handles() != 1 {
            return None;
        }
        // SAFETY: the `Arc` is never handed out, so no `Weak` exists and one strong handle — this one, borrowed
        // exclusively — is the only path to the value; no other handle can appear while the borrow lasts.
        Some(unsafe { &mut *self.word.cast_mut() })
    }

    /// The shared value for writing; another handle's value is copied into a new allocation first.
    pub fn make_unique(&mut self) -> Option<&mut T>
    where
        T: Clone,
    {
        let value = self.as_shared()?;
        if self.handles() != 1 {
            *self = Self::shared(value.clone());
        }
        self.unique_mut()
    }
}

impl<T> Clone for InlineOrShared<T> {
    #[inline]
    fn clone(&self) -> Self {
        if !self.is_inline() {
            // SAFETY: the word is the pointer of a live `Arc<T>`; the new handle owns the added count.
            unsafe { Arc::increment_strong_count(self.word) };
        }
        Self {
            word: self.word,
            marker: PhantomData,
        }
    }
}

impl<T> Drop for InlineOrShared<T> {
    #[inline]
    fn drop(&mut self) {
        if !self.is_inline() {
            // SAFETY: this handle owns one strong count of the allocation the word points to.
            drop(unsafe { Arc::from_raw(self.word) });
        }
    }
}

impl<T> Default for InlineOrShared<T> {
    /// The inline payload zero.
    #[inline]
    fn default() -> Self {
        Self::inline(0)
    }
}

impl<T: fmt::Debug> fmt::Debug for InlineOrShared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.view() {
            InlineOrSharedView::Inline(bits) => write!(f, "Inline({bits:#x})"),
            InlineOrSharedView::Shared(value) => f.debug_tuple("Shared").field(value).finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Debug)]
    struct Counted(Arc<AtomicUsize>, u64);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn inline_payloads_round_trip_and_own_nothing() {
        for bits in [0, 1, 0x7f, u64::MAX >> 1] {
            let word = InlineOrShared::<Vec<u32>>::inline(bits);
            assert_eq!(word.inline_bits(), Some(bits));
            assert!(word.as_shared().is_none());
            assert_eq!(word.handles(), 0);
            assert_eq!(word.shared_address(), None);
            let copy = word.clone();
            assert!(copy.same(&word));
        }
        let mut word = InlineOrShared::<Vec<u32>>::default();
        assert_eq!(word.inline_bits(), Some(0));
        assert!(word.unique_mut().is_none());
        assert!(word.make_unique().is_none());
    }

    #[test]
    fn shared_values_are_counted_written_only_when_unique_and_dropped_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut first = InlineOrShared::shared(Counted(drops.clone(), 7));
        assert_eq!(first.inline_bits(), None);
        assert_eq!(first.handles(), 1);
        first.unique_mut().unwrap().1 = 8;
        let mut second = first.clone();
        assert!(second.same(&first));
        assert_eq!(first.handles(), 2);
        assert!(first.unique_mut().is_none());
        assert_eq!(second.as_shared().unwrap().1, 8);
        // Writing through a shared handle copies the value; the other handle keeps its own.
        second.make_unique().unwrap().1 = 9;
        assert!(!second.same(&first));
        assert_eq!((first.handles(), second.handles()), (1, 1));
        assert_eq!(first.as_shared().unwrap().1, 8);
        assert_eq!(second.as_shared().unwrap().1, 9);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(first);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let third = second.clone();
        drop(second);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(third);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn handles_move_between_threads() {
        let word = InlineOrShared::shared(vec![1u32, 2, 3]);
        let other = word.clone();
        let sum = std::thread::spawn(move || other.as_shared().unwrap().iter().sum::<u32>())
            .join()
            .unwrap();
        assert_eq!(sum, 6);
        assert_eq!(word.handles(), 1);
    }
}
