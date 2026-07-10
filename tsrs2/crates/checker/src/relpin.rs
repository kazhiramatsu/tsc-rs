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
//! Stage 4.0 lands the harness first: `probe_relation` is a stub that
//! reports every query unsupported until stage 4.1 provides the
//! annotation path and 4.5 the engine core.

use tsrs2_types::CompilerOptions;

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

/// Stage 4.0 stub: the minimal type-from-annotation path is stage 4.1
/// and the engine core is stage 4.5; until they land every pin is
/// unsupported.
pub fn probe_relation(_query: &RelpinQuery) -> RelpinVerdict {
    RelpinVerdict::Unsupported {
        reason: "relation engine not implemented (M3 stages 4.1+)".to_owned(),
    }
}
