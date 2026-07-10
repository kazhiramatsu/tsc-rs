//! CompilerOptions — the subset the engine consumes so far. Lives in
//! tsrs2-types so both the binder and the checker can read it (the
//! checker re-exports it for downstream callers).

use crate::flags::ScriptTarget;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerOptions {
    /// tsc getAllowJSCompilerOption: allowJs ?? !!checkJs.
    pub allow_js: bool,
    pub experimental_decorators: bool,
    /// tsc ScriptTarget value; None when the option is absent.
    pub target: Option<i32>,
    pub always_strict: Option<bool>,
    pub strict: Option<bool>,
    /// strict-family flags the relation engine consumes (M3+); read
    /// through strict_option_value like tsc getStrictOptionValue.
    pub strict_null_checks: Option<bool>,
    pub strict_function_types: Option<bool>,
    /// Selects missingType for optional properties (undefinedOrMissingType,
    /// _tsc.js 47041).
    pub exact_optional_property_types: Option<bool>,
    /// Consumed by bindCaseBlock (clause.fallthroughFlowNode).
    pub no_fallthrough_cases_in_switch: Option<bool>,
}

impl CompilerOptions {
    /// tsc _computedOptions.target.computeValue (18245): ES3 counts as
    /// unset; the default is ScriptTarget.ES2025 (LatestStandard).
    pub fn emit_script_target(&self) -> ScriptTarget {
        match self.target {
            Some(target) if target != ScriptTarget::ES3.bits() => ScriptTarget::from_bits(target),
            _ => ScriptTarget::ES2025,
        }
    }

    /// tsc _computedOptions.alwaysStrict.computeValue (18257):
    /// `compilerOptions.alwaysStrict !== false` — strict-by-default in
    /// TS 6; only an explicit `alwaysStrict: false` disables it (the
    /// `strict` flag is NOT consulted).
    pub fn always_strict_effective(&self) -> bool {
        self.always_strict != Some(false)
    }

    /// tsc getStrictOptionValue (18280): `strict !== false` when the
    /// specific flag is absent — the strict family defaults ON.
    pub fn strict_option_value(&self, flag: Option<bool>) -> bool {
        match flag {
            Some(value) => value,
            None => self.strict != Some(false),
        }
    }
}
