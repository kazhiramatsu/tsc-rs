use tsc_types::{CompilerOptions, SymbolFlags, TypeSystemPropertyName};

use super::test_support::with_program_state;
use super::ResolutionTarget;
use crate::links::LinkSlot;

#[test]
fn diagnostic_deduplication_uses_js_values_before_utf8_output() {
    use tsc_diagnostics::{gen, JsString};

    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let lead = JsString::from_code_units(&[0xd800]);
        let other = JsString::from_code_units(&[0xd801]);
        let replacement = JsString::from("\u{fffd}");
        let before = state.diagnostics.len();
        let first = state.error_at_js(None, &gen::Duplicate_identifier_0, &[lead.as_js()]);
        let second = state.error_at_js(None, &gen::Duplicate_identifier_0, &[other.as_js()]);
        let third = state.error_at_js(None, &gen::Duplicate_identifier_0, &[replacement.as_js()]);
        assert_eq!([first, second, third], [before, before + 1, before + 2]);
        // The output encoding agrees, but neither reporting path may use it
        // to merge the different message values.
        assert_eq!(
            state.diagnostics[first].message.text.to_string_lossy(),
            state.diagnostics[second].message.text.to_string_lossy()
        );
        assert_eq!(
            state.diagnostics[first].message.text.to_string_lossy(),
            state.diagnostics[third].message.text.to_string_lossy()
        );
        assert_eq!(
            state.error_at_js(None, &gen::Duplicate_identifier_0, &[lead.as_js()]),
            first
        );
        assert_eq!(
            state.lookup_or_issue_error_js(None, &gen::Duplicate_identifier_0, &[other.as_js()]),
            second
        );
        assert_eq!(
            state.error_at(None, &gen::Duplicate_identifier_0, &["\u{fffd}"]),
            third
        );
        assert_eq!(state.diagnostics.len(), before + 3);
    });
}

#[test]
fn resolution_stack_flags_same_target_same_property_cycles() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let s = state.binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_escaped_value(("s".to_owned()).into()),
        );
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
        let s = state.binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_escaped_value(("s".to_owned()).into()),
        );
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
        let s = state.binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_escaped_value(("s".to_owned()).into()),
        );
        let u = state.binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_escaped_value(("u".to_owned()).into()),
        );
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
