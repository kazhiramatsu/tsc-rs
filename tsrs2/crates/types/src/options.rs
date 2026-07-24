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
    /// tsc ModuleDetectionKind value (Legacy=1, Auto=2, Force=3);
    /// None reads through the computed default, which is Force for
    /// Node16..NodeNext module kinds and Auto otherwise.
    pub module_detection: Option<i32>,
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
    /// M5 6.6: the 7030 arms — checkAllCodePaths' trailing else-if
    /// (79096) and checkReturnStatement's bare-return face (84546).
    /// NOT strict-family (plain option read).
    pub no_implicit_returns: Option<bool>,
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
    /// Imports downlevel emit helpers from `tslib`. Unlike the
    /// transformer-side use of this option, checkExternalEmitHelpers
    /// is semantic: it verifies that an in-program helper module
    /// exports every helper required by the checked syntax.
    pub import_helpers: Option<bool>,
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
    /// M4 5.8d: tsc ModuleResolutionKind value; None when absent. Read
    /// through emit_module_resolution_kind (computed default depends on
    /// module). Selects the Classic 2792 face over the plain 2307 in
    /// resolveExternalModuleName (49466). 85 conformance fixtures carry
    /// the directive (43 classic).
    pub module_resolution: Option<i32>,
    /// M4 5.8d: read through es_module_interop_effective (TS6 computed
    /// default TRUE, 18079-region). Selects interop faces in
    /// resolveESModuleSymbol (49715) + errorNoModuleMemberSymbol
    /// message flavors (48947). 19 conformance fixtures.
    pub es_module_interop: Option<bool>,
    /// M4 5.8d: read through allow_synthetic_default_imports_effective
    /// (TS6 computed default TRUE, 18088-region). Gates the §9
    /// default-import bands (canHaveSyntheticDefault 48609,
    /// getTargetofModuleDefault 48658). No standalone conformance
    /// directive observed; carried for the computed read.
    pub allow_synthetic_default_imports: Option<bool>,
    /// M4 5.8d: read through should_preserve_const_enums (tsc
    /// _computedOptions.preserveConstEnums 18157: preserveConstEnums ||
    /// computed isolatedModules). Feeds isInstantiatedModule in
    /// checkModuleDeclaration (85840). 2 conformance fixtures.
    pub preserve_const_enums: Option<bool>,
    /// 6.6 review: feeds ONLY the computed preserveConstEnums read
    /// (18157) — the isolatedModules diagnostic band itself (1208/
    /// 2748-family) stays unmodeled. Checker-side reads only (the
    /// lib-bundle key projects binder observables exclusively).
    pub isolated_modules: Option<bool>,
    /// 6.6 review: second disjunct of tsc's computed isolatedModules
    /// (18160-18162). Its own diagnostic band stays unmodeled.
    pub verbatim_module_syntax: Option<bool>,
    /// M4 5.8d: carried for the module resolver's suppression gate
    /// (baseUrl-relative candidates probe the program set; a miss
    /// under baseUrl is tsc-undecidable → no 2307). Full baseUrl
    /// semantics (paths mapping) stay unmodeled — ledger.
    pub base_url: Option<String>,
    /// M4 5.8d: gates the 5097 An_import_path_can_only_end_with_a_0_
    /// extension row (shouldAllowImportingTsExtension) — a true value
    /// legalizes .ts-family specifiers.
    pub allow_importing_ts_extensions: Option<bool>,
    /// tsc resolveJsonModule computed option. TS 6 enables it by
    /// default for Node20/NodeNext module kinds and Bundler resolution.
    pub resolve_json_module: Option<bool>,
    /// M4 5.8d: skipTypeCheckingWorker's first arm (18896) —
    /// declaration files produce NO bind/check diagnostics when set.
    /// 100 conformance fixtures carry the directive.
    pub skip_lib_check: Option<bool>,
    /// JSX namespace/runtime customization options.
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

    /// tsc-port: _computedOptions.moduleDetection.computeValue @6.0.3
    /// tsc-hash: 02bdfcb4bb95a89419dc45c895950e09fd6dcd12f54f2893380375555decf3ef
    /// tsc-span: _tsc.js:18062-18071
    pub fn emit_module_detection_kind(&self) -> i32 {
        self.module_detection.unwrap_or_else(|| {
            if (100..=199).contains(&self.emit_module_kind()) {
                3
            } else {
                2
            }
        })
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

    /// tsc _computedOptions.moduleResolution.computeValue (18040):
    /// explicit value wins; else None(0)/AMD(2)/UMD(3)/System(4) →
    /// Classic(1), NodeNext(199) → NodeNext(99), Node16(100)..<199 →
    /// Node16(3), else Bundler(100). TS6 dropped the Node10 default —
    /// CommonJS computes Bundler.
    pub fn emit_module_resolution_kind(&self) -> i32 {
        if let Some(module_resolution) = self.module_resolution {
            return module_resolution;
        }
        match self.emit_module_kind() {
            0 | 2 | 3 | 4 => 1,
            199 => 99,
            100..=198 => 3,
            _ => 100,
        }
    }

    /// tsc _computedOptions.esModuleInterop.computeValue (18079-region):
    /// explicit value wins; TS6 defaults TRUE.
    pub fn es_module_interop_effective(&self) -> bool {
        self.es_module_interop.unwrap_or(true)
    }

    /// tsc _computedOptions.allowSyntheticDefaultImports.computeValue
    /// (18088-region): explicit value wins; TS6 defaults TRUE (the TS5
    /// esModuleInterop/System derivation is gone).
    pub fn allow_synthetic_default_imports_effective(&self) -> bool {
        self.allow_synthetic_default_imports.unwrap_or(true)
    }

    pub fn resolve_json_module_effective(&self) -> bool {
        self.resolve_json_module.unwrap_or_else(|| {
            matches!(self.emit_module_kind(), 102 | 199)
                || self.emit_module_resolution_kind() == 100
        })
    }

    /// tsc shouldPreserveConstEnums = _computedOptions.preserveConstEnums
    /// .computeValue (18157): preserveConstEnums || computed
    /// isolatedModules, where computed isolatedModules =
    /// isolatedModules || verbatimModuleSyntax (18160-18162). An
    /// unreachable const enum under `@isolatedModules` reports 7027
    /// like tsc (the 6.6-review const-enum face); the options' own
    /// diagnostic bands stay unmodeled.
    pub fn should_preserve_const_enums(&self) -> bool {
        self.preserve_const_enums == Some(true)
            || self.isolated_modules == Some(true)
            || self.verbatim_module_syntax == Some(true)
    }
}
