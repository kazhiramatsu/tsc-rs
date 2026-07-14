//! Global-type bootstrap (M4 5.0) — the getGlobalType family plus the
//! LAZY memo slots for the core globals tsc binds eagerly in
//! initializeTypeChecker (88788-88873).
//!
//! The M4 5.0 decision (m4-checker-skeleton-steps.md): the core
//! globals (Array/Object/Function/String/...) resolve LAZILY here so
//! each begins resolving the moment 5.1's declared types exist —
//! keeping 5.1's array arm and 5.3's apparent chain unblocked. noLib
//! semantics stay the default: a global resolves only when the fixture
//! declares it; an undeclared global falls back per
//! getTypeOfGlobalSymbol (60604): program-level 2318 +
//! emptyGenericType (arity > 0) / emptyObjectType.

use tsrs2_binder::SymbolId;
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{NodeId, SyntaxKind};
use tsrs2_types::{SymbolFlags, TypeId};

use crate::state::{CheckResult2, CheckerState};

/// The deferredGlobal* memo slots (pattern at 60679) + the init-block
/// globals the M4 plan defers to lazy resolution. `Some` = resolved
/// once (fallbacks included — tsc assigns the fallback into the same
/// variable, so the 2318 fires once per program).
#[derive(Debug, Default)]
pub(crate) struct GlobalTypeMemos {
    array: Option<TypeId>,
    object: Option<TypeId>,
    function: Option<TypeId>,
    callable_function: Option<TypeId>,
    newable_function: Option<TypeId>,
    string: Option<TypeId>,
    number: Option<TypeId>,
    boolean: Option<TypeId>,
    regexp: Option<TypeId>,
    /// deferredGlobalESSymbolType / deferredGlobalBigIntType — filled
    /// only on SUCCESS (reportErrors=false lookups retry per call).
    es_symbol: Option<TypeId>,
    big_int: Option<TypeId>,
    readonly_array: Option<TypeId>,
    this_type: Option<Option<TypeId>>,
    any_array: Option<TypeId>,
    auto_array: Option<TypeId>,
    any_readonly_array: Option<TypeId>,
    arguments_type: Option<TypeId>,
    /// deferredGlobalImportCallOptionsType (60719) — the fallback
    /// (emptyObjectType) is memoized like tsc's `|| emptyObjectType`
    /// short-circuit target is NOT: tsc retries the lookup per call
    /// while the memo stays undefined, so only a SUCCESS memoizes.
    import_call_options: Option<TypeId>,
    /// deferredGlobalImportAttributesType (60727).
    import_attributes: Option<TypeId>,
    /// deferredGlobalIterableType (60820).
    iterable: Option<TypeId>,
    /// The §4 iteration-protocol resolver globals (60777-60881) — all
    /// `||`-memoized like `iterable` (only a SUCCESS memoizes; the
    /// emptyGenericType fallback is returned per-call).
    iterator: Option<TypeId>,
    iterable_iterator: Option<TypeId>,
    iterator_object: Option<TypeId>,
    generator: Option<TypeId>,
    async_iterable: Option<TypeId>,
    async_iterator: Option<TypeId>,
    async_iterable_iterator: Option<TypeId>,
    async_iterator_object: Option<TypeId>,
    async_generator: Option<TypeId>,
    iterator_yield_result: Option<TypeId>,
    iterator_return_result: Option<TypeId>,
    /// deferredGlobalBuiltinIteratorTypes /
    /// deferredGlobalBuiltinAsyncIteratorTypes — `??`-memoized (an
    /// empty list memoizes too, unlike the `||` family).
    builtin_iterator_types: Option<Vec<TypeId>>,
    builtin_async_iterator_types: Option<Vec<TypeId>>,
    /// deferredGlobalTemplateStringsArrayType (60688) — reportErrors
    /// TRUE and the `|| emptyObjectType` fallback sits INSIDE the memo
    /// assignment: a noLib miss reports 2318 once and memoizes the
    /// empty-object fallback.
    template_strings_array: Option<TypeId>,
    /// deferredGlobalPromiseConstructorLikeType (60769) — `||`
    /// memoized (only a SUCCESS memoizes; emptyObjectType fallback
    /// per call).
    promise_constructor_like: Option<TypeId>,
    /// deferredGlobalTypedPropertyDescriptorType (60679) — the
    /// emptyGenericType fallback sits INSIDE the memo assignment (a
    /// noLib miss reports 2318 once and memoizes the fallback).
    typed_property_descriptor: Option<TypeId>,
    /// The §10 decorator context globals (60945-61008) — `??`
    /// memoized: only a SUCCESS memoizes; the emptyGenericType
    /// fallback returns per call.
    class_decorator_context: Option<TypeId>,
    class_method_decorator_context: Option<TypeId>,
    class_getter_decorator_context: Option<TypeId>,
    class_setter_decorator_context: Option<TypeId>,
    class_accessor_decorator_context: Option<TypeId>,
    class_accessor_decorator_target: Option<TypeId>,
    class_accessor_decorator_result: Option<TypeId>,
    class_field_decorator_context: Option<TypeId>,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getGlobalSymbol @6.0.3
    /// tsc-hash: db19b41eed14644de0add1b20391729ac3cc3183a2349a58fd1c2a5c7d6102f0
    /// tsc-span: _tsc.js:60650-60662
    ///
    /// A locationless resolveName collapses to the globals-table tail
    /// of the scope walk (the createNameResolver globals lookup): the
    /// merged-symbol chase + meaning filter. The Alias hop in getSymbol
    /// is resolveAlias (M4 5.1) — global aliases are constructible only
    /// from import-equals in scripts; 5.1 replaces this lookup with the
    /// real resolveName tail.
    pub fn get_global_symbol(
        &mut self,
        name: &str,
        meaning: SymbolFlags,
        diagnostic: Option<&'static DiagnosticMessage>,
    ) -> Option<SymbolId> {
        let found = self.globals.get(name).copied().and_then(|symbol| {
            let symbol = self.get_merged_symbol(symbol);
            self.binder
                .symbol(symbol)
                .flags
                .intersects(meaning)
                .then_some(symbol)
        });
        if found.is_none() {
            if let Some(message) = diagnostic {
                // A resolveName WITH nameNotFoundMessage runs onFailed on
                // a miss (19797): locationless, the guard short-circuits
                // and the tail emits the global diagnostic; the
                // lib-suggestion probe and the budget-gated spelling
                // attempt both target the same locationless diagnostic —
                // never a per-file sink — so only the emission and the
                // unconditional suggestionCount++ (48152) are observable.
                self.error_at(
                    None,
                    message,
                    &[tsrs2_binder::unescape_leading_underscores(name)],
                );
                self.suggestion_count += 1;
            }
        }
        found
    }

    /// tsc-port: initializeTypeChecker @6.0.3
    /// tsc-hash: 7810a622d40fe41d9333a31267c113caa061cfed59c1823e328e852a7598e466
    /// tsc-span: _tsc.js:88779-88850
    ///
    /// The reportErrors=true getGlobalType calls, SYMBOL-probed eagerly
    /// in tsc's order. Type materialization stays lazy (the 5.0
    /// deviation) — but the probes cannot: each noLib failure burns one
    /// suggestionCount slot, and the burn must precede every file-level
    /// resolution failure or the name-side 2552/2304 selection diverges
    /// (risk #1; oracle-pinned via strictBindCallApply:false). The lazy
    /// getters consume this memo so each name keeps exactly one
    /// resolveName-with-message, like tsc.
    pub(crate) fn run_init_global_type_probes(&mut self) {
        let strict_bind_call_apply = self
            .options
            .strict_option_value(self.options.strict_bind_call_apply);
        let probes: [(&'static str, bool); 10] = [
            ("IArguments", true),
            ("Array", true),
            ("Object", true),
            ("Function", true),
            ("CallableFunction", strict_bind_call_apply),
            ("NewableFunction", strict_bind_call_apply),
            ("String", true),
            ("Number", true),
            ("Boolean", true),
            ("RegExp", true),
        ];
        for (name, live) in probes {
            if !live {
                continue;
            }
            // Symbol probe + budget consumption ONLY. tsc also emits
            // the locationless 2318 here; ours stays with the lazy
            // getter's first demand (pre-5.5d surface) — locationless
            // diagnostics never reach a per-file sink (lib.rs drops
            // them), so the timing difference is unobservable, while
            // the COUNT must happen now (the name-side 2552/2304
            // selection reads it).
            let symbol = self.get_global_symbol(name, SymbolFlags::TYPE, None);
            if symbol.is_none() {
                self.suggestion_count += 1;
            }
            self.init_global_type_probes.insert(name, symbol);
        }
    }

    /// The init-probe memo consult: names on the initializeTypeChecker
    /// list already ran their (counted) probe at init; a memoized MISS
    /// emits its 2318 on first demand (error_at dedupes repeats)
    /// without re-consuming budget.
    fn init_probed_global_type_symbol(&mut self, name: &'static str) -> Option<SymbolId> {
        match self.init_global_type_probes.get(name) {
            Some(&Some(symbol)) => Some(symbol),
            Some(&None) => {
                self.error_at(
                    None,
                    &diagnostics::Cannot_find_global_type_0,
                    &[tsrs2_binder::unescape_leading_underscores(name)],
                );
                None
            }
            None => self.get_global_type_symbol(name, /*report_errors*/ true),
        }
    }

    /// tsc-port: getGlobalTypeSymbol @6.0.3
    /// tsc-hash: 79e4e32b89b69c291bc572e0b38a387ebf68f5e7eaa792e242abadeefc49b7bc
    /// tsc-span: _tsc.js:60635-60637
    pub fn get_global_type_symbol(&mut self, name: &str, report_errors: bool) -> Option<SymbolId> {
        self.get_global_symbol(
            name,
            SymbolFlags::TYPE,
            report_errors.then_some(&diagnostics::Cannot_find_global_type_0),
        )
    }

    /// tsc-port: getTypeOfGlobalSymbol @6.0.3
    /// tsc-hash: 439c4c26ba61de8b13d2318306893a8d8ed9e1c7b0cefb01f74631307c312714
    /// tsc-span: _tsc.js:60604-60631
    ///
    /// The typeParameters arity check compares against the M3/5.0
    /// declared-type slice: non-generic interfaces only, so the count
    /// is 0 (generic interfaces unwind as Unsupported from
    /// getDeclaredTypeOfClassOrInterface until 5.1 lands them).
    pub fn get_type_of_global_symbol(
        &mut self,
        symbol: Option<SymbolId>,
        arity: usize,
    ) -> CheckResult2<TypeId> {
        let fallback = if arity > 0 {
            self.empty_generic_type
        } else {
            self.empty_object_type
        };
        let Some(symbol) = symbol else {
            return Ok(fallback);
        };
        let declared = self.get_declared_type_of_symbol_for_global(symbol)?;
        if !self
            .tables
            .flags_of(declared)
            .intersects(tsrs2_types::TypeFlags::OBJECT)
        {
            let name = self.symbol_display_name(symbol);
            let declaration = self.global_type_declaration(symbol);
            self.error_at(
                declaration,
                &diagnostics::Global_type_0_must_be_a_class_or_interface_type,
                &[&name],
            );
            return Ok(fallback);
        }
        // length(type.typeParameters): GenericType declared types
        // carry their parameters since 5.2b; plain Object declared
        // types are the non-generic (undefined -> 0) case.
        let type_parameter_count = match &self.tables.type_of(declared).data {
            tsrs2_types::TypeData::GenericType {
                type_parameters, ..
            } => type_parameters.len(),
            _ => 0,
        };
        if type_parameter_count != arity {
            let name = self.symbol_display_name(symbol);
            let declaration = self.global_type_declaration(symbol);
            self.error_at(
                declaration,
                &diagnostics::Global_type_0_must_have_1_type_parameter_s,
                &[&name, &arity.to_string()],
            );
            return Ok(fallback);
        }
        Ok(declared)
    }

    /// getTypeOfGlobalSymbol's inner getTypeDeclaration (60605-60617).
    fn global_type_declaration(&self, symbol: SymbolId) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                matches!(
                    self.binder
                        .source_of_node(declaration)
                        .arena
                        .node(declaration)
                        .kind,
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::EnumDeclaration
                )
            })
    }

    /// getDeclaredTypeOfSymbol for globals: the full
    /// tryGetDeclaredTypeOfSymbol dispatch (annotate.rs slice).
    fn get_declared_type_of_symbol_for_global(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        self.get_declared_type_of_symbol_slice(symbol)
    }

    /// tsc-port: getGlobalType @6.0.3
    /// tsc-hash: 9bdbf8979ae7ec8428bb59a64e0ce1b21937a306d447e3190a56279315bc5e26
    /// tsc-span: _tsc.js:60663-60666
    pub fn get_global_type(
        &mut self,
        name: &str,
        arity: usize,
        report_errors: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let symbol = self.get_global_type_symbol(name, report_errors);
        if symbol.is_some() || report_errors {
            Ok(Some(self.get_type_of_global_symbol(symbol, arity)?))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getGlobalTypeOrUndefined @6.0.3
    /// tsc-hash: 76eb67af3c404e3bb09739c3e2ac1f08c47093f6901e789233e7cb48581bfa94
    /// tsc-span: _tsc.js:60898-60906
    pub fn get_global_type_or_undefined(
        &mut self,
        name: &str,
        arity: usize,
    ) -> CheckResult2<Option<TypeId>> {
        let symbol = self.get_global_symbol(name, SymbolFlags::TYPE, None);
        match symbol {
            Some(symbol) => Ok(Some(self.get_type_of_global_symbol(Some(symbol), arity)?)),
            None => Ok(None),
        }
    }

    /// tsc-port: getGlobalDisposableType @6.0.3
    /// tsc-hash: 2f2cf3199d46ec1575c567857c6ca9a8f491e75a659cf35621b5a2e9171f16d3
    /// tsc-span: _tsc.js:60882-60889
    ///
    /// emptyObjectType memoizes a miss (the caller's `!==
    /// emptyObjectType` gate stands the band down; noLib fixtures ride
    /// the 2318 band from the reportErrors probe).
    pub fn get_global_disposable_type(&mut self, report_errors: bool) -> CheckResult2<TypeId> {
        if let Some(memo) = self.deferred_global_disposable_type {
            return Ok(memo);
        }
        let resolved = self
            .get_global_type("Disposable", /*arity*/ 0, report_errors)?
            .unwrap_or(self.empty_object_type);
        self.deferred_global_disposable_type = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: getGlobalAsyncDisposableType @6.0.3
    /// tsc-hash: 53203a106cdf5a79912acb05046f34917ef4be01660b3a70c3805fbd27cd6086
    /// tsc-span: _tsc.js:60890-60897
    pub fn get_global_async_disposable_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(memo) = self.deferred_global_async_disposable_type {
            return Ok(memo);
        }
        let resolved = self
            .get_global_type("AsyncDisposable", /*arity*/ 0, report_errors)?
            .unwrap_or(self.empty_object_type);
        self.deferred_global_async_disposable_type = Some(resolved);
        Ok(resolved)
    }

    // ---- the lazily-bound init-block globals (88788-88873) ----

    /// initializeTypeChecker 88788-88794, lazy per the 5.0 plan.
    pub fn global_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.array {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("Array");
        let resolved = self.get_type_of_global_symbol(symbol, 1)?;
        self.global_type_memos.array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88795-88801.
    pub fn global_object_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.object {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("Object");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.object = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88802-88808.
    pub fn global_function_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.function {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("Function");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.function = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88809-88815: strictBindCallApply selects
    /// CallableFunction (no report) with the Function fallback.
    pub fn global_callable_function_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.callable_function {
            return Ok(cached);
        }
        let strict_bind_call_apply = self
            .options
            .strict_option_value(self.options.strict_bind_call_apply);
        // `strictBindCallApply && getGlobalType(..., /*reportErrors*/
        // true) || globalFunctionType`: with reportErrors the lookup
        // always yields a (possibly fallback) type — truthy — so the
        // Function fallback fires only when strictBindCallApply is off.
        let resolved = if strict_bind_call_apply {
            {
                let symbol = self.init_probed_global_type_symbol("CallableFunction");
                self.get_type_of_global_symbol(symbol, 0)?
            }
        } else {
            self.global_function_type()?
        };
        self.global_type_memos.callable_function = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88816-88822.
    pub fn global_newable_function_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.newable_function {
            return Ok(cached);
        }
        let strict_bind_call_apply = self
            .options
            .strict_option_value(self.options.strict_bind_call_apply);
        let resolved = if strict_bind_call_apply {
            {
                let symbol = self.init_probed_global_type_symbol("NewableFunction");
                self.get_type_of_global_symbol(symbol, 0)?
            }
        } else {
            self.global_function_type()?
        };
        self.global_type_memos.newable_function = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88823-88829.
    pub fn global_string_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.string {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("String");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.string = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88830-88836.
    pub fn global_number_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.number {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("Number");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.number = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88837-88843.
    pub fn global_boolean_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.boolean {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("Boolean");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.boolean = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: getGlobalESSymbolType @6.0.3
    /// tsc-hash: 9d3cd071de9b48c23ee3704a3db67475970b93443e481bc6a6cc3f65bf1c97ef
    /// tsc-span: _tsc.js:60741-60749
    ///
    /// reportErrors=false — a failed lookup falls back to
    /// emptyObjectType WITHOUT memoizing (tsc retries per call).
    pub fn global_es_symbol_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.es_symbol {
            return Ok(cached);
        }
        let resolved = self.get_global_type("Symbol", 0, false)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.es_symbol = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_object_type)
    }

    /// tsc-port: getGlobalBigIntType @6.0.3
    /// tsc-hash: 53c9b2ec3484063b340bd5a57b78efd29aff564b303c1e92c64c71c602450d69
    /// tsc-span: _tsc.js:60936-60944
    pub fn global_big_int_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.big_int {
            return Ok(cached);
        }
        let resolved = self.get_global_type("BigInt", 0, false)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.big_int = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_object_type)
    }

    /// initializeTypeChecker 88844-88850.
    pub fn global_regexp_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.regexp {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("RegExp");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.regexp = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88863-88867: `|| globalArrayType`.
    pub fn global_readonly_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.readonly_array {
            return Ok(cached);
        }
        let resolved = match self.get_global_type_or_undefined("ReadonlyArray", 1)? {
            Some(ty) => ty,
            None => self.global_array_type()?,
        };
        self.global_type_memos.readonly_array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88869-88873 (ThisType stays optional).
    pub fn global_this_type_alias(&mut self) -> CheckResult2<Option<TypeId>> {
        if let Some(cached) = self.global_type_memos.this_type {
            return Ok(cached);
        }
        let resolved = self.get_global_type_or_undefined("ThisType", 1)?;
        self.global_type_memos.this_type = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: getGlobalImportCallOptionsType @6.0.3
    /// tsc-hash: 5f2aba39e62bc9958871cefa6d88a061576effca96d4f9ea914a59381768bbb4
    /// tsc-span: _tsc.js:60719-60726
    ///
    /// reportErrors=false at the 5.5b contextual arm, true at the
    /// import-call worker (77733) — a missing global falls back to
    /// emptyObjectType without memoizing, like tsc.
    pub(crate) fn get_global_import_call_options_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.import_call_options {
            return Ok(cached);
        }
        let resolved = self.get_global_type("ImportCallOptions", 0, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.import_call_options = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_object_type)
    }

    /// tsc-port: getGlobalImportAttributesType @6.0.3
    /// tsc-hash: dda5a08c113a1250008424b3ebc35f67a77158155b296ab688f68c61a2e4cbab
    /// tsc-span: _tsc.js:60727-60734
    pub(crate) fn get_global_import_attributes_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.import_attributes {
            return Ok(cached);
        }
        let resolved = self.get_global_type("ImportAttributes", 0, false)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.import_attributes = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_object_type)
    }

    /// tsc-port: getGlobalTypedPropertyDescriptorType @6.0.3
    /// tsc-hash: 22b2b2e9fb21cd1166f25a522e260d82e388d177541a11e582df40400572db3c
    /// tsc-span: _tsc.js:60679-60687
    pub(crate) fn get_global_typed_property_descriptor_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.typed_property_descriptor {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("TypedPropertyDescriptor", 1, /*report_errors*/ true)?
            .unwrap_or(self.empty_generic_type);
        self.global_type_memos.typed_property_descriptor = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: getGlobalClassDecoratorContextType @6.0.3
    /// tsc-hash: c0ab9cfcf0b2d556e8e5705e833e765174f73537ac631866a5e72d653a0fb824
    /// tsc-span: _tsc.js:60945-60952
    pub(crate) fn get_global_class_decorator_context_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_decorator_context {
            return Ok(cached);
        }
        let resolved =
            self.get_global_type("ClassDecoratorContext", 1, /*report_errors*/ true)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassMethodDecoratorContextType @6.0.3
    /// tsc-hash: 5ed635f5bf4e6fa8680d87e45408498f2de6fd5cf49ee1a9a810af1027816bea
    /// tsc-span: _tsc.js:60953-60960
    pub(crate) fn get_global_class_method_decorator_context_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_method_decorator_context {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassMethodDecoratorContext",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_method_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassGetterDecoratorContextType @6.0.3
    /// tsc-hash: 3e980b8213ad276b5d0226a4251cbdab783e31adae13ab5f89f84cee13d984fb
    /// tsc-span: _tsc.js:60961-60968
    pub(crate) fn get_global_class_getter_decorator_context_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_getter_decorator_context {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassGetterDecoratorContext",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_getter_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassSetterDecoratorContextType @6.0.3
    /// tsc-hash: 6d5f4db36ddf911f42a0a06ded1380d5181f8feddd2c9f420cf42ee43ca4636b
    /// tsc-span: _tsc.js:60969-60976
    pub(crate) fn get_global_class_setter_decorator_context_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_setter_decorator_context {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassSetterDecoratorContext",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_setter_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassAccessorDecoratorContextType @6.0.3
    /// tsc-hash: 9255eb94ba4fae1c52f4535995d838360dbf5fc13e49b35de6c8a03bcf392bb8
    /// tsc-span: _tsc.js:60977-60984
    pub(crate) fn get_global_class_accessor_decorator_context_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_accessor_decorator_context {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassAccessorDecoratorContext",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_accessor_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassAccessorDecoratorTargetType @6.0.3
    /// tsc-hash: 868f31819ef76612705c81014e07d72fa1ab0418521b320133ed5bf7868a944d
    /// tsc-span: _tsc.js:60985-60992
    pub(crate) fn get_global_class_accessor_decorator_target_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_accessor_decorator_target {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassAccessorDecoratorTarget",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_accessor_decorator_target = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassAccessorDecoratorResultType @6.0.3
    /// tsc-hash: 4b64c345ef7a029fa9b3a67d155e932c9c662c158e6caaf10365ae70ec84d3b1
    /// tsc-span: _tsc.js:60993-61000
    pub(crate) fn get_global_class_accessor_decorator_result_type(
        &mut self,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_accessor_decorator_result {
            return Ok(cached);
        }
        let resolved = self.get_global_type(
            "ClassAccessorDecoratorResult",
            2,
            /*report_errors*/ true,
        )?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_accessor_decorator_result = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalClassFieldDecoratorContextType @6.0.3
    /// tsc-hash: fa092c061b0e22bc938f2a384bbe4ecb59b1eb038e3b509a3e959f8df55209d2
    /// tsc-span: _tsc.js:61001-61008
    pub(crate) fn get_global_class_field_decorator_context_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.class_field_decorator_context {
            return Ok(cached);
        }
        let resolved =
            self.get_global_type("ClassFieldDecoratorContext", 2, /*report_errors*/ true)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.class_field_decorator_context = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalTemplateStringsArrayType @6.0.3
    /// tsc-hash: de0940ef152fcbf182f52f2e15a37b09cbc656cb634e07930302c7959427369d
    /// tsc-span: _tsc.js:60688-60696
    pub(crate) fn get_global_template_strings_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.template_strings_array {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("TemplateStringsArray", 0, /*report_errors*/ true)?
            .unwrap_or(self.empty_object_type);
        self.global_type_memos.template_strings_array = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: getGlobalIterableType @6.0.3
    /// tsc-hash: 33c29440c03323fad8bcacecfad8edb02e4959b2b609e4b72f6d80fa5976a610
    /// tsc-span: _tsc.js:60820-60827
    pub(crate) fn get_global_iterable_type(&mut self, report_errors: bool) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterable {
            return Ok(cached);
        }
        let resolved = self.get_global_type("Iterable", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterable = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalPromiseConstructorLikeType @6.0.3
    /// tsc-hash: 919e84bd89382839dcfc952131fa3361aa7e6bad292e3982ff86788fdac1bdd9
    /// tsc-span: _tsc.js:60769-60776
    pub(crate) fn get_global_promise_constructor_like_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.promise_constructor_like {
            return Ok(cached);
        }
        let resolved = self.get_global_type("PromiseConstructorLike", 0, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.promise_constructor_like = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_object_type)
    }

    /// tsc-port: getGlobalIteratorType @6.0.3
    /// tsc-hash: e01cd348b57917d99c019686f8d5b023e88c55802b63b47b05b6b2ca449f8c25
    /// tsc-span: _tsc.js:60828-60835
    pub(crate) fn get_global_iterator_type(&mut self, report_errors: bool) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("Iterator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalIterableIteratorType @6.0.3
    /// tsc-hash: 999a95dd563cc4418d1ad8152c4232786379fdc67f887c9a68712b0ebd708d25
    /// tsc-span: _tsc.js:60836-60843
    pub(crate) fn get_global_iterable_iterator_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterable_iterator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("IterableIterator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterable_iterator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalIteratorObjectType @6.0.3
    /// tsc-hash: c6bb65e792cad1a928e87009b8ef5016a7e19a2980fecffb032bab6276ede05f
    /// tsc-span: _tsc.js:60850-60857
    pub(crate) fn get_global_iterator_object_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterator_object {
            return Ok(cached);
        }
        let resolved = self.get_global_type("IteratorObject", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterator_object = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalGeneratorType @6.0.3
    /// tsc-hash: 60a6d039330a3b8c43fe9eb55d58ff30965acde70d3c95a60201f19193984437
    /// tsc-span: _tsc.js:60858-60865
    pub(crate) fn get_global_generator_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.generator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("Generator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.generator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalAsyncIterableType @6.0.3
    /// tsc-hash: a1f72f4bae88a60b8cecf030c82e1c458d69af1265797314a42c26249658010c
    /// tsc-span: _tsc.js:60777-60784
    pub(crate) fn get_global_async_iterable_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.async_iterable {
            return Ok(cached);
        }
        let resolved = self.get_global_type("AsyncIterable", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.async_iterable = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalAsyncIteratorType @6.0.3
    /// tsc-hash: 66980ef6137df46e6436c1c8d7a3fb37f737891663db49ad76ea23fa6ce49925
    /// tsc-span: _tsc.js:60785-60792
    pub(crate) fn get_global_async_iterator_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.async_iterator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("AsyncIterator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.async_iterator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalAsyncIterableIteratorType @6.0.3
    /// tsc-hash: 1f092ddeffc2a2cbf0d1bd0066cbc0e50a1348a954ee8eb16a55551b9d6046a4
    /// tsc-span: _tsc.js:60793-60800
    pub(crate) fn get_global_async_iterable_iterator_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.async_iterable_iterator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("AsyncIterableIterator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.async_iterable_iterator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalAsyncIteratorObjectType @6.0.3
    /// tsc-hash: 3a982c41a1bc9df643d7db217720be0ca87a49a598afcac7d3f7a64fc8d0ceab
    /// tsc-span: _tsc.js:60804-60811
    pub(crate) fn get_global_async_iterator_object_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.async_iterator_object {
            return Ok(cached);
        }
        let resolved = self.get_global_type("AsyncIteratorObject", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.async_iterator_object = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalAsyncGeneratorType @6.0.3
    /// tsc-hash: 34edd2907913120b42bfcd102035016c61afd7c057374cb0d983196dd745cc6f
    /// tsc-span: _tsc.js:60812-60819
    pub(crate) fn get_global_async_generator_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.async_generator {
            return Ok(cached);
        }
        let resolved = self.get_global_type("AsyncGenerator", 3, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.async_generator = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalIteratorYieldResultType @6.0.3
    /// tsc-hash: 512fc66673e267f41b9268990e9978029c995c1907973600a541db2dc02d9d3e
    /// tsc-span: _tsc.js:60866-60873
    pub(crate) fn get_global_iterator_yield_result_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterator_yield_result {
            return Ok(cached);
        }
        let resolved = self.get_global_type("IteratorYieldResult", 1, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterator_yield_result = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalIteratorReturnResultType @6.0.3
    /// tsc-hash: f99b115f6d8aeb327e460d78701862c251695ce374410e81fadede81924fd97a
    /// tsc-span: _tsc.js:60874-60881
    pub(crate) fn get_global_iterator_return_result_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.iterator_return_result {
            return Ok(cached);
        }
        let resolved = self.get_global_type("IteratorReturnResult", 1, report_errors)?;
        if let Some(resolved) = resolved {
            self.global_type_memos.iterator_return_result = Some(resolved);
            return Ok(resolved);
        }
        Ok(self.empty_generic_type)
    }

    /// tsc-port: getGlobalBuiltinTypes @6.0.3
    /// tsc-hash: d87b529fa7c3cdf581d8e6360a9798142580bcd87fa14f9eb7afad16382a82bb
    /// tsc-span: _tsc.js:60667-60678
    ///
    /// reportErrors=false lookups: a missing NAME contributes nothing
    /// (append skips undefined); a found symbol contributes its
    /// (possibly fallback) type.
    fn get_global_builtin_types(
        &mut self,
        type_names: &[&str],
        arity: usize,
    ) -> CheckResult2<Vec<TypeId>> {
        let mut types = Vec::new();
        for type_name in type_names {
            if let Some(resolved) = self.get_global_type(type_name, arity, false)? {
                types.push(resolved);
            }
        }
        Ok(types)
    }

    /// tsc-port: getGlobalBuiltinIteratorTypes @6.0.3
    /// tsc-hash: bfd64bb69a3e1655ca90dfa9c47428e519ca1838c4b52243a55884b9813f591d
    /// tsc-span: _tsc.js:60847-60849
    pub(crate) fn get_global_builtin_iterator_types(&mut self) -> CheckResult2<Vec<TypeId>> {
        if let Some(cached) = &self.global_type_memos.builtin_iterator_types {
            return Ok(cached.clone());
        }
        let resolved = self.get_global_builtin_types(
            &[
                "ArrayIterator",
                "MapIterator",
                "SetIterator",
                "StringIterator",
            ],
            1,
        )?;
        self.global_type_memos.builtin_iterator_types = Some(resolved.clone());
        Ok(resolved)
    }

    /// tsc-port: getGlobalBuiltinAsyncIteratorTypes @6.0.3
    /// tsc-hash: e833de5aac24e9784e84d05bd92e0243cf24bf89398393008dc112a01aefb407
    /// tsc-span: _tsc.js:60801-60803
    pub(crate) fn get_global_builtin_async_iterator_types(&mut self) -> CheckResult2<Vec<TypeId>> {
        if let Some(cached) = &self.global_type_memos.builtin_async_iterator_types {
            return Ok(cached.clone());
        }
        let resolved = self.get_global_builtin_types(&["ReadableStreamAsyncIterator"], 1)?;
        self.global_type_memos.builtin_async_iterator_types = Some(resolved.clone());
        Ok(resolved)
    }

    /// tsc-port: createIterableType @6.0.3
    /// tsc-hash: b5802d1aa06aab8cb1671347b5ab610a89231e3bbf120a01b813a1ae4a396ce0
    /// tsc-span: _tsc.js:61032-61037
    pub(crate) fn create_iterable_type(&mut self, iterated_type: TypeId) -> CheckResult2<TypeId> {
        let iterable = self.get_global_iterable_type(true)?;
        let void_type = self.tables.intrinsics.void;
        let undefined = self.tables.intrinsics.undefined;
        Ok(self
            .create_type_from_generic_global_type(iterable, &[iterated_type, void_type, undefined]))
    }

    /// initializeTypeChecker 88851: anyArrayType = createArrayType(any).
    pub fn any_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.any_array {
            return Ok(cached);
        }
        let any = self.tables.intrinsics.any;
        let resolved = self.create_array_type(any, false)?;
        self.global_type_memos.any_array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88852-88862: autoArrayType, with the
    /// "must not BE emptyObjectType" replacement (autoArrayType is an
    /// identity sentinel — see getTypeOfVariableOrParameterOrProperty
    /// consumers in M5's auto-typing).
    pub fn auto_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.auto_array {
            return Ok(cached);
        }
        let auto = self.tables.intrinsics.auto;
        let mut resolved = self.create_array_type(auto, false)?;
        if resolved == self.empty_object_type {
            resolved = self.create_resolved_empty_anonymous_type(None);
        }
        self.global_type_memos.auto_array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88868.
    pub fn any_readonly_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.any_readonly_array {
            return Ok(cached);
        }
        let readonly_array = self.global_readonly_array_type()?;
        let any = self.tables.intrinsics.any;
        let resolved = self.create_type_from_generic_global_type(readonly_array, &[any]);
        self.global_type_memos.any_readonly_array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88779-88785: argumentsSymbol.type =
    /// getGlobalType("IArguments") — lazy here (the identifier arm in
    /// 5.5 consumes it).
    pub fn arguments_symbol_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.arguments_type {
            return Ok(cached);
        }
        let symbol = self.init_probed_global_type_symbol("IArguments");
        let resolved = self.get_type_of_global_symbol(symbol, 0)?;
        self.global_type_memos.arguments_type = Some(resolved);
        Ok(resolved)
    }

    /// tsc-port: createTypeFromGenericGlobalType @6.0.3
    /// tsc-hash: f3573dbda83485ca83d17daddfb3c290dbd63086dec0aba4ea5b984b43e122d9
    /// tsc-span: _tsc.js:61026-61028
    pub fn create_type_from_generic_global_type(
        &mut self,
        generic_global_type: TypeId,
        type_arguments: &[TypeId],
    ) -> TypeId {
        if generic_global_type != self.empty_generic_type {
            self.tables
                .create_type_reference(generic_global_type, type_arguments)
        } else {
            self.empty_object_type
        }
    }

    /// tsc-port: createArrayType @6.0.3
    /// tsc-hash: 1e31fd2a58e7dbe2b77ef7105f0174e0be62786113ba75e5734ed7e4d8598651
    /// tsc-span: _tsc.js:61038-61040
    pub fn create_array_type(
        &mut self,
        element_type: TypeId,
        readonly: bool,
    ) -> CheckResult2<TypeId> {
        let target = if readonly {
            self.global_readonly_array_type()?
        } else {
            self.global_array_type()?
        };
        Ok(self.create_type_from_generic_global_type(target, &[element_type]))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, TypeFlags};

    use crate::state::test_support::with_program_state;

    #[test]
    fn declared_global_interface_resolves_through_get_global_type() {
        with_program_state(
            &[("a.ts", "interface Foo { x: number }\n")],
            &CompilerOptions::default(),
            |state| {
                let resolved = state
                    .get_global_type("Foo", 0, true)
                    .expect("in slice")
                    .expect("reportErrors");
                assert!(state
                    .tables
                    .flags_of(resolved)
                    .intersects(TypeFlags::OBJECT));
                assert_ne!(resolved, state.empty_object_type);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn missing_global_array_reports_2318_once_and_falls_back() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let first = state.global_array_type().expect("in slice");
            assert_eq!(first, state.empty_generic_type);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2318]);
            assert_eq!(state.diagnostics[0].file_name, None);
            // Memoized: the second call re-reports nothing.
            let second = state.global_array_type().expect("in slice");
            assert_eq!(second, first);
            assert_eq!(state.diagnostics.len(), 1);
        });
    }

    #[test]
    fn non_generic_global_array_reports_2317_arity_error() {
        with_program_state(
            &[("a.ts", "interface Array { length: number }\n")],
            &CompilerOptions::default(),
            |state| {
                let resolved = state.global_array_type().expect("in slice");
                assert_eq!(resolved, state.empty_generic_type);
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2317]);
                // The arity error sits on the interface declaration.
                assert_eq!(state.diagnostics[0].file_name.as_deref(), Some("a.ts"));
            },
        );
    }

    #[test]
    fn missing_arity_zero_global_falls_back_to_empty_object() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let resolved = state.global_object_type().expect("in slice");
            assert_eq!(resolved, state.empty_object_type);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2318]);
        });
    }
}
