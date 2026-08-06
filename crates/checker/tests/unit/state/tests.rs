use tsc_types::{CompilerOptions, SymbolFlags, TypeSystemPropertyName};

use super::test_support::with_program_state;
use super::ResolutionTarget;
use crate::links::LinkSlot;

#[test]
fn resolution_stack_flags_same_target_same_property_cycles() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let s = state
            .binder
            .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
        assert!(
            state.push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE)
        );
        // Same (target, kind) again: a cycle — every entry from the
        // cycle start is flagged false.
        assert!(
            !state.push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE)
        );
        assert!(!state.pop_type_resolution());
    });
}

#[test]
fn resolution_stack_distinguishes_property_names() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let s = state
            .binder
            .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
        assert!(
            state.push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE)
        );
        // One symbol can be mid-resolution for Type while safely
        // resolving DeclaredType (checker-foundations §1.2).
        assert!(state.push_type_resolution(
            ResolutionTarget::Symbol(s),
            TypeSystemPropertyName::DECLARED_TYPE
        ));
        assert!(state.pop_type_resolution());
        assert!(state.pop_type_resolution());
    });
}

#[test]
fn resolution_stack_resolved_intermediate_breaks_cycle_scan() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let s = state
            .binder
            .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
        let u = state
            .binder
            .create_symbol(SymbolFlags::PROPERTY, "u".to_owned());
        assert!(
            state.push_type_resolution(ResolutionTarget::Symbol(u), TypeSystemPropertyName::TYPE)
        );
        assert!(
            state.push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE)
        );
        // s's Type resolves while both are on the stack: the scan
        // stops at the first entry whose property is already
        // resolved (resolutionTargetHasProperty), so re-pushing u
        // is NOT a cycle.
        let any = state.tables.intrinsics.any;
        state
            .links
            .set_symbol_type(state.speculation_depth, s, LinkSlot::Resolved(any));
        assert!(
            state.push_type_resolution(ResolutionTarget::Symbol(u), TypeSystemPropertyName::TYPE)
        );
        assert!(state.pop_type_resolution());
        assert!(state.pop_type_resolution());
        assert!(state.pop_type_resolution());
    });
}
