use super::*;

fn tree_file(tree: Vec<TreeNodeRef>) -> FileDump {
    FileDump {
        name: "recovery.ts".to_owned(),
        parse_diagnostics: Vec::new(),
        declarations: Vec::new(),
        tree,
    }
}

#[test]
fn private_symbol_ids_are_wildcarded_without_hiding_the_name() {
    assert_eq!(
        normalize_private_symbol_name("__#76@#method"),
        "__#*@#method"
    );
    assert_eq!(normalize_private_symbol_name("ordinary"), "ordinary");
    assert_eq!(normalize_private_symbol_name("__#x@name"), "__#x@name");
}

#[test]
fn shape_boundaries_preserve_inside_edge_outside_classes() {
    assert_eq!(shape_boundary_class("before"), "outside");
    assert_eq!(shape_boundary_class("after"), "outside");
    assert_eq!(shape_boundary_class("at-start"), "edge");
    assert_eq!(shape_boundary_class("at-end"), "edge");
    assert_eq!(shape_boundary_class("before-body"), "inside");
    assert_eq!(shape_boundary_class("in-body"), "inside");
}

#[test]
fn region_tree_requires_an_exact_root_and_preserves_preorder_subtree() {
    let file = tree_file(vec![
        TreeNodeRef {
            kind: 308,
            pos: 0,
            end: 20,
            missing: false,
            depth: 0,
        },
        TreeNodeRef {
            kind: 263,
            pos: 2,
            end: 12,
            missing: false,
            depth: 1,
        },
        TreeNodeRef {
            kind: 80,
            pos: 4,
            end: 5,
            missing: false,
            depth: 2,
        },
        TreeNodeRef {
            kind: 1,
            pos: 12,
            end: 12,
            missing: false,
            depth: 1,
        },
    ]);

    assert_eq!(
        region_tree(&file, 2, 12).unwrap(),
        vec![
            RegionTreeNode {
                kind: 263,
                relative_pos: 0,
                relative_end: 10,
                missing: false,
                depth: 0,
            },
            RegionTreeNode {
                kind: 80,
                relative_pos: 2,
                relative_end: 3,
                missing: false,
                depth: 1,
            },
        ]
    );
    assert!(region_tree(&file, 3, 12).is_err());
}

#[test]
fn recovery_fingerprints_are_exactly_sixteen_hex_digits() {
    assert!(is_fingerprint("0123456789abcdef"));
    assert!(is_fingerprint("ABCDEF0123456789"));
    assert!(!is_fingerprint("0123456789abcde"));
    assert!(!is_fingerprint("0123456789abcdeg"));
}

#[test]
fn shape_search_uses_only_shared_nonempty_tree_ranges() {
    let node = |pos, end| TreeNodeRef {
        kind: 80,
        pos,
        end,
        missing: pos == end,
        depth: 0,
    };
    let rust = tree_file(vec![node(0, 20), node(2, 12), node(4, 4)]);
    let oracle = tree_file(vec![node(0, 20), node(3, 12), node(4, 4)]);

    assert_eq!(
        exact_region_ranges(&rust, &oracle),
        BTreeSet::from([(0, 20)])
    );
}
