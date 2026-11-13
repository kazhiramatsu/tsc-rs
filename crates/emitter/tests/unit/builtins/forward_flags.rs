//! Parsed flags are an exact table, including discarded records and arrays.
use crate::{TransformArena, TransformSourceId};
use tsc_syntax::{parse_source_file, ParseOptions, SourceFile};

fn compare(mut arena: TransformArena, source: TransformSourceId, label: &str) -> TransformArena {
    let root = arena.root(source).unwrap().node();
    let mut reference = arena.clone();
    super::compute_transform_flags_reference(&mut reference, source, root).unwrap();
    super::compute_transform_flags_forward(&mut arena, source, root).unwrap();
    let actual = super::snapshot_transform_flags(&arena, source).unwrap();
    let expected = super::snapshot_transform_flags(&reference, source).unwrap();
    for (category, (actual, expected)) in ["node", "array"]
        .into_iter()
        .zip([(&actual.0, &expected.0), (&actual.1, &expected.1)])
    {
        let differences: Vec<_> = actual
            .iter()
            .zip(expected)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(8)
            .collect();
        assert!(
            differences.is_empty(),
            "{label}: {category} flags differ: {differences:?}"
        );
    }
    arena
}

fn parsed(file: &SourceFile, label: &str) {
    let mut arena = TransformArena::new();
    let source = arena.add_source(file, None);
    compare(arena, source, label);
}

#[test]
fn forward_flags_match_every_record_and_array_on_recovery_and_contextual_fixtures() {
    let fixtures = [
        "/** @type {Promise<number>} */ const value = 1; /** @see Other */ class C {}",
        "@dec class C { @dec [this.x] = this; static [this.y] = this; declare x: number; abstract m(): void; }",
        "class C extends B { constructor<T>(this: X, public x: number): X { super(); } get value<T>(this: X): number { return super.x; } set value<T>(this: X, value: number): X { super.x = value; } }",
        "class C { constructor<T>(); get x<T>(): number; set x<T>(x: number): void; }",
        "({ ...rest, [this.key]: 1, get [this.other]() { return this; } }); const { a, ...r } = obj; ([a, ...b] = x); ({a, ...b} = x);",
        "const f = async function* () { yield await import('x'); }; const a = async () => this; class C extends B { m() { return super.m(); } } ++a; a--; a ||= b; a ??= b; a **= b;",
        "const \\u{61} = 1; export = x; import a = A.B; import b = require('b'); import type c = require('c'); function f(this, x?) {}",
        "foo<T>(x); foo < T > x; <T>(x: T) => x; class C { @dec } await (await x); ({ x: y = 1 }); const a = <div a={x}>{this}</div>;",
    ];
    for (index, text) in fixtures.iter().enumerate() {
        let file = parse_source_file(
            format!("fixture-{index}.tsx"),
            *text,
            ParseOptions {
                script_kind: tsc_syntax::ScriptKind::Tsx,
                node_id_base: 10000 + index as u32 * 10000,
                node_array_id_base: 2000 + index as u32 * 2000,
                ..Default::default()
            },
            None,
        );
        parsed(&file, &format!("fixture {index}"));
    }
}

fn corpus_files() -> Vec<String> {
    let list = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/emitter-flag-oracle-files.txt");
    let files: Vec<_> = std::fs::read_to_string(list)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    assert!(!files.is_empty());
    files
}

#[test]
#[ignore = "requires target/emitter-flag-oracle-files.txt; checks every node and array"]
fn forward_flags_match_the_old_classifier_on_corpus_files() {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(|| {
            let mut nodes = 0usize;
            let files = corpus_files();
            for path in &files {
                let text = std::fs::read_to_string(path).unwrap();
                let file = parse_source_file(
                    path.as_str(),
                    text,
                    ParseOptions {
                        script_kind: tsc_syntax::ScriptKind::ensure_from_file_name(path),
                        js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseForTypeErrors,
                        ..Default::default()
                    },
                    None,
                );
                nodes += file.arena.len();
                parsed(&file, path);
            }
            eprintln!(
                "flag oracle: {} files, {nodes} records, every node and array identical",
                files.len()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_node_flag_edit_preserves_unread_array_aggregates() {
    let file = parse_source_file(
        "deferred.ts",
        "const values = [this, await value, 1n];",
        Default::default(),
        None,
    );
    let mut actual = TransformArena::new();
    let source = actual.add_source(&file, None);
    let root = actual.root(source).unwrap().node();
    let mut reference = actual.clone();
    super::compute_transform_flags_reference(&mut reference, source, root).unwrap();
    super::compute_transform_flags_forward(&mut actual, source, root).unwrap();
    let array = (file.arena.array_base()..file.arena.array_end())
        .find_map(|id| {
            let id = tsc_syntax::NodeArrayId::new(id);
            let array = reference.node_array_ref(source, id)?;
            (!reference.node_array(array).unwrap().nodes.is_empty()
                && reference.array_transform_flags(array) != crate::TransformFlags::NONE)
                .then_some(array)
        })
        .unwrap();
    let before = reference.array_transform_flags(array);
    let child = crate::TransformNode::new(source, actual.node_array(array).unwrap().nodes[0]);
    let changed = crate::TransformFlags::from_bits(!actual.transform_flags(child).bits());
    actual.set_transform_flags(child, changed);
    reference.set_transform_flags(child, changed);
    assert_eq!(actual.array_transform_flags(array), before);
    assert_eq!(
        super::snapshot_transform_flags(&actual, source).unwrap(),
        super::snapshot_transform_flags(&reference, source).unwrap()
    );
}

#[test]
fn an_explicit_array_override_and_a_cloned_session_keep_their_values() {
    let file = parse_source_file(
        "override.ts",
        "const values = [this, await value];",
        Default::default(),
        None,
    );
    let mut actual = TransformArena::new();
    let source = actual.add_source(&file, None);
    let root = actual.root(source).unwrap().node();
    super::compute_transform_flags_forward(&mut actual, source, root).unwrap();
    let mut cloned = actual.clone();
    let array = (file.arena.array_base()..file.arena.array_end())
        .find_map(|id| {
            let array = actual.node_array_ref(source, tsc_syntax::NodeArrayId::new(id))?;
            (!actual.node_array(array).unwrap().nodes.is_empty()).then_some(array)
        })
        .unwrap();
    let before = cloned.array_transform_flags(array);
    let override_flags = crate::TransformFlags::from_bits(!before.bits());
    actual.set_array_transform_flags(array, override_flags);
    let child = crate::TransformNode::new(source, actual.node_array(array).unwrap().nodes[0]);
    actual.set_transform_flags(child, crate::TransformFlags::NONE);
    assert_eq!(actual.array_transform_flags(array), override_flags);
    assert_eq!(cloned.array_transform_flags(array), before);
    cloned.clear_session_metadata();
    assert_eq!(
        cloned.array_transform_flags(array),
        crate::TransformFlags::NONE
    );
}

#[test]
fn reclassification_replaces_only_the_new_walks_array_values() {
    let file = parse_source_file(
        "again.ts",
        "const values = [this, await value];",
        Default::default(),
        None,
    );
    let mut actual = TransformArena::new();
    let source = actual.add_source(&file, None);
    let root = actual.root(source).unwrap().node();
    super::compute_transform_flags_forward(&mut actual, source, root).unwrap();
    let mut reference = actual.clone();
    super::compute_transform_flags_reference(&mut reference, source, root).unwrap();
    super::compute_transform_flags_forward(&mut actual, source, root).unwrap();
    assert_eq!(
        super::snapshot_transform_flags(&actual, source).unwrap(),
        super::snapshot_transform_flags(&reference, source).unwrap()
    );
}
