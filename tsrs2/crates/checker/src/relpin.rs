//! Relation pin probe bridge (greenfield M3 stage 4.0).
//!
//! Relations are not directly observable until the checker exists, so
//! the pin harness (`cargo xtask relpin`) drives the relation engine
//! through this test-only entry: it parses the pin's type annotations
//! in a scratch program, resolves them through the MINIMAL
//! type-from-annotation path (stage 4.1), and asks the relation engine
//! (stages 4.4-4.6). Ground truth comes from oracle probes of
//! `declare var s: Source; var t: Target = s;` fixtures (any semantic
//! diagnostic = not related; comparable pins use `s as Target` and the
//! 2352 family the same way).
//!
//! Stage 4.1 wires the probe through the MINIMAL type-from-annotation
//! path (annotate.rs): both annotations resolve to real types in a
//! scratch program; the relation ANSWER stays unsupported until the
//! engine core lands (stages 4.4-4.5).

use tsrs2_syntax::{NodeData, NodeId, SourceFile};
use tsrs2_types::CompilerOptions;

use crate::state::CheckerState;

/// Which tsc relation a pin exercises. M3 pins only probe the two
/// checkTypeRelatedTo entry relations the fixtures can observe;
/// identity/subtype/strictSubtype pins arrive with stage 4.8 via
/// their assignability consequences.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelpinRelation {
    Assignable,
    Comparable,
}

/// One pin, decoded from pins/relations.toml by xtask.
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
    /// The probe cannot answer yet (machinery lands in a later stage).
    /// `xtask relpin run` counts these as failures so the M3 gate
    /// cannot pass with a stubbed engine.
    Unsupported {
        reason: String,
    },
}

/// Stage 4.1: parse + bind the scratch program, resolve BOTH pin
/// annotations through the minimal annotation path, then hand off to
/// the relation engine — which is stages 4.4-4.5, so every pin that
/// constructs cleanly still reports the engine as the blocker.
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

    let source_file = tsrs2_syntax::parse_source_file(
        "relpin.ts".to_owned(),
        text,
        tsrs2_syntax::ParseOptions {
            language_variant: tsrs2_syntax::LanguageVariant::Standard,
            javascript_file: false,
        },
        None,
    );
    if !source_file.parse_diagnostics.is_empty() {
        return RelpinVerdict::Unsupported {
            reason: format!(
                "scratch program has parse errors (first: TS{})",
                source_file.parse_diagnostics[0].code()
            ),
        };
    }
    let binder = tsrs2_binder::bind_source_file(&source_file, query.options);
    let mut state = CheckerState::new(&source_file, binder, query.options);

    let Some(source_annotation) = find_probe_annotation(&source_file, "__relpin_source") else {
        return RelpinVerdict::Unsupported {
            reason: "probe source annotation not found in scratch program".to_owned(),
        };
    };
    let Some(target_annotation) = find_probe_annotation(&source_file, "__relpin_target") else {
        return RelpinVerdict::Unsupported {
            reason: "probe target annotation not found in scratch program".to_owned(),
        };
    };

    let source_type = match state.get_type_from_type_node(source_annotation) {
        Ok(ty) => ty,
        Err(err) => {
            return RelpinVerdict::Unsupported {
                reason: format!("source type: {}", err.reason),
            }
        }
    };
    let target_type = match state.get_type_from_type_node(target_annotation) {
        Ok(ty) => ty,
        Err(err) => {
            return RelpinVerdict::Unsupported {
                reason: format!("target type: {}", err.reason),
            }
        }
    };
    let _ = (
        source_type,
        target_type,
        query.source_is_fresh,
        query.relation,
    );

    RelpinVerdict::Unsupported {
        reason: "relation engine not implemented (M3 stages 4.4+)".to_owned(),
    }
}

/// The scratch program is generated above: find the declared probe
/// var's type annotation by the identifier's raw text (escapedText
/// would carry the leading-underscore escape).
pub(crate) fn find_probe_annotation(source: &SourceFile, name: &str) -> Option<NodeId> {
    for index in 0..source.arena.len() {
        let node = source.arena.node(NodeId(index as u32));
        let NodeData::VariableDeclaration(data) = &node.data else {
            continue;
        };
        let Some(declared_name) = data.name else {
            continue;
        };
        let NodeData::Identifier(identifier) = &source.arena.node(declared_name).data else {
            continue;
        };
        if identifier.text == name {
            return data.r#type;
        }
    }
    None
}
