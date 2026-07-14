//! CompilerOptions — the subset the engine consumes so far. Lives in
//! tsrs2-types so both the binder and the checker can read it (the
//! checker re-exports it for downstream callers).

use crate::flags::ScriptTarget;

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct CompilerOptions {
    /// tsc getAllowJSCompilerOption: allowJs ?? !!checkJs.
    pub allow_js: bool,
    pub experimental_decorators: bool,
    /// tsc ScriptTarget value; None when the option is absent.
    pub target: Option<i32>,
    /// tsc ModuleKind value; None when the option is absent. Read
    /// through emit_module_kind (the computed default depends on
    /// target). Consumed by checkGrammarImportCallExpression's rows
    /// (18060/1323/1324/1450/1325) — other moduleKind reads across the
    /// checker still assume the default (their fixtures' divergences
    /// predate this field; tighten per-site as bands land).
    pub module: Option<i32>,
    pub always_strict: Option<bool>,
    pub strict: Option<bool>,
    /// strict-family flags the relation engine consumes (M3+); read
    /// through strict_option_value like tsc getStrictOptionValue.
    pub strict_null_checks: Option<bool>,
    pub strict_function_types: Option<bool>,
    /// Gates the 7022/7023-family implicit-any circularity reports
    /// (reportCircularityError 56893, getReturnTypeOfSignature 59826).
    pub no_implicit_any: Option<bool>,
    /// Gates checkThisExpression's 2683/7041-family implicit-this
    /// reports; strict-family (read through strict_option_value).
    pub no_implicit_this: Option<bool>,
    /// Selects CallableFunction/NewableFunction over Function in the
    /// global bootstrap (initializeTypeChecker 88809-88822).
    pub strict_bind_call_apply: Option<bool>,
    /// Selects missingType for optional properties (undefinedOrMissingType,
    /// _tsc.js 47041).
    pub exact_optional_property_types: Option<bool>,
    /// Consumed by bindCaseBlock (clause.fallthroughFlowNode).
    pub no_fallthrough_cases_in_switch: Option<bool>,
    /// tsc allowUnreachableCode: undefined = warn-as-suggestion for
    /// unreachable statements, but the comma-operator 2695 gate reads
    /// plain falsiness (`!compilerOptions.allowUnreachableCode`).
    pub allow_unreachable_code: Option<bool>,
    /// tsc checkJs: absent per-file directives, an EXPLICIT false turns
    /// bind/check diagnostics off for JS files
    /// (canIncludeBindAndCheckDiagnostics — isPlainJsFile requires
    /// checkJs === undefined). A per-file @ts-check/@ts-nocheck
    /// overrides the option; true is the checkJs band (JS checking
    /// semantics largely unported).
    pub check_js: Option<bool>,
    /// M4 5.5d access band: index-signature reads union in missingType
    /// (checkPropertyAccessExpressionOrQualifiedName 75301,
    /// getPropertyTypeForIndexType 62575). NOT strict-family.
    pub no_unchecked_indexed_access: Option<bool>,
    /// 4111 (property comes from an index signature) at 75304.
    pub no_property_access_from_index_signature: Option<bool>,
    /// strict-family; the assumeUninitialized this-property arm of
    /// getFlowTypeOfAccessExpression (75352).
    pub strict_property_initialization: Option<bool>,
    /// checkPropertyNotUsedBeforeDeclaration's static-method exemption
    /// gate (75378); defaulted from target (>= ES2022) like tsc's
    /// computed option.
    pub use_define_for_class_fields: Option<bool>,
    /// strict-family; the catch-clause arm of
    /// getTypeForVariableLikeDeclaration (56055).
    pub use_unknown_in_catch_variables: Option<bool>,
    /// The RAW `lib` option entries, lowercased (e.g. "es5", "dom") —
    /// containerSeemsToBeEmptyDomElement (75471) only asks whether the
    /// option EXISTS without "dom".
    pub lib: Option<Vec<String>>,
    /// tsc JsxEmit value (None=0/Preserve=1/React=2/ReactNative=3/
    /// ReactJSX=4/ReactJSXDev=5); None when the option is absent.
    /// checkJsxPreconditions' 17004 reads `(jsx || 0) === 0`.
    pub jsx: Option<i32>,
    /// M4 5.8a: the skippedOn("noEmit") filter's input — collision-band
    /// diagnostics (errorSkippedOn 47575) drop at the program layer
    /// when noEmit is set (filterSemanticDiagnostics 125664). 727
    /// conformance fixtures carry the directive (469 true-valued).
    pub no_emit: Option<bool>,
    /// M4 5.8b §4: getIteratedTypeOrElementType's plain-falsiness read
    /// (83915 `!uplevelIteration && compilerOptions.downlevelIteration`)
    /// — selects the downlevel diagnostic flavors under low targets.
    /// 21 conformance fixtures carry the directive.
    pub downlevel_iteration: Option<bool>,
    /// strict-family (46472: getStrictOptionValue — defaults ON);
    /// getBuiltinIteratorReturnType (60844) selects undefinedType over
    /// anyType for the builtin-iterator TReturn slot. Read through
    /// strict_option_value.
    pub strict_builtin_iterator_return: Option<bool>,
    /// jsxFactory/jsxFragmentFactory/jsxImportSource/reactNamespace:
    /// carried so the 5.5f JSX slice can ESCAPE fixtures that
    /// customize the namespace entity (pragma machinery + entity
    /// parsing are unported — ledger).
    pub jsx_factory: Option<String>,
    pub jsx_fragment_factory: Option<String>,
    pub jsx_import_source: Option<String>,
    pub react_namespace: Option<String>,
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

    /// tsc _computedOptions.module.computeValue (18190-region):
    /// explicit value wins; else target ESNext → ESNext(99), target ≥
    /// ES2022 → ES2022(7), ≥ ES2020 → ES2020(6), ≥ ES2015 → ES2015(5),
    /// else CommonJS(1). The ES2025 default target computes ES2022.
    pub fn emit_module_kind(&self) -> i32 {
        if let Some(module) = self.module {
            return module;
        }
        let target = self.emit_script_target();
        if target == ScriptTarget::ES_NEXT {
            99
        } else if target >= ScriptTarget::ES2022 {
            7
        } else if target >= ScriptTarget::ES2020 {
            6
        } else if target >= ScriptTarget::ES2015 {
            5
        } else {
            1
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

    /// tsc _computedOptions.useDefineForClassFields.computeValue
    /// (18251): defaults to target >= ES2022.
    pub fn use_define_for_class_fields_effective(&self) -> bool {
        match self.use_define_for_class_fields {
            Some(value) => value,
            None => self.emit_script_target() >= ScriptTarget::ES2022,
        }
    }

    /// tsc getEmitStandardClassFields (18286): NOT the same as the
    /// computed useDefineForClassFields — an explicit `true` below
    /// ES2022 still emits legacy fields.
    pub fn emit_standard_class_fields(&self) -> bool {
        self.use_define_for_class_fields != Some(false)
            && self.emit_script_target() >= ScriptTarget::ES2022
    }
}
