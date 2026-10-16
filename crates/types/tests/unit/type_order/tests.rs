use super::{sort_func_like_go, sort_types_like_tsgo};

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

/// The Go `slices.SortFunc` port gives Go's order, equal elements included
/// (pdqsort is not stable), on each of its paths: insertion sort, the median
/// of three and Tukey's ninther, a decreasing run reversed, a nearly sorted
/// run finished by partial insertion sort, and partitions of many duplicates.
/// The expected orders are Go 1.27.1's output for the same inputs.
#[test]
fn the_go_sort_port_gives_go_s_order() {
    let mut keyed: Vec<i64> = (0..100).collect();
    sort_func_like_go(&mut keyed, |x, y| ((x * 37) % 10).cmp(&((y * 37) % 10)));
    assert_eq!(
        keyed,
        [
            10, 80, 0, 30, 60, 20, 40, 70, 90, 50, 93, 33, 63, 13, 3, 83, 73, 43, 53, 23, 66, 86,
            96, 36, 76, 56, 26, 16, 46, 6, 29, 79, 99, 89, 69, 9, 19, 59, 49, 39, 72, 62, 42, 2,
            92, 32, 12, 82, 52, 22, 25, 15, 95, 75, 85, 55, 45, 35, 65, 5, 8, 28, 38, 18, 78, 58,
            98, 88, 68, 48, 41, 51, 31, 11, 1, 71, 91, 61, 81, 21, 4, 64, 94, 44, 84, 54, 14, 24,
            74, 34, 37, 87, 7, 17, 77, 27, 67, 97, 57, 47,
        ]
    );

    let mut random = Vec::new();
    let mut state = 2_463_534_242_u32;
    for _ in 0..300 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        random.push(i64::from(state % 1000));
    }
    sort_func_like_go(&mut random, |x, y| (x / 10).cmp(&(y / 10)));
    assert_eq!(
        random,
        [
            4, 5, 14, 19, 29, 28, 23, 30, 35, 40, 55, 56, 65, 63, 70, 76, 82, 89, 87, 82, 86, 91,
            97, 95, 92, 92, 115, 119, 113, 115, 122, 129, 120, 126, 123, 133, 143, 148, 154, 159,
            153, 156, 164, 160, 162, 160, 182, 183, 184, 186, 195, 192, 194, 201, 201, 200, 200,
            216, 210, 218, 211, 218, 220, 226, 228, 224, 220, 236, 235, 233, 236, 244, 240, 243,
            246, 243, 249, 252, 266, 267, 262, 261, 272, 275, 274, 284, 280, 280, 282, 299, 293,
            304, 319, 319, 317, 325, 327, 333, 331, 335, 333, 349, 349, 344, 348, 360, 367, 366,
            368, 379, 373, 370, 376, 381, 392, 391, 401, 402, 407, 416, 410, 414, 417, 414, 418,
            424, 422, 422, 438, 443, 443, 448, 457, 455, 465, 467, 473, 489, 486, 491, 490, 491,
            502, 504, 517, 517, 522, 526, 535, 546, 540, 541, 545, 551, 554, 567, 568, 563, 571,
            571, 577, 585, 581, 592, 603, 607, 609, 606, 602, 614, 615, 623, 628, 632, 636, 630,
            639, 631, 638, 633, 646, 649, 640, 649, 655, 660, 660, 661, 676, 675, 681, 687, 690,
            698, 707, 701, 703, 707, 710, 717, 714, 715, 720, 725, 727, 734, 731, 738, 748, 744,
            748, 742, 747, 748, 756, 752, 754, 767, 764, 763, 769, 766, 767, 760, 772, 781, 784,
            788, 781, 793, 793, 790, 806, 806, 800, 819, 813, 815, 829, 824, 829, 821, 829, 822,
            837, 846, 840, 843, 853, 850, 859, 860, 863, 861, 863, 861, 867, 869, 872, 870, 879,
            884, 882, 880, 902, 906, 906, 909, 929, 925, 924, 924, 922, 936, 931, 935, 936, 943,
            942, 942, 940, 952, 958, 951, 952, 951, 968, 967, 968, 974, 979, 976, 970, 971, 985,
            985, 998, 991, 995, 999,
        ]
    );

    let mut decreasing: Vec<i64> = (0..120).rev().collect();
    sort_func_like_go(&mut decreasing, |x, y| (x / 4).cmp(&(y / 4)));
    assert_eq!(
        decreasing,
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 51, 55, 52, 53, 54, 58, 59, 57, 56, 61, 60, 62, 63, 67, 64, 65, 66,
            68, 71, 69, 70, 74, 72, 73, 75, 76, 77, 78, 79, 83, 80, 81, 82, 86, 87, 85, 84, 90, 88,
            89, 91, 92, 93, 94, 95, 99, 96, 97, 98, 100, 101, 102, 103, 107, 104, 105, 106, 108,
            109, 110, 111, 112, 113, 114, 115, 119, 116, 117, 118,
        ]
    );

    let mut nearly: Vec<i64> = (0..80).collect();
    nearly.swap(10, 11);
    nearly.swap(50, 53);
    sort_func_like_go(&mut nearly, |x, y| (x / 3).cmp(&(y / 3)));
    assert_eq!(
        nearly,
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 53, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
            68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
        ]
    );
}

/// A comparison that is no total order (tsgo's auto-import fix comparison
/// compares some fixes only against their own kind) must not panic, as the
/// standard sorts may, and must give Go's order: here rock-paper-scissors on
/// the value modulo 3, Go 1.27.1's output.
#[test]
fn the_go_sort_port_tolerates_a_comparison_with_no_total_order() {
    let mut cyclic: Vec<i64> = (0..60).map(|index| (index * 7) % 60).collect();
    sort_func_like_go(&mut cyclic, |x, y| match (x % 3 - y % 3 + 3) % 3 {
        1 => std::cmp::Ordering::Less,
        2 => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    assert_eq!(
        cyclic,
        [
            27, 39, 18, 21, 57, 36, 42, 15, 54, 3, 33, 12, 24, 51, 30, 45, 0, 9, 6, 48, 23, 35, 53,
            41, 32, 14, 2, 59, 11, 50, 38, 20, 44, 29, 8, 5, 17, 56, 26, 47, 52, 10, 19, 1, 58, 49,
            22, 31, 43, 37, 4, 28, 40, 16, 55, 25, 13, 7, 46, 34,
        ]
    );
}
