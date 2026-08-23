//! Byte-parity contracts for shared helper texts against the vendored
//! TypeScript helper declarations.
//!
//! The recipe is the landed `typescript:read` port's: take the
//! declaration's template literal, drop the leading newline, and strip
//! the common indentation. `read` itself is the first table row, so the
//! recipe is proven against corpus-validated bytes before it vouches
//! for the four H2.5h-b substrate helpers. Exact span identity (line
//! range to content hash) is owned by the ledger d2 check; this suite
//! asserts the declaration boundary shape, the emitted text bytes, and
//! the metadata fields the owners will rely on.

use std::fs;
use std::path::PathBuf;

use crate::EmitHelper;

struct PinnedHelper {
    factory: fn() -> EmitHelper,
    name: &'static str,
    upstream_variable: &'static str,
    priority: Option<u8>,
    span_start: usize,
    span_end: usize,
}

const PINNED: &[PinnedHelper] = &[
    PinnedHelper {
        factory: super::read,
        name: "typescript:read",
        upstream_variable: "readHelper",
        priority: None,
        span_start: 26_258,
        span_end: 26_279,
    },
    PinnedHelper {
        factory: super::extends,
        name: "typescript:extends",
        upstream_variable: "extendsHelper",
        priority: Some(0),
        span_start: 26_224,
        span_end: 26_246,
    },
    PinnedHelper {
        factory: super::assign,
        name: "typescript:assign",
        upstream_variable: "assignHelper",
        priority: Some(1),
        span_start: 26_122,
        span_end: 26_139,
    },
    PinnedHelper {
        factory: super::make_template_object,
        name: "typescript:makeTemplateObject",
        upstream_variable: "templateObjectHelper",
        priority: Some(0),
        span_start: 26_247,
        span_end: 26_257,
    },
    PinnedHelper {
        factory: super::spread_array,
        name: "typescript:spreadArray",
        upstream_variable: "spreadArrayHelper",
        priority: None,
        span_start: 26_280,
        span_end: 26_294,
    },
    PinnedHelper {
        factory: super::values,
        name: "typescript:values",
        upstream_variable: "valuesHelper",
        priority: None,
        span_start: 26_314,
        span_end: 26_330,
    },
    PinnedHelper {
        factory: super::generator,
        name: "typescript:generator",
        upstream_variable: "generatorHelper",
        priority: Some(6),
        span_start: 26_331,
        span_end: 26_364,
    },
];

fn vendored_implementation() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib/_tsc.js");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn declaration_slice(text: &str, start: usize, end: usize) -> String {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    assert!(
        start >= 1 && end <= lines.len() && start <= end,
        "span {start}-{end} outside the vendored file"
    );
    lines[start - 1..end].concat()
}

fn template_text(slice: &str) -> &str {
    let opening = slice
        .find("text: `")
        .expect("declaration slice carries a template text field")
        + "text: `".len();
    let closing = slice[opening..]
        .find('`')
        .expect("template literal is closed inside the slice");
    &slice[opening..opening + closing]
}

fn dedent(template: &str) -> String {
    let body = template
        .strip_prefix('\n')
        .expect("helper template starts with a newline");
    let base = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .expect("helper template has non-blank lines");
    body.lines()
        .map(|line| {
            if line.trim().is_empty() {
                line
            } else {
                &line[base..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn helper_texts_match_the_vendored_declarations() {
    let vendored = vendored_implementation();
    for pinned in PINNED {
        let slice = declaration_slice(&vendored, pinned.span_start, pinned.span_end);
        let first = slice.lines().next().expect("slice has a first line");
        assert_eq!(
            first,
            format!("var {} = {{", pinned.upstream_variable),
            "{} span start drifted",
            pinned.name
        );
        assert_eq!(
            slice.lines().last().expect("slice has a last line"),
            "};",
            "{} span end drifted",
            pinned.name
        );
        assert!(
            slice.contains(&format!("name: \"{}\"", pinned.name)),
            "{} declaration lost its name field",
            pinned.name
        );

        let helper = (pinned.factory)();
        assert_eq!(helper.name(), pinned.name);
        assert!(!helper.scoped(), "{} must stay unscoped", pinned.name);
        assert_eq!(
            helper.priority(),
            pinned.priority,
            "{} priority drifted",
            pinned.name
        );
        assert!(
            helper.dependencies().is_empty(),
            "{} must not declare dependencies",
            pinned.name
        );
        assert_eq!(
            helper.text().expect("pinned helpers carry text"),
            dedent(template_text(&slice)),
            "{} text is not byte-equal to the vendored declaration",
            pinned.name
        );
    }
}
