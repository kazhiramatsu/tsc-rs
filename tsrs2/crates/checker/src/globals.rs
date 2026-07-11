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
                self.error_at(
                    None,
                    message,
                    &[tsrs2_binder::unescape_leading_underscores(name)],
                );
            }
        }
        found
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

    /// getDeclaredTypeOfSymbol slice for globals: class/interface
    /// symbols route to the M3 interface path; everything else is a
    /// 5.1 row (type aliases, enums as globals).
    fn get_declared_type_of_symbol_for_global(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            return self.get_declared_type_of_class_or_interface(symbol);
        }
        Err(crate::state::Unsupported::new(
            "getDeclaredTypeOfSymbol beyond class/interface (M4 5.1)",
        ))
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

    // ---- the lazily-bound init-block globals (88788-88873) ----

    /// initializeTypeChecker 88788-88794, lazy per the 5.0 plan.
    pub fn global_array_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.array {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("Array", 1, true)?
            .expect("reportErrors");
        self.global_type_memos.array = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88795-88801.
    pub fn global_object_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.object {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("Object", 0, true)?
            .expect("reportErrors");
        self.global_type_memos.object = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88802-88808.
    pub fn global_function_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.function {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("Function", 0, true)?
            .expect("reportErrors");
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
            self.get_global_type("CallableFunction", 0, true)?
                .expect("reportErrors")
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
            self.get_global_type("NewableFunction", 0, true)?
                .expect("reportErrors")
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
        let resolved = self
            .get_global_type("String", 0, true)?
            .expect("reportErrors");
        self.global_type_memos.string = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88830-88836.
    pub fn global_number_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.number {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("Number", 0, true)?
            .expect("reportErrors");
        self.global_type_memos.number = Some(resolved);
        Ok(resolved)
    }

    /// initializeTypeChecker 88837-88843.
    pub fn global_boolean_type(&mut self) -> CheckResult2<TypeId> {
        if let Some(cached) = self.global_type_memos.boolean {
            return Ok(cached);
        }
        let resolved = self
            .get_global_type("Boolean", 0, true)?
            .expect("reportErrors");
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
        let resolved = self
            .get_global_type("RegExp", 0, true)?
            .expect("reportErrors");
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
        let resolved = self
            .get_global_type("IArguments", 0, true)?
            .expect("reportErrors");
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
