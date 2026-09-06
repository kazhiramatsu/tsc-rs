//! W5 checker regressions use complete frozen upstream observations.

use super::h2_7b_w4a_controls::{assert_every_row, assert_frozen_observation};

#[test]
fn any_base_singleton_does_not_become_a_static_declaration_index() {
    assert_frozen_observation(
        "typescript-6.0.3/compiler/declarationEmitClassInherritsAny.ts#default",
    );
}

#[test]
fn tuple_lengths_are_allocated_when_the_target_is_created() {
    assert_frozen_observation(
        "typescript-6.0.3/conformance/types/tuple/variadicTuples1.ts#default",
    );
}

#[test]
fn cached_jsdoc_overloads_survive_indirect_comment_hosts() {
    assert_every_row(&[
        "typescript-6.0.3/compiler/jsFileMethodOverloads4.ts#default",
        "typescript-6.0.3/compiler/jsFileMethodOverloads5.ts#default",
    ]);
}

#[test]
fn negative_literal_types_check_the_positive_operand_first() {
    assert_frozen_observation(
        "typescript-6.0.3/compiler/arrayFlatNoCrashInferenceDeclarations.ts#default",
    );
}
