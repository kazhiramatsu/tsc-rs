use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

fn kinds(text: &str, javascript_file: bool) -> Vec<AssignmentDeclarationKind> {
    let source = parse_source_file(
        if javascript_file { "a.js" } else { "a.ts" },
        text,
        ParseOptions {
            javascript_file,
            ..ParseOptions::default()
        },
        None,
    );
    source
        .arena
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            matches!(
                node.kind,
                SyntaxKind::BinaryExpression | SyntaxKind::CallExpression
            )
        })
        .map(|(index, _)| get_assignment_declaration_kind(&source, NodeId(index as u32)))
        .filter(|kind| *kind != AssignmentDeclarationKind::None)
        .collect()
}

#[test]
fn assignment_kind_matrix_matches_tsc_6_0_3() {
    let text = "\
exports.x = 1;
module.exports = {};
C.prototype.x = function () {};
this.x = 1;
F.x = 1;
C.prototype = {};
Object.defineProperty(F, \"x\", { value: 1 });
Object.defineProperty(exports, \"x\", { value: 1 });
Object.defineProperty(C.prototype, \"x\", { value: 1 });
";
    assert_eq!(
        kinds(text, true),
        [
            AssignmentDeclarationKind::ExportsProperty,
            AssignmentDeclarationKind::ModuleExports,
            AssignmentDeclarationKind::PrototypeProperty,
            AssignmentDeclarationKind::ThisProperty,
            AssignmentDeclarationKind::Property,
            AssignmentDeclarationKind::Prototype,
            AssignmentDeclarationKind::ObjectDefinePropertyValue,
            AssignmentDeclarationKind::ObjectDefinePropertyExports,
            AssignmentDeclarationKind::ObjectDefinePrototypeProperty,
        ]
    );
    assert_eq!(kinds(text, false), [AssignmentDeclarationKind::Property]);
}

#[test]
fn void_zero_and_dynamic_export_edges_match_tsc() {
    assert!(kinds("F.x = void 0;", true).is_empty());
    assert_eq!(
        kinds("F[key] = 1;", true),
        [AssignmentDeclarationKind::Property]
    );
    assert_eq!(
        kinds("exports[key] = 1;", true),
        [AssignmentDeclarationKind::Property]
    );
}
