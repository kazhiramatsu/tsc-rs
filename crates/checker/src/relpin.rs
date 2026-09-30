//! Relation probe: declares a source and a target type in a scratch
//! program, resolves both annotations, and asks the relation engine
//! whether they are assignable or comparable.
//!
//! The harness's relation-pin test (`crates/harness/tests/integration/
//! relation_pins.rs`) compares the answers with TypeScript's recorded
//! ones; the checker's unit tests reuse the scratch-program helpers.

use tsc_syntax::{NodeData, NodeId, SourceFile};
use tsc_types::{CompilerOptions, TypeId};

use crate::state::CheckerState;

/// Which relation a pin exercises: the two checkTypeRelatedTo entry
/// relations a fixture can observe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelpinRelation {
    Assignable,
    Comparable,
}

/// One relation question (a pin from pins/relations.toml).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelpinQuery<'a> {
    /// Prelude declarations bound into the scratch program before the
    /// probe vars (recursive `interface A { next: B }` pins live here).
    pub setup: &'a str,
    /// Source type annotation text.
    pub source: &'a str,
    /// Target type annotation text.
    pub target: &'a str,
    /// True when the pin supplies `expr` (the fixture assigns a literal
    /// expression, so the source type is the FRESH literal type; the
    /// probe takes the fresh variant of the resolved source type).
    pub source_is_fresh: bool,
    pub relation: RelpinRelation,
    pub options: &'a CompilerOptions,
}

/// The engine's answer for one pin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelpinVerdict {
    Related,
    NotRelated,
    /// The scratch program or an annotation could not be resolved; the
    /// pin test counts this as a failure.
    Unavailable {
        reason: String,
    },
}

/// Parse and bind the scratch program, resolve both annotations, and ask
/// the relation engine.
/// tsrs-native: harness and unit-test entry; no tsc counterpart.
pub fn probe_relation(query: &RelpinQuery) -> RelpinVerdict {
    let mut text = String::new();
    if !query.setup.is_empty() {
        text.push_str(query.setup);
        if !query.setup.ends_with('\n') {
            text.push('\n');
        }
    }
    text.push_str(&format!("declare var __relpin_source: {};\n", query.source));
    text.push_str(&format!("declare var __relpin_target: {};\n", query.target));

    let source_file = tsc_syntax::parse_source_file(
        "relpin.ts".to_owned(),
        text,
        tsc_syntax::ParseOptions {
            language_variant: tsc_syntax::LanguageVariant::Standard,
            javascript_file: false,
            ..tsc_syntax::ParseOptions::default()
        },
        None,
    );
    if !source_file.parse_diagnostics.is_empty() {
        return RelpinVerdict::Unavailable {
            reason: format!(
                "scratch program has parse errors (first: TS{})",
                source_file.parse_diagnostics[0].code()
            ),
        };
    }
    let binder = tsc_binder::bind_source_file(&source_file, query.options);
    let mut state = CheckerState::new(&source_file, &binder, query.options);

    let Some(source_annotation) = find_probe_annotation(&source_file, "__relpin_source") else {
        return RelpinVerdict::Unavailable {
            reason: "probe source annotation not found in scratch program".to_owned(),
        };
    };
    let Some(target_annotation) = find_probe_annotation(&source_file, "__relpin_target") else {
        return RelpinVerdict::Unavailable {
            reason: "probe target annotation not found in scratch program".to_owned(),
        };
    };

    let source_type = match state.get_type_from_type_node(source_annotation) {
        Ok(ty) => ty,
        Err(err) => {
            return RelpinVerdict::Unavailable {
                reason: format!("source type: {err}"),
            }
        }
    };
    let target_type = match state.get_type_from_type_node(target_annotation) {
        Ok(ty) => ty,
        Err(err) => {
            return RelpinVerdict::Unavailable {
                reason: format!("target type: {err}"),
            }
        }
    };
    // expr pins: the fixture assigns a literal EXPRESSION, so the
    // engine must see the checkExpression-shaped FRESH type — fresh
    // literal variants for freshable literals, FreshLiteral|
    // ObjectLiteral object flags for object literals (excess-property
    // checking keys on them; the probe checks no expression).
    let source_type = if query.source_is_fresh {
        mark_fresh_probe_source(&mut state, source_type)
    } else {
        source_type
    };

    let related = match query.relation {
        RelpinRelation::Assignable => state.is_type_assignable_to(source_type, target_type),
        // The comparable fixture is an as-assertion: its legality is
        // checkAssertionDeferred's two-step comparable formula, not a
        // single isTypeComparableTo call.
        RelpinRelation::Comparable => assertion_is_legal(&mut state, source_type, target_type),
    };
    match related {
        Ok(true) => RelpinVerdict::Related,
        Ok(false) => RelpinVerdict::NotRelated,
        Err(err) => RelpinVerdict::Unavailable {
            reason: err.to_string(),
        },
    }
}

/// The comparable fixture `s as Target` reports 2352 iff neither
/// comparable(target, widened(exprType)) nor comparable(exprType, target)
/// holds, where exprType =
/// getRegularTypeOfObjectLiteral(getBaseTypeOfLiteralType(source))
/// (checkAssertionDeferred, _tsc.js:77939-77955). A declared probe source
/// never requires widening, so the widened type is exprType itself.
/// tsrs-native: probe projection of checkAssertionDeferred.
fn assertion_is_legal(
    state: &mut CheckerState,
    source: TypeId,
    target: TypeId,
) -> crate::state::CheckResult<bool> {
    let expr_type = state.get_base_type_of_literal_type(source)?;
    let expr_type = state.get_regular_type_of_object_literal(expr_type)?;
    let first = state.is_type_comparable_to(target, expr_type);
    if let Ok(true) = first {
        return Ok(true);
    }
    let second = state.is_type_comparable_to(expr_type, target);
    if let Ok(true) = second {
        return Ok(true);
    }
    first?;
    second?;
    Ok(false)
}

/// checkExpression's freshness for the probe's expression pins.
fn mark_fresh_probe_source(state: &mut CheckerState, ty: TypeId) -> TypeId {
    use tsc_types::{ObjectFlags, TypeFlags};
    if state.tables.flags_of(ty).intersects(TypeFlags::FRESHABLE) {
        return state.tables.get_fresh_type_of_literal_type(ty);
    }
    if state.tables.flags_of(ty).intersects(TypeFlags::OBJECT)
        && state
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::ANONYMOUS)
    {
        let flags = state.tables.object_flags_of(ty).bits()
            | ObjectFlags::OBJECT_LITERAL.bits()
            | ObjectFlags::FRESH_LITERAL.bits();
        state.tables.type_mut(ty).object_flags = tsc_types::ObjectFlags::from_bits(flags);
        state.tables.type_mut(ty).fresh_type = Some(ty);
        // A REAL object literal's symbol is a VALUE symbol whose
        // valueDeclaration is the literal node, and every own
        // property's declaration parent is that node — which is what
        // shouldCheckAsExcessProperty (65411) keys on. The probe's
        // annotation-derived __type symbol has no value declaration
        // (TypeLiteral is not a value flag), so the shim installs the
        // type-literal node as one; own members' parents already
        // match.
        if let Some(symbol) = state.tables.type_of(ty).symbol {
            let original = state.binder.symbol(symbol);
            if original.value_declaration.is_none() {
                // File binders are read-only post-bind (shared lib
                // bundles enforce it structurally), so the shim clones
                // the __type symbol into a TRANSIENT twin carrying the
                // value declaration and re-points the probe type at
                // it; member symbols stay the originals, whose
                // declaration parents already match the literal node.
                let flags = original.flags;
                let escaped_name = original.escaped_name;
                let declarations = original.declarations.clone();
                let parent = original.parent;
                let members = original.members().clone();
                let clone = state.binder.create_symbol(flags, escaped_name);
                let clone_data = state.binder.symbol_mut(clone);
                clone_data.value_declaration = declarations.first().copied();
                clone_data.declarations = declarations;
                clone_data.parent = parent;
                *clone_data.members_mut() = members;
                state.tables.type_mut(ty).symbol = Some(clone);
            }
        }
    }
    ty
}

/// The scratch program is generated above: find the declared probe
/// var's type annotation by the identifier's raw text (escapedText
/// would carry the leading-underscore escape).
/// tsrs-native: relpin scratch-program lookup helper; no tsc
/// counterpart.
pub(crate) fn find_probe_annotation(source: &SourceFile, name: &str) -> Option<NodeId> {
    for index in 0..source.arena.len() {
        let node = source.arena.node(NodeId::new(index as u32));
        let NodeData::VariableDeclaration(data) = &node.data else {
            continue;
        };
        let Some(declared_name) = data.name else {
            continue;
        };
        let NodeData::Identifier(identifier) = &source.arena.node(declared_name).data else {
            continue;
        };
        if identifier.text() == name {
            return data.r#type;
        }
    }
    None
}
