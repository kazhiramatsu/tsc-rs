//! M4 5.8b: the iteration protocol (§4 of the statement-extraction
//! doc) — IterationTypes singletons + the per-type verdict cache, the
//! sync/async resolvers, checkIteratedTypeOrElementType and the
//! getIterationTypesOf* worker family, and the generator return-type
//! readers. The [ITER] stub tag retires against this band; consumers
//! lift per-site (operators/literals/functions/contextual/widen/expr/
//! calls).
//!
//! Representation deviation (verdict-neutral): tsc interns
//! "intrinsic-ish" IterationTypes triples in iterationTypesCache so
//! `===` works for anyIterationTypes; the port uses Copy VALUE
//! semantics (component equality) and a dedicated enum variant for the
//! noIterationTypes poison — the only identity tsc relies on is the
//! anyIterationTypes/noIterationTypes sentinel compare, which value
//! equality reproduces exactly (m4-58 §4 "identity only matters for
//! the cache").
//!
//! Container semantics (risk §14.3/.6): errorOutputContainer ports as
//! COLLECT-ONLY — the relation machinery never adds to the program
//! sink while collecting; each call site performs exactly one
//! diagnostics-add (or related-info attach) per collected diagnostic.
//! With skipLogging UNSET tsc adds eagerly AND pushes (its
//! diagnostics.add dedupes the exact duplicate away), so one explicit
//! add is byte-equivalent.

use tsrs2_diags::{gen as diagnostics, Diagnostic, RelatedInfo};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    IterationTypeKind, IterationUse, ScriptTarget, SymbolFlags, TypeFacts, TypeFlags, TypeId,
    UnionReduction,
};

use crate::state::{CheckResult2, CheckerState};

/// tsc IterationTypes triple (the non-poison shape). Copy value
/// semantics; see the module note on the interning deviation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IterationTypes {
    pub yield_type: TypeId,
    pub return_type: TypeId,
    pub next_type: TypeId,
}

impl IterationTypes {
    /// tsc-port: getIterationTypesKeyFromIterationTypeKind @6.0.3
    /// tsc-hash: dbe00f29d8431167568c07e26f9a3a2c0f16ca48bfcc6b833d68471e22fd8949
    /// tsc-span: _tsc.js:90932-90941
    ///
    /// The field selector, applied directly instead of returning a
    /// key string.
    pub fn by_kind(self, kind: IterationTypeKind) -> TypeId {
        match kind {
            IterationTypeKind::RETURN => self.return_type,
            IterationTypeKind::NEXT => self.next_type,
            _ => self.yield_type,
        }
    }
}

/// A computed iteration-types verdict: `No` is the noIterationTypes
/// poison singleton (field reads Debug.fail in tsc — a dedicated
/// variant, never a triple). "Not yet computed / undefined" is
/// `Option::None` at rest (the links cache) and in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IterationTypesResult {
    No,
    Types(IterationTypes),
}

impl IterationTypesResult {
    /// tsrs-native: the `=== noIterationTypes ? undefined : x` unwrap
    /// every tsc consumer writes inline.
    pub fn types(self) -> Option<IterationTypes> {
        match self {
            IterationTypesResult::No => None,
            IterationTypesResult::Types(types) => Some(types),
        }
    }
}

/// The five per-type cache slots (tsc type[cacheKey]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IterationCacheKey {
    Iterable,
    AsyncIterable,
    Iterator,
    AsyncIterator,
    IteratorResult,
}

/// The sync/async resolver pair (47301/47316) — tsc closes over
/// global getters, cache keys, the awaited hook, and the diagnostic
/// flavors; the port dispatches on this tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IterResolver {
    Sync,
    Async,
}

impl IterResolver {
    fn iterable_cache_key(self) -> IterationCacheKey {
        match self {
            IterResolver::Sync => IterationCacheKey::Iterable,
            IterResolver::Async => IterationCacheKey::AsyncIterable,
        }
    }

    fn iterator_cache_key(self) -> IterationCacheKey {
        match self {
            IterResolver::Sync => IterationCacheKey::Iterator,
            IterResolver::Async => IterationCacheKey::AsyncIterator,
        }
    }

    fn iterator_symbol_name(self) -> &'static str {
        match self {
            IterResolver::Sync => "iterator",
            IterResolver::Async => "asyncIterator",
        }
    }

    fn must_have_a_next_method_diagnostic(self) -> &'static tsrs2_diags::DiagnosticMessage {
        match self {
            IterResolver::Sync => &diagnostics::An_iterator_must_have_a_next_method,
            IterResolver::Async => &diagnostics::An_async_iterator_must_have_a_next_method,
        }
    }

    fn must_be_a_method_diagnostic(self) -> &'static tsrs2_diags::DiagnosticMessage {
        match self {
            IterResolver::Sync => &diagnostics::The_0_property_of_an_iterator_must_be_a_method,
            IterResolver::Async => {
                &diagnostics::The_0_property_of_an_async_iterator_must_be_a_method
            }
        }
    }

    fn must_have_a_value_diagnostic(self) -> &'static tsrs2_diags::DiagnosticMessage {
        match self {
            IterResolver::Sync => {
                &diagnostics::The_type_returned_by_the_0_method_of_an_iterator_must_have_a_value_property
            }
            IterResolver::Async => {
                &diagnostics::The_type_returned_by_the_0_method_of_an_async_iterator_must_be_a_promise_for_a_type_with_a_value_property
            }
        }
    }
}

/// tsc errorOutputContainer for this band: collect-only (module note).
#[derive(Debug, Default)]
pub(crate) struct IterationErrorContainer {
    pub errors: Vec<Diagnostic>,
    /// tsc skipLogging — when UNSET, the relation machinery's eager
    /// add is replayed by the collecting call site (one explicit add).
    pub skip_logging: bool,
}

fn related_info_from_diagnostic(diagnostic: &Diagnostic) -> RelatedInfo {
    RelatedInfo {
        file_name: diagnostic.file_name.clone(),
        start: diagnostic.start,
        length: diagnostic.length,
        message: diagnostic.message.clone(),
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getBuiltinIteratorReturnType @6.0.3
    /// tsc-hash: 35680cc5c91b52cf0674aa86001a96d335d4274f01a92592b28ca2e98bf4474a
    /// tsc-span: _tsc.js:60844-60846
    pub(crate) fn get_builtin_iterator_return_type(&self) -> TypeId {
        if self
            .options
            .strict_option_value(self.options.strict_builtin_iterator_return)
        {
            self.tables.intrinsics.undefined
        } else {
            self.tables.intrinsics.any
        }
    }

    /// anyIterationTypes (47327-region singleton).
    fn any_iteration_types(&self) -> IterationTypes {
        let any = self.tables.intrinsics.any;
        IterationTypes {
            yield_type: any,
            return_type: any,
            next_type: any,
        }
    }

    fn is_type_any(&self, ty: TypeId) -> bool {
        self.tables.flags_of(ty).intersects(TypeFlags::ANY)
    }

    // ---- resolver dispatch ----

    fn resolver_global_iterable_type(
        &mut self,
        resolver: IterResolver,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        match resolver {
            IterResolver::Sync => self.get_global_iterable_type(report_errors),
            IterResolver::Async => self.get_global_async_iterable_type(report_errors),
        }
    }

    fn resolver_global_iterator_type(
        &mut self,
        resolver: IterResolver,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        match resolver {
            IterResolver::Sync => self.get_global_iterator_type(report_errors),
            IterResolver::Async => self.get_global_async_iterator_type(report_errors),
        }
    }

    fn resolver_global_iterable_iterator_type(
        &mut self,
        resolver: IterResolver,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        match resolver {
            IterResolver::Sync => self.get_global_iterable_iterator_type(report_errors),
            IterResolver::Async => self.get_global_async_iterable_iterator_type(report_errors),
        }
    }

    fn resolver_global_iterator_object_type(
        &mut self,
        resolver: IterResolver,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        match resolver {
            IterResolver::Sync => self.get_global_iterator_object_type(report_errors),
            IterResolver::Async => self.get_global_async_iterator_object_type(report_errors),
        }
    }

    fn resolver_global_generator_type(
        &mut self,
        resolver: IterResolver,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        match resolver {
            IterResolver::Sync => self.get_global_generator_type(report_errors),
            IterResolver::Async => self.get_global_async_generator_type(report_errors),
        }
    }

    fn resolver_global_builtin_iterator_types(
        &mut self,
        resolver: IterResolver,
    ) -> CheckResult2<Vec<TypeId>> {
        match resolver {
            IterResolver::Sync => self.get_global_builtin_iterator_types(),
            IterResolver::Async => self.get_global_builtin_async_iterator_types(),
        }
    }

    /// resolver.resolveIterationType: sync = identity; async =
    /// getAwaitedType(type, errorNode, Type_of_await_operand_must...).
    fn resolve_iteration_type(
        &mut self,
        resolver: IterResolver,
        ty: TypeId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<Option<TypeId>> {
        match resolver {
            IterResolver::Sync => Ok(Some(ty)),
            IterResolver::Async => self.get_awaited_type_with_error(
                ty,
                error_node.map(|node| {
                    (
                        node,
                        &diagnostics::Type_of_await_operand_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
                    )
                }),
            ),
        }
    }

    // ---- diagnostics collection (container plumbing) ----

    /// Runs `f` with the program sink swapped out, handing back what
    /// it pushed. On Err the collected diagnostics are spliced back
    /// (side-effect emissions — 2318s, circularities — survive
    /// containment exactly as without the swap).
    fn with_collected_diagnostics<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> CheckResult2<T>,
    ) -> CheckResult2<(T, Vec<Diagnostic>)> {
        let saved = std::mem::take(&mut self.diagnostics);
        let result = f(self);
        let collected = std::mem::replace(&mut self.diagnostics, saved);
        match result {
            Ok(value) => Ok((value, collected)),
            Err(err) => {
                for diagnostic in collected {
                    self.push_error_diagnostic(diagnostic);
                }
                Err(err)
            }
        }
    }

    // ---- shape helpers ----

    /// tsc-port: createIterationTypes @6.0.3
    /// tsc-hash: 1d776805f6e32ddf0887a6161f38f6038b8b35d33d66575c9e306f847638685a
    /// tsc-span: _tsc.js:84020-84031
    ///
    /// Defaults (yield=never, return=never, next=unknown) applied at
    /// the call sites; the intern cache is elided (module note).
    fn create_iteration_types(
        &self,
        yield_type: Option<TypeId>,
        return_type: Option<TypeId>,
        next_type: Option<TypeId>,
    ) -> IterationTypes {
        IterationTypes {
            yield_type: yield_type.unwrap_or(self.tables.intrinsics.never),
            return_type: return_type.unwrap_or(self.tables.intrinsics.never),
            next_type: next_type.unwrap_or(self.tables.intrinsics.unknown),
        }
    }

    /// tsc-port: combineIterationTypes @6.0.3
    /// tsc-hash: 4ce2584c218e01a15cd6e1ca7eaf024199b47520cda31e1fada535bb6c2c472f
    /// tsc-span: _tsc.js:84032-84055
    fn combine_iteration_types(
        &mut self,
        array: &[Option<IterationTypesResult>],
    ) -> CheckResult2<IterationTypesResult> {
        let any_iteration_types = self.any_iteration_types();
        let mut yield_types: Vec<TypeId> = Vec::new();
        let mut return_types: Vec<TypeId> = Vec::new();
        let mut next_types: Vec<TypeId> = Vec::new();
        for entry in array {
            let types = match entry {
                None | Some(IterationTypesResult::No) => continue,
                Some(IterationTypesResult::Types(types)) => *types,
            };
            if types == any_iteration_types {
                return Ok(IterationTypesResult::Types(any_iteration_types));
            }
            yield_types.push(types.yield_type);
            return_types.push(types.return_type);
            next_types.push(types.next_type);
        }
        if !yield_types.is_empty() || !return_types.is_empty() || !next_types.is_empty() {
            let yield_type = if yield_types.is_empty() {
                None
            } else {
                Some(self.get_union_type_ex(&yield_types, UnionReduction::Literal)?)
            };
            let return_type = if return_types.is_empty() {
                None
            } else {
                Some(self.get_union_type_ex(&return_types, UnionReduction::Literal)?)
            };
            let next_type = if next_types.is_empty() {
                None
            } else {
                Some(self.get_intersection_type(
                    &next_types,
                    tsrs2_types::IntersectionFlags::default(),
                )?)
            };
            return Ok(IterationTypesResult::Types(self.create_iteration_types(
                yield_type,
                return_type,
                next_type,
            )));
        }
        Ok(IterationTypesResult::No)
    }

    /// tsc-port: getCachedIterationTypes @6.0.3
    /// tsc-hash: a2443aaea77ddd3a6c0b583bfa8a876123c9d6570ca1477a0f614c5ff38d9867
    /// tsc-span: _tsc.js:84056-84058
    fn get_cached_iteration_types(
        &self,
        ty: TypeId,
        key: IterationCacheKey,
    ) -> Option<IterationTypesResult> {
        let links = self.links.ty(ty);
        match key {
            IterationCacheKey::Iterable => links.iteration_types_of_iterable,
            IterationCacheKey::AsyncIterable => links.iteration_types_of_async_iterable,
            IterationCacheKey::Iterator => links.iteration_types_of_iterator,
            IterationCacheKey::AsyncIterator => links.iteration_types_of_async_iterator,
            IterationCacheKey::IteratorResult => links.iteration_types_of_iterator_result,
        }
    }

    /// tsc-port: setCachedIterationTypes @6.0.3
    /// tsc-hash: 88c6f180fbf4e691848e221f1d75fb9788b4a643b2b7b93f8a0eba1766b5e0c1
    /// tsc-span: _tsc.js:84059-84061
    fn set_cached_iteration_types(
        &mut self,
        ty: TypeId,
        key: IterationCacheKey,
        value: IterationTypesResult,
    ) -> IterationTypesResult {
        self.links
            .set_type_iteration_types(self.speculation_depth, ty, key, value);
        value
    }

    // ---- entry points ----

    /// tsc-port: checkIteratedTypeOrElementType @6.0.3
    /// tsc-hash: a1995c20c13326238073bb23c7c5d5466eb6bdaf3300df699009327171469f8a
    /// tsc-span: _tsc.js:83894-83906
    pub(crate) fn check_iterated_type_or_element_type(
        &mut self,
        use_: IterationUse,
        input_type: TypeId,
        sent_type: TypeId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<TypeId> {
        if self.is_type_any(input_type) {
            return Ok(input_type);
        }
        Ok(self
            .get_iterated_type_or_element_type(
                use_, input_type, sent_type, error_node, /*check_assignability*/ true,
            )?
            .unwrap_or(self.tables.intrinsics.any))
    }

    /// tsc-port: getIteratedTypeOrElementType @6.0.3
    /// tsc-hash: e783823500eedf6f17761b199a6dce2b16a695888d966af9309e6e06d02d842b
    /// tsc-span: _tsc.js:83907-83996
    pub(crate) fn get_iterated_type_or_element_type(
        &mut self,
        use_: IterationUse,
        input_type: TypeId,
        sent_type: TypeId,
        error_node: Option<NodeId>,
        check_assignability: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let allow_async_iterables = use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG);
        if input_type == self.tables.intrinsics.never {
            if let Some(error_node) = error_node {
                self.report_type_not_iterable_error(error_node, input_type, allow_async_iterables)?;
            }
            return Ok(None);
        }
        let iterable_exists =
            self.get_global_iterable_type(/*report_errors*/ false)? != self.empty_generic_type;
        let uplevel_iteration =
            self.options.emit_script_target() >= ScriptTarget::ES2015 && iterable_exists;
        let downlevel_iteration =
            !uplevel_iteration && self.options.downlevel_iteration.unwrap_or(false);
        let possible_out_of_bounds = self.options.no_unchecked_indexed_access.unwrap_or(false)
            && use_.intersects(IterationUse::POSSIBLY_OUT_OF_BOUNDS);
        if uplevel_iteration || downlevel_iteration || allow_async_iterables {
            let iteration_types = self.get_iteration_types_of_iterable(
                input_type,
                use_,
                if uplevel_iteration { error_node } else { None },
            )?;
            if check_assignability {
                if let Some(iteration_types) = iteration_types {
                    let diagnostic = if use_.intersects(IterationUse::FOR_OF_FLAG) {
                        Some(&diagnostics::Cannot_iterate_value_because_the_next_method_of_its_iterator_expects_type_1_but_for_of_will_always_send_0)
                    } else if use_.intersects(IterationUse::SPREAD_FLAG) {
                        Some(&diagnostics::Cannot_iterate_value_because_the_next_method_of_its_iterator_expects_type_1_but_array_spread_will_always_send_0)
                    } else if use_.intersects(IterationUse::DESTRUCTURING_FLAG) {
                        Some(&diagnostics::Cannot_iterate_value_because_the_next_method_of_its_iterator_expects_type_1_but_array_destructuring_will_always_send_0)
                    } else if use_.intersects(IterationUse::YIELD_STAR_FLAG) {
                        Some(&diagnostics::Cannot_delegate_iteration_to_value_because_the_next_method_of_its_iterator_expects_type_1_but_the_containing_generator_will_always_send_0)
                    } else {
                        None
                    };
                    if let Some(diagnostic) = diagnostic {
                        self.check_type_assignable_to(
                            sent_type,
                            iteration_types.next_type,
                            error_node,
                            diagnostic,
                        )?;
                    }
                }
            }
            if iteration_types.is_some() || uplevel_iteration {
                let yield_type = iteration_types.map(|types| types.yield_type);
                return if possible_out_of_bounds {
                    self.include_undefined_in_index_signature(yield_type)
                } else {
                    Ok(yield_type)
                };
            }
        }
        let mut array_type = input_type;
        let mut has_string_constituent = false;
        if use_.intersects(IterationUse::ALLOWS_STRING_INPUT_FLAG) {
            if self
                .tables
                .flags_of(array_type)
                .intersects(TypeFlags::UNION)
            {
                let array_types: Vec<TypeId> = match &self.tables.type_of(input_type).data {
                    tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                    _ => unreachable!("union flag implies union data"),
                };
                let filtered_types: Vec<TypeId> = array_types
                    .iter()
                    .copied()
                    .filter(|&t| !self.tables.flags_of(t).intersects(TypeFlags::STRING_LIKE))
                    .collect();
                if filtered_types.len() != array_types.len() {
                    array_type =
                        self.get_union_type_ex(&filtered_types, UnionReduction::Subtype)?;
                }
            } else if self
                .tables
                .flags_of(array_type)
                .intersects(TypeFlags::STRING_LIKE)
            {
                array_type = self.tables.intrinsics.never;
            }
            has_string_constituent = array_type != input_type;
            if has_string_constituent
                && self
                    .tables
                    .flags_of(array_type)
                    .intersects(TypeFlags::NEVER)
            {
                let string_type = self.tables.intrinsics.string;
                return if possible_out_of_bounds {
                    self.include_undefined_in_index_signature(Some(string_type))
                } else {
                    Ok(Some(string_type))
                };
            }
        }
        if !self.is_array_like_type(array_type)? {
            if let Some(error_node) = error_node {
                let allows_strings = use_.intersects(IterationUse::ALLOWS_STRING_INPUT_FLAG)
                    && !has_string_constituent;
                let (default_diagnostic, maybe_missing_await) = self
                    .get_iteration_diagnostic_details(
                        use_,
                        input_type,
                        allows_strings,
                        downlevel_iteration,
                    )?;
                let suggest_await =
                    maybe_missing_await && self.get_awaited_type_of_promise(array_type)?.is_some();
                let display = self.type_to_string_slice(array_type)?;
                self.error_and_maybe_suggest_await(
                    error_node,
                    suggest_await,
                    default_diagnostic,
                    &[&display],
                );
            }
            return if has_string_constituent {
                let string_type = self.tables.intrinsics.string;
                if possible_out_of_bounds {
                    self.include_undefined_in_index_signature(Some(string_type))
                } else {
                    Ok(Some(string_type))
                }
            } else {
                Ok(None)
            };
        }
        let number_type = self.tables.intrinsics.number;
        let array_element_type = self.get_index_type_of_type(array_type, number_type)?;
        if has_string_constituent {
            if let Some(array_element_type) = array_element_type {
                if self
                    .tables
                    .flags_of(array_element_type)
                    .intersects(TypeFlags::STRING_LIKE)
                    && !self.options.no_unchecked_indexed_access.unwrap_or(false)
                {
                    return Ok(Some(self.tables.intrinsics.string));
                }
                let string_type = self.tables.intrinsics.string;
                let undefined_type = self.tables.intrinsics.undefined;
                let members = if possible_out_of_bounds {
                    vec![array_element_type, string_type, undefined_type]
                } else {
                    vec![array_element_type, string_type]
                };
                return Ok(Some(
                    self.get_union_type_ex(&members, UnionReduction::Subtype)?,
                ));
            }
        }
        if use_.intersects(IterationUse::POSSIBLY_OUT_OF_BOUNDS) {
            self.include_undefined_in_index_signature(array_element_type)
        } else {
            Ok(array_element_type)
        }
    }

    /// getIterationDiagnosticDetails (inner fn of
    /// getIteratedTypeOrElementType, 83978-83995).
    fn get_iteration_diagnostic_details(
        &mut self,
        use_: IterationUse,
        input_type: TypeId,
        allows_strings: bool,
        downlevel_iteration: bool,
    ) -> CheckResult2<(&'static tsrs2_diags::DiagnosticMessage, bool)> {
        if downlevel_iteration {
            return Ok(if allows_strings {
                (
                    &diagnostics::Type_0_is_not_an_array_type_or_a_string_type_or_does_not_have_a_Symbol_iterator_method_that_returns_an_iterator,
                    true,
                )
            } else {
                (
                    &diagnostics::Type_0_is_not_an_array_type_or_does_not_have_a_Symbol_iterator_method_that_returns_an_iterator,
                    true,
                )
            });
        }
        let yield_type =
            self.get_iteration_type_of_iterable(use_, IterationTypeKind::YIELD, input_type, None)?;
        if yield_type.is_some() {
            return Ok((
                &diagnostics::Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher,
                false,
            ));
        }
        let symbol_name = self
            .tables
            .type_of(input_type)
            .symbol
            .map(|symbol| self.binder.symbol(symbol).escaped_name.clone());
        if symbol_name.is_some_and(|name| is_es2015_or_later_iterable(&name)) {
            return Ok((
                &diagnostics::Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher,
                true,
            ));
        }
        Ok(if allows_strings {
            (
                &diagnostics::Type_0_is_not_an_array_type_or_a_string_type,
                true,
            )
        } else {
            (&diagnostics::Type_0_is_not_an_array_type, true)
        })
    }

    /// tsc-port: includeUndefinedInIndexSignature @6.0.3
    /// tsc-hash: 81c24fed0446c0b66a2be78d8a9b40f096a6244e35b2c0ccf536f8603a59b973
    /// tsc-span: _tsc.js:69829-69832
    pub(crate) fn include_undefined_in_index_signature(
        &mut self,
        ty: Option<TypeId>,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(ty) = ty else {
            return Ok(None);
        };
        if self.options.no_unchecked_indexed_access.unwrap_or(false) {
            let missing = self.tables.intrinsics.missing;
            Ok(Some(self.get_union_type_ex(
                &[ty, missing],
                UnionReduction::Literal,
            )?))
        } else {
            Ok(Some(ty))
        }
    }

    /// tsc-port: getIterationTypeOfIterable @6.0.3
    /// tsc-hash: 0cb3c5ae2e493a57648798c3613e9d740de72469deacea9c93b6c2b53dd78233
    /// tsc-span: _tsc.js:84013-84019
    pub(crate) fn get_iteration_type_of_iterable(
        &mut self,
        use_: IterationUse,
        type_kind: IterationTypeKind,
        input_type: TypeId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<Option<TypeId>> {
        if self.is_type_any(input_type) {
            return Ok(None);
        }
        let iteration_types = self.get_iteration_types_of_iterable(input_type, use_, error_node)?;
        Ok(iteration_types.map(|types| types.by_kind(type_kind)))
    }

    // ---- iterable side ----

    /// tsc-port: getIterationTypesOfIterable @6.0.3
    /// tsc-hash: aefc4ffe6329819570bfd169b5075f1b0f5b05ebc266ebf3a8471cdbd53a02c3
    /// tsc-span: _tsc.js:84062-84112
    pub(crate) fn get_iteration_types_of_iterable(
        &mut self,
        ty: TypeId,
        use_: IterationUse,
        error_node: Option<NodeId>,
    ) -> CheckResult2<Option<IterationTypes>> {
        let ty = self.get_reduced_type(ty)?;
        if self.is_type_any(ty) {
            return Ok(Some(self.any_iteration_types()));
        }
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let mut container = error_node.map(|_| IterationErrorContainer {
                errors: Vec::new(),
                skip_logging: true,
            });
            let iteration_types =
                self.get_iteration_types_of_iterable_worker(ty, use_, error_node, &mut container)?;
            if iteration_types == IterationTypesResult::No {
                if let Some(error_node) = error_node {
                    let root_index = self.report_type_not_iterable_error(
                        error_node,
                        ty,
                        use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG),
                    )?;
                    if let Some(container) = &container {
                        let related: Vec<RelatedInfo> = container
                            .errors
                            .iter()
                            .map(related_info_from_diagnostic)
                            .collect();
                        self.diagnostics[root_index].related.extend(related);
                    }
                }
                return Ok(None);
            }
            if let Some(container) = container {
                // Collected during a SUCCESSFUL resolution (the
                // mustHaveAValue face): each row adds once.
                for diagnostic in container.errors {
                    self.push_error_diagnostic(diagnostic);
                }
            }
            return Ok(iteration_types.types());
        }
        let cache_key = if use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG) {
            IterationCacheKey::AsyncIterable
        } else {
            IterationCacheKey::Iterable
        };
        if let Some(cached) = self.get_cached_iteration_types(ty, cache_key) {
            return Ok(cached.types());
        }
        let constituents: Vec<TypeId> = match &self.tables.type_of(ty).data {
            tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies union data"),
        };
        let mut all_iteration_types: Vec<Option<IterationTypesResult>> = Vec::new();
        for constituent in constituents {
            let mut container = error_node.map(|_| IterationErrorContainer {
                errors: Vec::new(),
                skip_logging: false,
            });
            let iteration_types = self.get_iteration_types_of_iterable_worker(
                constituent,
                use_,
                error_node,
                &mut container,
            )?;
            if iteration_types == IterationTypesResult::No {
                if let Some(error_node) = error_node {
                    let root_index = self.report_type_not_iterable_error(
                        error_node,
                        ty,
                        use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG),
                    )?;
                    if let Some(container) = &container {
                        let related: Vec<RelatedInfo> = container
                            .errors
                            .iter()
                            .map(related_info_from_diagnostic)
                            .collect();
                        self.diagnostics[root_index].related.extend(related);
                    }
                }
                self.set_cached_iteration_types(ty, cache_key, IterationTypesResult::No);
                return Ok(None);
            }
            if let Some(container) = container {
                for diagnostic in container.errors {
                    self.push_error_diagnostic(diagnostic);
                }
            }
            all_iteration_types.push(Some(iteration_types));
        }
        let iteration_types = if all_iteration_types.is_empty() {
            IterationTypesResult::No
        } else {
            self.combine_iteration_types(&all_iteration_types)?
        };
        self.set_cached_iteration_types(ty, cache_key, iteration_types);
        Ok(iteration_types.types())
    }

    /// tsc-port: getAsyncFromSyncIterationTypes @6.0.3
    /// tsc-hash: 3c35e85c7d8e414d2bb973ae4c6c25a607d14f4bfc8d14b1414bee938399633f
    /// tsc-span: _tsc.js:84113-84128
    fn get_async_from_sync_iteration_types(
        &mut self,
        iteration_types: IterationTypesResult,
        error_node: Option<NodeId>,
    ) -> CheckResult2<IterationTypesResult> {
        let types = match iteration_types {
            IterationTypesResult::No => return Ok(IterationTypesResult::No),
            IterationTypesResult::Types(types) => types,
        };
        if types == self.any_iteration_types() {
            return Ok(iteration_types);
        }
        if error_node.is_some() {
            // The reportErrors=true Awaited-symbol probe (84118-84121)
            // burns/dedupes the global lookup exactly once.
            self.get_global_awaited_symbol(/*report_errors*/ true)?;
        }
        let any = self.tables.intrinsics.any;
        let awaited_yield = self
            .get_awaited_type_with_error(
                types.yield_type,
                error_node.map(|node| (node, &diagnostics::Type_of_await_operand_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member)),
            )?
            .unwrap_or(any);
        let awaited_return = self
            .get_awaited_type_with_error(
                types.return_type,
                error_node.map(|node| (node, &diagnostics::Type_of_await_operand_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member)),
            )?
            .unwrap_or(any);
        Ok(IterationTypesResult::Types(self.create_iteration_types(
            Some(awaited_yield),
            Some(awaited_return),
            Some(types.next_type),
        )))
    }

    /// tsc-port: getIterationTypesOfIterableWorker @6.0.3
    /// tsc-hash: c4a8f8d6494501c7a4662b292dec10d491259ff8b625e88a5f2b2f475144a8bf
    /// tsc-span: _tsc.js:84129-84175
    fn get_iteration_types_of_iterable_worker(
        &mut self,
        ty: TypeId,
        use_: IterationUse,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
    ) -> CheckResult2<IterationTypesResult> {
        if self.is_type_any(ty) {
            return Ok(IterationTypesResult::Types(self.any_iteration_types()));
        }
        let mut no_cache = false;
        if use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG) {
            let mut iteration_types = match self
                .get_cached_iteration_types(ty, IterResolver::Async.iterable_cache_key())
            {
                Some(cached) => Some(cached),
                None => self.get_iteration_types_of_iterable_fast(ty, IterResolver::Async)?,
            };
            if let Some(found) = iteration_types {
                if found == IterationTypesResult::No && error_node.is_some() {
                    no_cache = true;
                } else {
                    return if use_.intersects(IterationUse::FOR_OF_FLAG) {
                        // for-await over an async iterable re-awaits.
                        self.get_async_from_sync_iteration_types(found, error_node)
                    } else {
                        Ok(found)
                    };
                }
            }
            iteration_types = Some(self.get_iteration_types_of_iterable_slow(
                ty,
                IterResolver::Async,
                error_node,
                container,
                no_cache,
            )?);
            if iteration_types != Some(IterationTypesResult::No) {
                return Ok(iteration_types.expect("just assigned"));
            }
        }
        if use_.intersects(IterationUse::ALLOWS_SYNC_ITERABLES_FLAG) {
            let mut iteration_types = match self
                .get_cached_iteration_types(ty, IterResolver::Sync.iterable_cache_key())
            {
                Some(cached) => Some(cached),
                None => self.get_iteration_types_of_iterable_fast(ty, IterResolver::Sync)?,
            };
            if let Some(found) = iteration_types {
                if found == IterationTypesResult::No && error_node.is_some() {
                    no_cache = true;
                } else if use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG) {
                    if found != IterationTypesResult::No {
                        let async_types =
                            self.get_async_from_sync_iteration_types(found, error_node)?;
                        return Ok(if no_cache {
                            async_types
                        } else {
                            self.set_cached_iteration_types(
                                ty,
                                IterationCacheKey::AsyncIterable,
                                async_types,
                            )
                        });
                    }
                } else {
                    return Ok(found);
                }
            }
            iteration_types = Some(self.get_iteration_types_of_iterable_slow(
                ty,
                IterResolver::Sync,
                error_node,
                container,
                no_cache,
            )?);
            if iteration_types != Some(IterationTypesResult::No) {
                let found = iteration_types.expect("just assigned");
                if use_.intersects(IterationUse::ALLOWS_ASYNC_ITERABLES_FLAG) {
                    let async_types =
                        self.get_async_from_sync_iteration_types(found, error_node)?;
                    return Ok(if no_cache {
                        async_types
                    } else {
                        self.set_cached_iteration_types(
                            ty,
                            IterationCacheKey::AsyncIterable,
                            async_types,
                        )
                    });
                }
                return Ok(found);
            }
        }
        Ok(IterationTypesResult::No)
    }

    /// tsc-port: getIterationTypesOfIterableFast @6.0.3
    /// tsc-hash: 69db806265ad5f54b3febf80f939e4f2833a702e7e91b20538df4afdef3f2983
    /// tsc-span: _tsc.js:84179-84218
    fn get_iteration_types_of_iterable_fast(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
    ) -> CheckResult2<Option<IterationTypesResult>> {
        let global_iterable = self.resolver_global_iterable_type(resolver, false)?;
        let global_iterator_object = self.resolver_global_iterator_object_type(resolver, false)?;
        let global_iterable_iterator =
            self.resolver_global_iterable_iterator_type(resolver, false)?;
        let global_generator = self.resolver_global_generator_type(resolver, false)?;
        if self.is_reference_to_type(ty, global_iterable)
            || self.is_reference_to_type(ty, global_iterator_object)
            || self.is_reference_to_type(ty, global_iterable_iterator)
            || self.is_reference_to_type(ty, global_generator)
        {
            let arguments = self.get_type_arguments(ty)?;
            let (yield_type, return_type, next_type) = (arguments[0], arguments[1], arguments[2]);
            let resolved_yield = self
                .resolve_iteration_type(resolver, yield_type, None)?
                .unwrap_or(yield_type);
            let resolved_return = self
                .resolve_iteration_type(resolver, return_type, None)?
                .unwrap_or(return_type);
            let types = self.create_iteration_types(
                Some(resolved_yield),
                Some(resolved_return),
                Some(next_type),
            );
            return Ok(Some(self.set_cached_iteration_types(
                ty,
                resolver.iterable_cache_key(),
                IterationTypesResult::Types(types),
            )));
        }
        let builtin = self.resolver_global_builtin_iterator_types(resolver)?;
        if self.is_reference_to_some_type(ty, &builtin) {
            let arguments = self.get_type_arguments(ty)?;
            let yield_type = arguments[0];
            let return_type = self.get_builtin_iterator_return_type();
            let next_type = self.tables.intrinsics.unknown;
            let resolved_yield = self
                .resolve_iteration_type(resolver, yield_type, None)?
                .unwrap_or(yield_type);
            let resolved_return = self
                .resolve_iteration_type(resolver, return_type, None)?
                .unwrap_or(return_type);
            let types = self.create_iteration_types(
                Some(resolved_yield),
                Some(resolved_return),
                Some(next_type),
            );
            return Ok(Some(self.set_cached_iteration_types(
                ty,
                resolver.iterable_cache_key(),
                IterationTypesResult::Types(types),
            )));
        }
        Ok(None)
    }

    /// tsc-port: getIterationTypesOfIterableSlow @6.0.3
    /// tsc-hash: d962aeda084cb143061c817febafcc181bb3e0ec4c32318320505819c06d4751
    /// tsc-span: _tsc.js:84227-84256
    fn get_iteration_types_of_iterable_slow(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
        no_cache: bool,
    ) -> CheckResult2<IterationTypesResult> {
        let property_name =
            self.get_property_name_for_known_symbol_name(resolver.iterator_symbol_name())?;
        let method = self.get_property_of_type_full(ty, &property_name)?;
        let method_type = match method {
            Some(method)
                if !self
                    .binder
                    .symbol(method)
                    .flags
                    .intersects(SymbolFlags::OPTIONAL) =>
            {
                Some(self.get_type_of_symbol(method)?)
            }
            _ => None,
        };
        if method_type.is_some_and(|method_type| self.is_type_any(method_type)) {
            let any_types = IterationTypesResult::Types(self.any_iteration_types());
            return Ok(if no_cache {
                any_types
            } else {
                self.set_cached_iteration_types(ty, resolver.iterable_cache_key(), any_types)
            });
        }
        let all_signatures = match method_type {
            Some(method_type) => {
                self.get_signatures_of_type(method_type, crate::structural::SignatureKind::Call)?
            }
            None => Vec::new(),
        };
        let mut valid_signatures = Vec::new();
        for &signature in &all_signatures {
            if self.get_min_argument_count(signature)? == 0 {
                valid_signatures.push(signature);
            }
        }
        if valid_signatures.is_empty() {
            if let Some(error_node) = error_node {
                if !all_signatures.is_empty() {
                    let global_iterable = self.resolver_global_iterable_type(resolver, true)?;
                    // Collect-only container discipline (module note):
                    // warm the verdict, then re-run the reporting pass
                    // with the sink swapped out.
                    let verdict = self.is_type_assignable_to(ty, global_iterable)?;
                    if !verdict {
                        let (_, collected) = self.with_collected_diagnostics(|state| {
                            state.check_type_assignable_to(
                                ty,
                                global_iterable,
                                Some(error_node),
                                &diagnostics::Type_0_is_not_assignable_to_type_1,
                            )
                        })?;
                        match container {
                            Some(container) => {
                                if !container.skip_logging {
                                    for diagnostic in &collected {
                                        self.push_error_diagnostic(diagnostic.clone());
                                    }
                                }
                                container.errors.extend(collected);
                            }
                            None => {
                                for diagnostic in collected {
                                    self.push_error_diagnostic(diagnostic);
                                }
                            }
                        }
                    }
                }
            }
            return Ok(if no_cache {
                IterationTypesResult::No
            } else {
                self.set_cached_iteration_types(
                    ty,
                    resolver.iterable_cache_key(),
                    IterationTypesResult::No,
                )
            });
        }
        let mut return_types = Vec::new();
        for &signature in &valid_signatures {
            return_types.push(self.get_return_type_of_signature(signature)?);
        }
        let iterator_type =
            self.get_intersection_type(&return_types, tsrs2_types::IntersectionFlags::default())?;
        let iteration_types = self
            .get_iteration_types_of_iterator_worker(
                iterator_type,
                resolver,
                error_node,
                container,
                no_cache,
            )?
            .map(IterationTypesResult::Types)
            .unwrap_or(IterationTypesResult::No);
        Ok(if no_cache {
            iteration_types
        } else {
            self.set_cached_iteration_types(ty, resolver.iterable_cache_key(), iteration_types)
        })
    }

    /// tsc-port: reportTypeNotIterableError @6.0.3
    /// tsc-hash: 6dbc62092224a477ee120794c6f118ee39b79ef5f7798e47253e317834c73967
    /// tsc-span: _tsc.js:84257-84270
    ///
    /// Returns the diagnostic's sink index (the caller attaches the
    /// container rows as related info).
    fn report_type_not_iterable_error(
        &mut self,
        error_node: NodeId,
        ty: TypeId,
        allow_async_iterables: bool,
    ) -> CheckResult2<usize> {
        let message = if allow_async_iterables {
            &diagnostics::Type_0_must_have_a_Symbol_asyncIterator_method_that_returns_an_async_iterator
        } else {
            &diagnostics::Type_0_must_have_a_Symbol_iterator_method_that_returns_an_iterator
        };
        let mut suggest_await = self.get_awaited_type_of_promise(ty)?.is_some();
        if !suggest_await && !allow_async_iterables {
            let parent = self.parent_of(error_node);
            let is_for_of_expression = parent.is_some_and(|parent| {
                self.kind_of(parent) == SyntaxKind::ForOfStatement
                    && matches!(
                        self.data_of(parent),
                        NodeData::ForOfStatement(data) if data.expression == Some(error_node)
                    )
            });
            if is_for_of_expression {
                let global_async_iterable = self.get_global_async_iterable_type(false)?;
                if global_async_iterable != self.empty_generic_type {
                    let any = self.tables.intrinsics.any;
                    let target = self.create_type_from_generic_global_type(
                        global_async_iterable,
                        &[any, any, any],
                    );
                    suggest_await = self.is_type_assignable_to(ty, target)?;
                }
            }
        }
        let display = self.type_to_string_slice(ty)?;
        Ok(self.error_and_maybe_suggest_await(error_node, suggest_await, message, &[&display]))
    }

    // ---- iterator side ----

    /// tsc-port: getIterationTypesOfIterator @6.0.3
    /// tsc-hash: 7bba8513dfea669fbb014932e4c42cc98c85429a0209d6813a368d7f34369f2b
    /// tsc-span: _tsc.js:84271-84280
    pub(crate) fn get_iteration_types_of_iterator(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
    ) -> CheckResult2<Option<IterationTypes>> {
        self.get_iteration_types_of_iterator_worker(
            ty, resolver, error_node, container, /*no_cache*/ false,
        )
    }

    /// tsc-port: getIterationTypesOfIteratorWorker @6.0.3
    /// tsc-hash: 656e73dd7c45124a0cdedb5fc01d28479e1a5fcca483cda15a82871b0aecdad4
    /// tsc-span: _tsc.js:84281-84292
    fn get_iteration_types_of_iterator_worker(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
        no_cache: bool,
    ) -> CheckResult2<Option<IterationTypes>> {
        if self.is_type_any(ty) {
            return Ok(Some(self.any_iteration_types()));
        }
        let mut no_cache = no_cache;
        let mut iteration_types =
            match self.get_cached_iteration_types(ty, resolver.iterator_cache_key()) {
                Some(cached) => Some(cached),
                None => self.get_iteration_types_of_iterator_fast(ty, resolver)?,
            };
        if iteration_types == Some(IterationTypesResult::No) && error_node.is_some() {
            iteration_types = None;
            no_cache = true;
        }
        let iteration_types = match iteration_types {
            Some(found) => found,
            None => self.get_iteration_types_of_iterator_slow(
                ty, resolver, error_node, container, no_cache,
            )?,
        };
        Ok(iteration_types.types())
    }

    /// tsc-port: getIterationTypesOfIteratorFast @6.0.3
    /// tsc-hash: 12f20aac34eb2428b90da8fec5d8cbaabf2c9af609819d7318e5c308b412a891
    /// tsc-span: _tsc.js:84296-84319
    ///
    /// Unlike the iterable fast path, the iterator side takes the
    /// type arguments RAW (no resolveIterationType).
    fn get_iteration_types_of_iterator_fast(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
    ) -> CheckResult2<Option<IterationTypesResult>> {
        let global_iterable_iterator =
            self.resolver_global_iterable_iterator_type(resolver, false)?;
        let global_iterator = self.resolver_global_iterator_type(resolver, false)?;
        let global_iterator_object = self.resolver_global_iterator_object_type(resolver, false)?;
        let global_generator = self.resolver_global_generator_type(resolver, false)?;
        if self.is_reference_to_type(ty, global_iterable_iterator)
            || self.is_reference_to_type(ty, global_iterator)
            || self.is_reference_to_type(ty, global_iterator_object)
            || self.is_reference_to_type(ty, global_generator)
        {
            let arguments = self.get_type_arguments(ty)?;
            let types = self.create_iteration_types(
                Some(arguments[0]),
                Some(arguments[1]),
                Some(arguments[2]),
            );
            return Ok(Some(self.set_cached_iteration_types(
                ty,
                resolver.iterator_cache_key(),
                IterationTypesResult::Types(types),
            )));
        }
        let builtin = self.resolver_global_builtin_iterator_types(resolver)?;
        if self.is_reference_to_some_type(ty, &builtin) {
            let arguments = self.get_type_arguments(ty)?;
            let return_type = self.get_builtin_iterator_return_type();
            let unknown = self.tables.intrinsics.unknown;
            let types =
                self.create_iteration_types(Some(arguments[0]), Some(return_type), Some(unknown));
            return Ok(Some(self.set_cached_iteration_types(
                ty,
                resolver.iterator_cache_key(),
                IterationTypesResult::Types(types),
            )));
        }
        Ok(None)
    }

    /// tsc-port: getIterationTypesOfIteratorSlow @6.0.3
    /// tsc-hash: bb9ef75a8d02e75a573e649127dfbf63874e5641d11cdce28367eab32ebecf58
    /// tsc-span: _tsc.js:84462-84469
    fn get_iteration_types_of_iterator_slow(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
        no_cache: bool,
    ) -> CheckResult2<IterationTypesResult> {
        let next =
            self.get_iteration_types_of_method(ty, resolver, "next", error_node, container)?;
        let return_ =
            self.get_iteration_types_of_method(ty, resolver, "return", error_node, container)?;
        let throw =
            self.get_iteration_types_of_method(ty, resolver, "throw", error_node, container)?;
        let iteration_types = self.combine_iteration_types(&[next, return_, throw])?;
        Ok(if no_cache {
            iteration_types
        } else {
            self.set_cached_iteration_types(ty, resolver.iterator_cache_key(), iteration_types)
        })
    }

    /// tsc-port: isIteratorResult @6.0.3
    /// tsc-hash: ea6f8dc7d3927078c419583afddcab2bf43a1fc74de7180d95ea3ab16f52b2c6
    /// tsc-span: _tsc.js:84320-84323
    ///
    /// (isYieldIteratorResult 84324-84326 / isReturnIteratorResult
    /// 84327-84329 are the two kind-fixing wrappers, inlined at the
    /// filter sites.)
    fn is_iterator_result(&mut self, ty: TypeId, kind: IterationTypeKind) -> CheckResult2<bool> {
        let done_prop = self.get_property_of_type_full(ty, "done")?;
        let done_type = match done_prop {
            Some(prop) => self.get_type_of_symbol(prop)?,
            None => self.tables.intrinsics.false_fresh,
        };
        let source = if kind == IterationTypeKind::YIELD {
            self.tables.intrinsics.false_fresh
        } else {
            self.tables.intrinsics.true_fresh
        };
        self.is_type_assignable_to(source, done_type)
    }

    /// tsc-port: getIterationTypesOfIteratorResult @6.0.3
    /// tsc-hash: 1d50281f1c3720a1889109504c58a44652e37855a7d92389a2e2e10848139926
    /// tsc-span: _tsc.js:84330-84377
    fn get_iteration_types_of_iterator_result(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<IterationTypesResult> {
        if self.is_type_any(ty) {
            return Ok(IterationTypesResult::Types(self.any_iteration_types()));
        }
        if let Some(cached) = self.get_cached_iteration_types(ty, IterationCacheKey::IteratorResult)
        {
            return Ok(cached);
        }
        let yield_result_type = self.get_global_iterator_yield_result_type(false)?;
        if self.is_reference_to_type(ty, yield_result_type) {
            let yield_type = self.get_type_arguments(ty)?[0];
            let types = self.create_iteration_types(Some(yield_type), None, None);
            return Ok(self.set_cached_iteration_types(
                ty,
                IterationCacheKey::IteratorResult,
                IterationTypesResult::Types(types),
            ));
        }
        let return_result_type = self.get_global_iterator_return_result_type(false)?;
        if self.is_reference_to_type(ty, return_result_type) {
            let return_type = self.get_type_arguments(ty)?[0];
            let types = self.create_iteration_types(None, Some(return_type), None);
            return Ok(self.set_cached_iteration_types(
                ty,
                IterationCacheKey::IteratorResult,
                IterationTypesResult::Types(types),
            ));
        }
        let yield_iterator_result = self.filter_type_with(ty, |state, t| {
            state.is_iterator_result(t, IterationTypeKind::YIELD)
        })?;
        let yield_type = if yield_iterator_result != self.tables.intrinsics.never {
            let prop = self.get_property_of_type_full(yield_iterator_result, "value")?;
            match prop {
                Some(prop) => Some(self.get_type_of_symbol(prop)?),
                None => None,
            }
        } else {
            None
        };
        let return_iterator_result = self.filter_type_with(ty, |state, t| {
            state.is_iterator_result(t, IterationTypeKind::RETURN)
        })?;
        let return_type = if return_iterator_result != self.tables.intrinsics.never {
            let prop = self.get_property_of_type_full(return_iterator_result, "value")?;
            match prop {
                Some(prop) => Some(self.get_type_of_symbol(prop)?),
                None => None,
            }
        } else {
            None
        };
        if yield_type.is_none() && return_type.is_none() {
            return Ok(self.set_cached_iteration_types(
                ty,
                IterationCacheKey::IteratorResult,
                IterationTypesResult::No,
            ));
        }
        let void_type = self.tables.intrinsics.void;
        let types =
            self.create_iteration_types(yield_type, Some(return_type.unwrap_or(void_type)), None);
        Ok(self.set_cached_iteration_types(
            ty,
            IterationCacheKey::IteratorResult,
            IterationTypesResult::Types(types),
        ))
    }

    /// tsc-port: getIterationTypesOfMethod @6.0.3
    /// tsc-hash: 707b054666ae6f040cca9e97052e6fdbb203485c7a73cc683e66bca10c6be3d0
    /// tsc-span: _tsc.js:84378-84461
    fn get_iteration_types_of_method(
        &mut self,
        ty: TypeId,
        resolver: IterResolver,
        method_name: &str,
        error_node: Option<NodeId>,
        container: &mut Option<IterationErrorContainer>,
    ) -> CheckResult2<Option<IterationTypesResult>> {
        let method = self.get_property_of_type_full(ty, method_name)?;
        if method.is_none() && method_name != "next" {
            return Ok(None);
        }
        let method_type = match method {
            Some(method)
                if !(method_name == "next"
                    && self
                        .binder
                        .symbol(method)
                        .flags
                        .intersects(SymbolFlags::OPTIONAL)) =>
            {
                let raw = self.get_type_of_symbol(method)?;
                if method_name == "next" {
                    Some(raw)
                } else {
                    Some(self.get_type_with_facts(raw, TypeFacts::NE_UNDEFINED_OR_NULL)?)
                }
            }
            _ => None,
        };
        if method_type.is_some_and(|method_type| self.is_type_any(method_type)) {
            return Ok(Some(IterationTypesResult::Types(
                self.any_iteration_types(),
            )));
        }
        let method_signatures = match method_type {
            Some(method_type) => {
                self.get_signatures_of_type(method_type, crate::structural::SignatureKind::Call)?
            }
            None => Vec::new(),
        };
        if method_signatures.is_empty() {
            if let Some(error_node) = error_node {
                let diagnostic = if method_name == "next" {
                    resolver.must_have_a_next_method_diagnostic()
                } else {
                    resolver.must_be_a_method_diagnostic()
                };
                let built = self.create_error(Some(error_node), diagnostic, &[method_name]);
                match container {
                    Some(container) => container.errors.push(built),
                    None => {
                        self.push_error_diagnostic(built);
                    }
                }
            }
            return Ok(if method_name == "next" {
                Some(IterationTypesResult::No)
            } else {
                None
            });
        }
        // Single-signature Generator/Iterator member shortcut
        // (84401-84422): member-symbol identity against the GLOBAL's
        // table (lib-loaded only), mapped through the instantiation
        // mapper.
        if let Some(method_type) = method_type {
            let method_symbol = self.tables.type_of(method_type).symbol;
            if method_signatures.len() == 1 {
                if let Some(method_symbol) = method_symbol {
                    let global_generator = self.resolver_global_generator_type(resolver, false)?;
                    let global_iterator = self.resolver_global_iterator_type(resolver, false)?;
                    let member_of =
                        |state: &Self, global: TypeId| -> Option<tsrs2_binder::SymbolId> {
                            let symbol = state.tables.type_of(global).symbol?;
                            state.symbol_members(symbol).get(method_name).copied()
                        };
                    let is_generator_method =
                        member_of(self, global_generator) == Some(method_symbol);
                    let is_iterator_method = !is_generator_method
                        && member_of(self, global_iterator) == Some(method_symbol);
                    if is_generator_method || is_iterator_method {
                        let global_type = if is_generator_method {
                            global_generator
                        } else {
                            global_iterator
                        };
                        let mapper = self.links.ty(method_type).instantiated_mapper;
                        // tsc reads methodType.mapper unconditionally;
                        // a mapper-less method type here means the
                        // member was NOT an instantiation — fall
                        // through to the general path (unreachable in
                        // tsc's flows).
                        if let Some(mapper) = mapper {
                            let type_parameters: Vec<TypeId> =
                                match &self.tables.type_of(global_type).data {
                                    tsrs2_types::TypeData::GenericType {
                                        type_parameters, ..
                                    } => type_parameters.to_vec(),
                                    _ => Vec::new(),
                                };
                            if type_parameters.len() >= 3 {
                                let yield_type =
                                    self.get_mapped_type(type_parameters[0], mapper)?;
                                let return_type =
                                    self.get_mapped_type(type_parameters[1], mapper)?;
                                let next_type = if method_name == "next" {
                                    Some(self.get_mapped_type(type_parameters[2], mapper)?)
                                } else {
                                    None
                                };
                                return Ok(Some(IterationTypesResult::Types(
                                    self.create_iteration_types(
                                        Some(yield_type),
                                        Some(return_type),
                                        next_type,
                                    ),
                                )));
                            }
                        }
                    }
                }
            }
        }
        let mut method_parameter_types: Vec<TypeId> = Vec::new();
        let mut method_return_types: Vec<TypeId> = Vec::new();
        for &signature in &method_signatures {
            if method_name != "throw" && !self.signature_of(signature).parameters.is_empty() {
                method_parameter_types.push(self.get_type_at_position(signature, 0)?);
            }
            method_return_types.push(self.get_return_type_of_signature(signature)?);
        }
        let mut return_types: Vec<TypeId> = Vec::new();
        let mut next_type: Option<TypeId> = None;
        if method_name != "throw" {
            let method_parameter_type = if method_parameter_types.is_empty() {
                self.tables.intrinsics.unknown
            } else {
                self.get_union_type_ex(&method_parameter_types, UnionReduction::Literal)?
            };
            if method_name == "next" {
                next_type = Some(method_parameter_type);
            } else if method_name == "return" {
                let resolved = self
                    .resolve_iteration_type(resolver, method_parameter_type, error_node)?
                    .unwrap_or(self.tables.intrinsics.any);
                return_types.push(resolved);
            }
        }
        let method_return_type = if method_return_types.is_empty() {
            self.tables.intrinsics.never
        } else {
            self.get_intersection_type(
                &method_return_types,
                tsrs2_types::IntersectionFlags::default(),
            )?
        };
        let resolved_method_return_type = self
            .resolve_iteration_type(resolver, method_return_type, error_node)?
            .unwrap_or(self.tables.intrinsics.any);
        let iteration_types =
            self.get_iteration_types_of_iterator_result(resolved_method_return_type)?;
        let yield_type = match iteration_types {
            IterationTypesResult::No => {
                if let Some(error_node) = error_node {
                    let built = self.create_error(
                        Some(error_node),
                        resolver.must_have_a_value_diagnostic(),
                        &[method_name],
                    );
                    match container {
                        Some(container) => container.errors.push(built),
                        None => {
                            self.push_error_diagnostic(built);
                        }
                    }
                }
                return_types.push(self.tables.intrinsics.any);
                self.tables.intrinsics.any
            }
            IterationTypesResult::Types(types) => {
                return_types.push(types.return_type);
                types.yield_type
            }
        };
        let return_type = self.get_union_type_ex(&return_types, UnionReduction::Literal)?;
        Ok(Some(IterationTypesResult::Types(
            self.create_iteration_types(Some(yield_type), Some(return_type), next_type),
        )))
    }

    /// tsc-port: createGeneratorType @6.0.3
    /// tsc-hash: 5f86dfc5ef9e8ce45344225f20febcd2bd491ecc224e7f10ad057299acbe556f
    /// tsc-span: _tsc.js:78842-78873
    ///
    /// Global Generator missing → IterableIterator fallback; BOTH
    /// missing → the reportErrors=true IterableIterator probe (2318
    /// under noLib) + emptyObjectType.
    pub(crate) fn create_generator_type(
        &mut self,
        yield_type: TypeId,
        return_type: TypeId,
        next_type: TypeId,
        is_async_generator: bool,
    ) -> CheckResult2<TypeId> {
        let resolver = if is_async_generator {
            IterResolver::Async
        } else {
            IterResolver::Sync
        };
        let global_generator = self.resolver_global_generator_type(resolver, false)?;
        let unknown = self.tables.intrinsics.unknown;
        let yield_type = self
            .resolve_iteration_type(resolver, yield_type, None)?
            .unwrap_or(unknown);
        let return_type = self
            .resolve_iteration_type(resolver, return_type, None)?
            .unwrap_or(unknown);
        if global_generator == self.empty_generic_type {
            let global_iterable_iterator =
                self.resolver_global_iterable_iterator_type(resolver, false)?;
            if global_iterable_iterator != self.empty_generic_type {
                return Ok(self.create_type_from_generic_global_type(
                    global_iterable_iterator,
                    &[yield_type, return_type, next_type],
                ));
            }
            self.resolver_global_iterable_iterator_type(resolver, true)?;
            return Ok(self.empty_object_type);
        }
        Ok(self.create_type_from_generic_global_type(
            global_generator,
            &[yield_type, return_type, next_type],
        ))
    }

    /// tsc-port: checkGeneratorInstantiationAssignabilityToReturnType @6.0.3
    /// tsc-hash: a37af9bd34fd34ca69122da325f52abee123ac7a5286d4b5c7a7d3a0336811cd
    /// tsc-span: _tsc.js:81356-81362
    pub(crate) fn check_generator_instantiation_assignability_to_return_type(
        &mut self,
        return_type: TypeId,
        function_flags: u32,
        error_node: Option<NodeId>,
    ) -> CheckResult2<bool> {
        let is_async = function_flags & crate::functions::FUNCTION_FLAGS_ASYNC != 0;
        let yield_type = self
            .get_iteration_type_of_generator_function_return_type(
                IterationTypeKind::YIELD,
                return_type,
                is_async,
            )?
            .unwrap_or(self.tables.intrinsics.any);
        let generator_return_type = self
            .get_iteration_type_of_generator_function_return_type(
                IterationTypeKind::RETURN,
                return_type,
                is_async,
            )?
            .unwrap_or(yield_type);
        let next_type = self
            .get_iteration_type_of_generator_function_return_type(
                IterationTypeKind::NEXT,
                return_type,
                is_async,
            )?
            .unwrap_or(self.tables.intrinsics.unknown);
        let generator_instantiation =
            self.create_generator_type(yield_type, generator_return_type, next_type, is_async)?;
        self.check_type_assignable_to(
            generator_instantiation,
            return_type,
            error_node,
            &diagnostics::Type_0_is_not_assignable_to_type_1,
        )
    }

    // ---- generator return-type readers ----

    /// tsc-port: getIterationTypeOfGeneratorFunctionReturnType @6.0.3
    /// tsc-hash: 0d5bbdd05b9448567e7abce3764dd066db2aa2f9ecba1b30cd5f04da51a0b7bf
    /// tsc-span: _tsc.js:84470-84476
    pub(crate) fn get_iteration_type_of_generator_function_return_type(
        &mut self,
        kind: IterationTypeKind,
        return_type: TypeId,
        is_async_generator: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if self.is_type_any(return_type) {
            return Ok(None);
        }
        let iteration_types = self.get_iteration_types_of_generator_function_return_type(
            return_type,
            is_async_generator,
        )?;
        Ok(iteration_types.map(|types| types.by_kind(kind)))
    }

    /// tsc-port: getIterationTypesOfGeneratorFunctionReturnType @6.0.3
    /// tsc-hash: 9f234f0e72b31fd5bc26e66bb31f9ae141eab44120fdffba84ddd0fb95a39bc0
    /// tsc-span: _tsc.js:84477-84496
    pub(crate) fn get_iteration_types_of_generator_function_return_type(
        &mut self,
        ty: TypeId,
        is_async_generator: bool,
    ) -> CheckResult2<Option<IterationTypes>> {
        if self.is_type_any(ty) {
            return Ok(Some(self.any_iteration_types()));
        }
        let use_ = if is_async_generator {
            IterationUse::ASYNC_GENERATOR_RETURN_TYPE
        } else {
            IterationUse::GENERATOR_RETURN_TYPE
        };
        let resolver = if is_async_generator {
            IterResolver::Async
        } else {
            IterResolver::Sync
        };
        if let Some(types) = self.get_iteration_types_of_iterable(ty, use_, None)? {
            return Ok(Some(types));
        }
        self.get_iteration_types_of_iterator(ty, resolver, None, &mut None)
    }
}

/// tsc-port: isES2015OrLaterIterable @6.0.3
/// tsc-hash: e3288357c0c2d9e4cc8f746d7907c816aa7f01561ff6e9a70d4fd84feece10eb
/// tsc-span: _tsc.js:83997-84012
fn is_es2015_or_later_iterable(name: &str) -> bool {
    matches!(
        name,
        "Float32Array"
            | "Float64Array"
            | "Int16Array"
            | "Int32Array"
            | "Int8Array"
            | "NodeList"
            | "Uint16Array"
            | "Uint32Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
    )
}
