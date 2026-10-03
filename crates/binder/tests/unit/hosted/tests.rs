use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

fn parse_js(text: &str) -> SourceFile {
    parse_source_file(
        "a.js",
        text,
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    )
}

/// The first node of `kind` whose text (after leading trivia) starts with
/// `prefix`.
fn find(source: &SourceFile, kind: SyntaxKind, prefix: &str) -> NodeId {
    let text = source.text();
    source
        .arena
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| {
            let start = tsc_syntax::skip_trivia(text, node.pos as usize);
            node.kind == kind && text[start..node.end as usize].starts_with(prefix)
        })
        .map(|(index, _)| NodeId::new(index as u32))
        .unwrap_or_else(|| panic!("no {kind:?} starting with {prefix:?}"))
}

fn text_of(source: &SourceFile, node: NodeId) -> &str {
    let node = source.arena.node(node);
    let start = tsc_syntax::skip_trivia(source.text(), node.pos as usize);
    &source.text()[start..node.end as usize]
}

#[test]
fn hosted_tags_land_where_tsgo_reparses_them() {
    // reparser.go reparseHosted (346-613): `@type` on a variable statement
    // types its first untyped declaration; `@template`, `@param` and
    // `@return` go to the function (a bracketed `@param` also makes the
    // parameter optional); modifier tags become the member's modifiers; a
    // parenthesized `@type` casts the expression; a `@type` that no
    // declaration takes types the whole function (FullSignature).
    let text = "\
/** @type {number} */
var a = 1, b;
/**
 * @template T
 * @param {T} x
 * @param {string} [y]
 * @return {T}
 */
function f(x, y) { return x; }
class C {
  /** @readonly @private */
  m() {}
}
const g = /** @type {() => void} */ (() => {});
/** @type {(n: number) => void} */
function h(n) {}
";
    let source = parse_js(text);
    let hosted = jsdoc_hosted(&source);

    let a = find(&source, SyntaxKind::VariableDeclaration, "a");
    let b = find(&source, SyntaxKind::VariableDeclaration, "b");
    assert_eq!(
        hosted.type_of(a).map(|t| text_of(&source, t)),
        Some("number")
    );
    assert_eq!(hosted.type_of(b), None);

    let f = find(&source, SyntaxKind::FunctionDeclaration, "function f");
    let template = find(&source, SyntaxKind::JSDocTemplateTag, "@template");
    let list = hosted
        .type_parameters_of(f)
        .expect("reparsed type parameters");
    assert_eq!(list.tags, [template]);
    let template_node = source.arena.node(template);
    assert_eq!((list.pos, list.end), (template_node.pos, template_node.end));
    assert_eq!(hosted.type_of(f).map(|t| text_of(&source, t)), Some("T"));

    let x = find(&source, SyntaxKind::Parameter, "x");
    let y = find(&source, SyntaxKind::Parameter, "y");
    let param_x = find(&source, SyntaxKind::JSDocParameterTag, "@param {T} x");
    let param_y = find(
        &source,
        SyntaxKind::JSDocParameterTag,
        "@param {string} [y]",
    );
    assert_eq!(hosted.matched_parameter_of(param_x), Some(x));
    assert_eq!(hosted.matched_parameter_of(param_y), Some(y));
    assert_eq!(hosted.type_of(x).map(|t| text_of(&source, t)), Some("T"));
    assert_eq!(
        hosted.type_of(y).map(|t| text_of(&source, t)),
        Some("string")
    );
    assert_eq!(hosted.question_token_of(x), None);
    assert_eq!(hosted.question_token_of(y), Some(param_y));

    let m = find(&source, SyntaxKind::MethodDeclaration, "m()");
    let readonly = find(&source, SyntaxKind::JSDocReadonlyTag, "@readonly");
    let private = find(&source, SyntaxKind::JSDocPrivateTag, "@private");
    assert_eq!(hosted.modifier_tags_of(m), [readonly, private]);

    let arrow = find(&source, SyntaxKind::ArrowFunction, "() => {}");
    let cast = hosted.cast_of(arrow).expect("reparsed cast");
    assert!(cast.is_assertion);
    assert_eq!(text_of(&source, cast.type_node), "() => void");

    let h = find(&source, SyntaxKind::FunctionDeclaration, "function h");
    assert_eq!(
        hosted.full_signature_of(h).map(|t| text_of(&source, t)),
        Some("(n: number) => void")
    );
    assert_eq!(hosted.type_of(h), None);
}

#[test]
fn typescript_files_have_no_hosted_tags() {
    // tsgo reparses JSDoc only in JavaScript files (jsdoc.go withJSDoc).
    let source = parse_source_file(
        "a.ts",
        "/** @type {number} */\nvar a;\n",
        ParseOptions::default(),
        None,
    );
    assert_eq!(jsdoc_hosted(&source), &JsDocHosted::default());
}
