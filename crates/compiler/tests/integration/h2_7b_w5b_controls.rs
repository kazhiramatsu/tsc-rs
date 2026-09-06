//! W5 NodeBuilder and emitter regressions use complete frozen observations.

use super::h2_7b_w4a_controls::{assert_every_row, assert_frozen_observation};

#[test]
fn synthesized_accessor_lists_keep_each_members_comments() {
    assert_every_row(&[
        "typescript-6.0.3/compiler/accessorInAmbientContextES5.ts#target%3Des5",
        "typescript-6.0.3/compiler/commentsClassMembers.ts#target%3Des5",
        "typescript-6.0.3/compiler/declarationEmitClassMemberNameConflict.ts#target%3Des5",
    ]);
}

#[test]
fn private_names_follow_transformed_class_provenance() {
    assert_frozen_observation(
        "typescript-6.0.3/compiler/privateFieldsInClassExpressionDeclaration.ts#default",
    );
}

#[test]
fn import_type_reuses_its_literal_child() {
    assert_frozen_observation(
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsUniqueSymbolUsage.ts#default",
    );
}
