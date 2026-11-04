//! Go slice algorithms used where comparator call order matters.
//!
//! Keep the pinned Go algorithms together; the old `type_order` exports
//! remain available to existing callers.

use std::cmp::Ordering;

/// Go's `slices.BinarySearchFunc`: lower bound, then a final equality
/// comparison at that bound. Equal elements return their first index.
#[inline]
pub fn binary_search_func<E, T: ?Sized>(
    data: &[E],
    target: &T,
    mut cmp: impl FnMut(&E, &T) -> Ordering,
) -> (usize, bool) {
    let (mut i, mut j) = (0, data.len());
    while i < j {
        let h = usize::midpoint(i, j);
        if cmp(&data[h], target) == Ordering::Less {
            i = h + 1;
        } else {
            j = h;
        }
    }
    (
        i,
        i < data.len() && cmp(&data[i], target) == Ordering::Equal,
    )
}

/// tsgo-port: slices.SortFunc @go1.27 (`pdqsortCmpFunc` in
/// `slices/zsortanyfunc.go`): pattern-defeating quicksort, with insertion
/// sort for 12 elements or fewer and heapsort after too many bad pivots.
/// tsgo sorts with it comparisons that are not a total order (auto-import
/// fixes compare some kinds only against their own kind), so the same
/// algorithm gives tsgo's order and never panics on an inconsistent
/// comparison as the standard sorts may.
pub fn sort_func_like_go<T>(data: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let n = data.len();
    // bits.Len(uint(n))
    let limit = (usize::BITS - n.leading_zeros()) as usize;
    pdqsort_go(data, 0, n, limit, &mut cmp);
}

/// Fallible Go sort. After the first error no further comparisons are
/// evaluated; the local slice's order on error is unspecified.
pub fn try_sort_func_like_go<T, E>(
    data: &mut [T],
    mut cmp: impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E> {
    let mut error = None;
    sort_func_like_go(data, |a, b| {
        if error.is_some() {
            return Ordering::Equal;
        }
        match cmp(a, b) {
            Ok(order) => order,
            Err(abort) => {
                error = Some(abort);
                Ordering::Equal
            }
        }
    });
    error.map_or(Ok(()), Err)
}

/// Go's `cmp(data[i], data[j]) < 0`.
fn go_less<T>(data: &[T], i: usize, j: usize, cmp: &mut impl FnMut(&T, &T) -> Ordering) -> bool {
    cmp(&data[i], &data[j]) == Ordering::Less
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortedHint {
    Unknown,
    Increasing,
    Decreasing,
}

fn pdqsort_go<T>(
    data: &mut [T],
    mut a: usize,
    mut b: usize,
    mut limit: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    const MAX_INSERTION: usize = 12;
    // Whether the last partitioning was reasonably balanced, and whether the
    // slice was already partitioned.
    let mut was_balanced = true;
    let mut was_partitioned = true;
    loop {
        let length = b - a;
        if length <= MAX_INSERTION {
            insertion_sort_go(data, a, b, cmp);
            return;
        }
        // Fall back to heapsort if too many bad choices were made.
        if limit == 0 {
            heap_sort_go(data, a, b, cmp);
            return;
        }
        // If the last partitioning was imbalanced, break patterns.
        if !was_balanced {
            break_patterns_go(data, a, b);
            limit -= 1;
        }
        let (mut pivot, mut hint) = choose_pivot_go(data, a, b, cmp);
        if hint == SortedHint::Decreasing {
            data[a..b].reverse();
            // The pivot was pivot-a elements after the start; after the
            // reversal it is pivot-a elements before the end.
            pivot = (b - 1) - (pivot - a);
            hint = SortedHint::Increasing;
        }
        // The slice is likely already sorted.
        if was_balanced
            && was_partitioned
            && hint == SortedHint::Increasing
            && partial_insertion_sort_go(data, a, b, cmp)
        {
            return;
        }
        // Probably many duplicate elements: partition into the elements
        // equal to the pivot and the elements greater than it.
        if a > 0 && !go_less(data, a - 1, pivot, cmp) {
            a = partition_equal_go(data, a, b, pivot, cmp);
            continue;
        }
        let (mid, already_partitioned) = partition_go(data, a, b, pivot, cmp);
        was_partitioned = already_partitioned;
        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort_go(data, a, mid, limit, cmp);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort_go(data, mid + 1, b, limit, cmp);
            b = mid;
        }
    }
}

/// Go `insertionSortCmpFunc`: `data[a..b]` by insertion.
fn insertion_sort_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && go_less(data, j, j - 1, cmp) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

/// Go `siftDownCmpFunc`: the heap property on `data[lo..hi]` of the heap
/// rooted at `first`.
fn sift_down_go<T>(
    data: &mut [T],
    lo: usize,
    hi: usize,
    first: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && go_less(data, first + child, first + child + 1, cmp) {
            child += 1;
        }
        if !go_less(data, first + root, first + child, cmp) {
            return;
        }
        data.swap(first + root, first + child);
        root = child;
    }
}

/// Go `heapSortCmpFunc`.
fn heap_sort_go<T>(data: &mut [T], a: usize, b: usize, cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    let first = a;
    let hi = b - a;
    if hi == 0 {
        return;
    }
    // Build the heap with the greatest element at the top.
    for i in (0..=(hi - 1) / 2).rev() {
        sift_down_go(data, i, hi, first, cmp);
    }
    // Pop the elements, largest first, into the end of data.
    for i in (0..hi).rev() {
        data.swap(first, first + i);
        sift_down_go(data, 0, i, first, cmp);
    }
}

/// Go `partitionCmpFunc`: one quicksort partition around `data[pivot]`;
/// the pivot's new index and whether the range was already partitioned.
fn partition_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    pivot: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> (usize, bool) {
    data.swap(a, pivot);
    // i and j are inclusive of the elements remaining to be partitioned.
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    while i <= j && go_less(data, i as usize, a, cmp) {
        i += 1;
    }
    while i <= j && !go_less(data, j as usize, a, cmp) {
        j -= 1;
    }
    if i > j {
        data.swap(j as usize, a);
        return (j as usize, true);
    }
    data.swap(i as usize, j as usize);
    i += 1;
    j -= 1;
    loop {
        while i <= j && go_less(data, i as usize, a, cmp) {
            i += 1;
        }
        while i <= j && !go_less(data, j as usize, a, cmp) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    data.swap(j as usize, a);
    (j as usize, false)
}

/// Go `partitionEqualCmpFunc`: the elements equal to `data[pivot]`, then
/// the greater ones (none is smaller).
fn partition_equal_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    pivot: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    data.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    loop {
        while i <= j && !go_less(data, a, i as usize, cmp) {
            i += 1;
        }
        while i <= j && go_less(data, a, j as usize, cmp) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    i as usize
}

/// Go `partialInsertionSortCmpFunc`: a few adjacent out-of-order pairs
/// shifted into place; whether the range ends sorted.
fn partial_insertion_sort_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> bool {
    // The maximum number of adjacent out-of-order pairs shifted, and no
    // shifting on short arrays.
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;
    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !go_less(data, i, i - 1, cmp) {
            i += 1;
        }
        if i == b {
            return true;
        }
        if b - a < SHORTEST_SHIFTING {
            return false;
        }
        data.swap(i, i - 1);
        // Shift the smaller one to the left (Go stops at index 1, not at
        // `a + 1`).
        if i - a >= 2 {
            let mut j = i - 1;
            while j >= 1 {
                if !go_less(data, j, j - 1, cmp) {
                    break;
                }
                data.swap(j, j - 1);
                j -= 1;
            }
        }
        // Shift the greater one to the right.
        if b - i >= 2 {
            let mut j = i + 1;
            while j < b {
                if !go_less(data, j, j - 1, cmp) {
                    break;
                }
                data.swap(j, j - 1);
                j += 1;
            }
        }
    }
    false
}

/// Go `breakPatternsCmpFunc`: three elements around the middle swapped with
/// pseudo-random ones (an xorshift seeded by the length).
fn break_patterns_go<T>(data: &mut [T], a: usize, b: usize) {
    let length = b - a;
    if length >= 8 {
        let mut random = length as u64;
        // nextPowerOfTwo: 1 << bits.Len(uint(length))
        let modulus = 1usize << (usize::BITS - length.leading_zeros());
        let middle = a + (length / 4) * 2;
        for index in middle - 1..=middle + 1 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut other = (random as usize) & (modulus - 1);
            if other >= length {
                other -= length;
            }
            data.swap(index, a + other);
        }
    }
}

/// Go `choosePivotCmpFunc`: a static pivot below 8 elements, the median of
/// three below 50, else Tukey's ninther; with how sorted the range looks.
fn choose_pivot_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> (usize, SortedHint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;
    let l = b - a;
    let mut swaps = 0;
    let mut i = a + l / 4;
    let mut j = a + l / 4 * 2;
    let mut k = a + l / 4 * 3;
    if l >= 8 {
        if l >= SHORTEST_NINTHER {
            i = median_go(data, i - 1, i, i + 1, &mut swaps, cmp);
            j = median_go(data, j - 1, j, j + 1, &mut swaps, cmp);
            k = median_go(data, k - 1, k, k + 1, &mut swaps, cmp);
        }
        // The median among i, j and k, into j.
        j = median_go(data, i, j, k, &mut swaps, cmp);
    }
    match swaps {
        0 => (j, SortedHint::Increasing),
        MAX_SWAPS => (j, SortedHint::Decreasing),
        _ => (j, SortedHint::Unknown),
    }
}

/// Go `medianCmpFunc` (with `order2CmpFunc`): the index of the median of
/// `data[a]`, `data[b]` and `data[c]`, counting the swaps made.
fn median_go<T>(
    data: &[T],
    a: usize,
    b: usize,
    c: usize,
    swaps: &mut usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    let mut order2 = |x: usize, y: usize, swaps: &mut usize| {
        if go_less(data, y, x, cmp) {
            *swaps += 1;
            (y, x)
        } else {
            (x, y)
        }
    };
    let (a, b) = order2(a, b, swaps);
    let (b, _c) = order2(b, c, swaps);
    let (_a, b) = order2(a, b, swaps);
    b
}

/// tsgo-port: slices.SortStableFunc @go1.26 (`stableCmpFunc` in
/// `slices/zsortanyfunc.go`): insertion sort of 20-element blocks, then
/// SymMerge passes. tsgo sorts a union's members with it
/// (`addTypesToUnion`), and `compareTypes` is not a total order in every
/// case (two same-named aliases compare by their members, which can cycle
/// with the alias-argument comparison), so the same algorithm gives the
/// same order as tsgo where a different stable sort could not, and never
/// panics on an inconsistent comparison as the standard sorts may.
pub fn sort_types_like_tsgo<T: Copy>(data: &mut [T], mut less: impl FnMut(T, T) -> bool) {
    sort_stable_func_like_go(data, |a, b| {
        if less(*a, *b) {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    });
}

/// Go stable sort for arbitrary payloads, with the same comparison and swap
/// sequence as the Copy/less adapter above.
pub fn sort_stable_func_like_go<T>(data: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let mut less = |a: &T, b: &T| cmp(a, b).is_lt();
    let n = data.len();
    let mut block = 20;
    let (mut a, mut b) = (0, block);
    while b <= n {
        insertion_sort(data, a, b, &mut less);
        a = b;
        b += block;
    }
    insertion_sort(data, a, n, &mut less);
    while block < n {
        let (mut a, mut b) = (0, 2 * block);
        while b <= n {
            sym_merge(data, a, a + block, b, &mut less);
            a = b;
            b += 2 * block;
        }
        let m = a + block;
        if m < n {
            sym_merge(data, a, m, n, &mut less);
        }
        block *= 2;
    }
}

fn insertion_sort<T>(data: &mut [T], a: usize, b: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && less(&data[j], &data[j - 1]) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn sym_merge<T>(
    data: &mut [T],
    a: usize,
    m: usize,
    b: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    if m - a == 1 {
        // Insert data[a] into data[m..b]: the lowest i with data[i] >= data[a].
        let (mut i, mut j) = (m, b);
        while i < j {
            let h = (i + j) >> 1;
            if less(&data[h], &data[a]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        for k in a..i.saturating_sub(1) {
            data.swap(k, k + 1);
        }
        return;
    }
    if b - m == 1 {
        // Insert data[m] into data[a..m]: the lowest i with data[i] > data[m].
        let (mut i, mut j) = (a, m);
        while i < j {
            let h = (i + j) >> 1;
            if !less(&data[m], &data[h]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        let mut k = m;
        while k > i {
            data.swap(k, k - 1);
            k -= 1;
        }
        return;
    }
    let mid = (a + b) >> 1;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = (start + r) >> 1;
        if !less(&data[p - c], &data[c]) {
            start = c + 1;
        } else {
            r = c;
        }
    }
    let end = n - start;
    if start < m && m < end {
        rotate(data, start, m, end);
    }
    if a < start && start < mid {
        sym_merge(data, a, start, mid, less);
    }
    if mid < end && end < b {
        sym_merge(data, mid, end, b, less);
    }
}

fn rotate<T>(data: &mut [T], a: usize, m: usize, b: usize) {
    let (mut i, mut j) = (m - a, b - m);
    while i != j {
        if i > j {
            swap_range(data, m - i, m, j);
            i -= j;
        } else {
            swap_range(data, m - i, m + j - i, i);
            j -= i;
        }
    }
    swap_range(data, m - i, m, i);
}

fn swap_range<T>(data: &mut [T], a: usize, b: usize, n: usize) {
    for i in 0..n {
        data.swap(a + i, b + i);
    }
}
