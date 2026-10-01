use super::sort_types_like_tsgo;

/// The tsgo stable sort port orders like a stable sort for a total order
/// on every block-size path (below, at and well above the 20-element
/// block) and keeps equal keys in their input order.
#[test]
fn the_tsgo_stable_sort_matches_a_stable_sort_on_total_orders() {
    for n in [0usize, 1, 2, 3, 19, 20, 21, 40, 41, 100, 257] {
        // A deterministic pseudo-random sequence of (key, tag) pairs.
        let mut state = 0x9E37_79B9_u32.wrapping_add(n as u32);
        let items: Vec<(u32, usize)> = (0..n)
            .map(|index| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 7, index)
            })
            .collect();
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        let mut actual = items.clone();
        sort_types_like_tsgo(&mut actual, |a, b| a.0 < b.0);
        assert_eq!(actual, expected, "n = {n}");
    }
}

/// An inconsistent comparison (a cycle) must not panic and must give the
/// order tsgo's algorithm gives: for three elements, one insertion-sort
/// pass over the input order.
#[test]
fn the_tsgo_stable_sort_tolerates_a_cyclic_comparison() {
    // a < b, b < c, c < a.
    let less = |x: char, y: char| matches!((x, y), ('a', 'b') | ('b', 'c') | ('c', 'a'));
    let mut data = ['a', 'b', 'c'];
    sort_types_like_tsgo(&mut data, less);
    assert_eq!(data, ['a', 'b', 'c']);
    let mut data = ['c', 'b', 'a'];
    sort_types_like_tsgo(&mut data, less);
    assert_eq!(data, ['b', 'c', 'a']);
}
