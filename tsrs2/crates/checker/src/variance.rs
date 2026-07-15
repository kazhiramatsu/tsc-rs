//! Variance measurement (M4 5.3b): getVariances/getVariancesWorker
//! over marker-type probes, createMarkerType, and the helpers the
//! relateVariances arms consume. The measured lists live in
//! SymbolLinks.variances — LinkSlot's Resolving state IS tsc's
//! shared-emptyArray in-progress sentinel (getVariances call sites
//! answer Ternary.Unknown while a measurement is on the stack).

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_types::{ModifierFlags, ObjectFlags, TypeData, TypeFlags, TypeId, VarianceFlags};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Unsupported, VarianceHandlerFrame};

/// tsc arrayVariances (46460): `[VarianceFlags.Covariant]` — shared by
/// both global array types and every tuple target
/// (typeArgumentsRelatedTo pads missing entries covariantly).
pub(crate) const ARRAY_VARIANCES: &[VarianceFlags] = &[VarianceFlags::COVARIANT];

/// A getVariances answer. `InProgress` is the identity test
/// `variances === emptyArray` at the call sites (66084, 66425).
#[derive(Clone, Debug)]
pub(crate) enum VariancesResult {
    InProgress,
    Known(Box<[VarianceFlags]>),
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getVariances @6.0.3
    /// tsc-hash: 1e9d0e5ee768931179190817e1e6d172a87e0cf756d67f73d9eac22fde95e9ac
    /// tsc-span: _tsc.js:67306-67308
    pub(crate) fn get_variances(&mut self, ty: TypeId) -> CheckResult2<VariancesResult> {
        if ty == self.global_array_type()?
            || ty == self.global_readonly_array_type()?
            || self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::TUPLE)
        {
            return Ok(VariancesResult::Known(ARRAY_VARIANCES.into()));
        }
        let symbol = self
            .tables
            .type_of(ty)
            .symbol
            .expect("generic reference targets carry their symbol");
        let type_parameters = match &self.tables.type_of(ty).data {
            TypeData::GenericType {
                type_parameters, ..
            } => type_parameters.to_vec(),
            _ => unreachable!(
                "getVariances runs on same-target reference pairs, whose non-tuple \
                 targets are GenericTypes"
            ),
        };
        self.get_variances_worker(symbol, &type_parameters)
    }

    /// tsc-port: getAliasVariances @6.0.3
    /// tsc-hash: 376698f797bba63d51c9d84c710fbee8b53cad042ac55a78544c2f400f258850
    /// tsc-span: _tsc.js:67309-67311
    pub(crate) fn get_alias_variances(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<VariancesResult> {
        let type_parameters = self
            .links
            .symbol(symbol)
            .type_parameters
            .clone()
            .unwrap_or_default();
        self.get_variances_worker(symbol, &type_parameters)
    }

    /// tsc-port: getVariancesWorker @6.0.3
    /// tsc-hash: b3d0b6716d244e10697b68ff53caea57f5c265658ccd109699d7b82dc3140e05
    /// tsc-span: _tsc.js:67312-67359
    ///
    /// The tracing push/pop is elided. On Unsupported unwind the
    /// Resolving sentinel reverts (tsc cannot fail here) and the
    /// inVarianceComputation/resolutionStart saves are restored on
    /// both paths.
    fn get_variances_worker(
        &mut self,
        symbol: SymbolId,
        type_parameters: &[TypeId],
    ) -> CheckResult2<VariancesResult> {
        match &self.links.symbol(symbol).variances {
            LinkSlot::Resolved(list) => return Ok(VariancesResult::Known(list.clone())),
            LinkSlot::Resolving => return Ok(VariancesResult::InProgress),
            LinkSlot::Vacant => {}
        }
        let old_variance_computation = self.in_variance_computation;
        let save_resolution_start = self.resolution_start;
        if !self.in_variance_computation {
            self.in_variance_computation = true;
            self.resolution_start = self.resolution_targets.len();
        }
        self.links
            .set_symbol_variances(self.speculation_depth, symbol, LinkSlot::Resolving);
        let mut variances: Vec<VarianceFlags> = Vec::with_capacity(type_parameters.len());
        let mut failure: Option<Unsupported> = None;
        for &tp in type_parameters {
            match self.measure_type_parameter_variance(symbol, tp) {
                Ok(variance) => variances.push(variance),
                Err(err) => {
                    failure = Some(err);
                    break;
                }
            }
        }
        if !old_variance_computation {
            self.in_variance_computation = false;
            self.resolution_start = save_resolution_start;
        }
        match failure {
            Some(err) => {
                self.links.revert_symbol_variances(symbol);
                Err(err)
            }
            None => {
                let list: Box<[VarianceFlags]> = variances.into();
                self.links.set_symbol_variances(
                    self.speculation_depth,
                    symbol,
                    LinkSlot::Resolved(list.clone()),
                );
                Ok(VariancesResult::Known(list))
            }
        }
    }

    /// One iteration of the 67325-67350 loop: the in/out modifier fast
    /// path, else marker measurement under a Base handler frame.
    fn measure_type_parameter_variance(
        &mut self,
        symbol: SymbolId,
        tp: TypeId,
    ) -> CheckResult2<VarianceFlags> {
        let modifiers = self.get_type_parameter_modifiers(tp);
        if modifiers.intersects(ModifierFlags::OUT) {
            return Ok(if modifiers.intersects(ModifierFlags::IN) {
                VarianceFlags::INVARIANT
            } else {
                VarianceFlags::COVARIANT
            });
        }
        if modifiers.intersects(ModifierFlags::IN) {
            return Ok(VarianceFlags::CONTRAVARIANT);
        }
        self.variance_handler_stack
            .push(VarianceHandlerFrame::Base {
                unmeasurable: false,
                unreliable: false,
            });
        let outcome = self.measure_type_parameter_variance_worker(symbol, tp);
        let (unmeasurable, unreliable) = match self.variance_handler_stack.pop() {
            Some(VarianceHandlerFrame::Base {
                unmeasurable,
                unreliable,
            }) => (unmeasurable, unreliable),
            _ => unreachable!("the Base frame pushed above is still on top"),
        };
        let mut variance = outcome?;
        if unmeasurable {
            variance =
                VarianceFlags::from_bits(variance.bits() | VarianceFlags::UNMEASURABLE.bits());
        }
        if unreliable {
            variance = VarianceFlags::from_bits(variance.bits() | VarianceFlags::UNRELIABLE.bits());
        }
        Ok(variance)
    }

    fn measure_type_parameter_variance_worker(
        &mut self,
        symbol: SymbolId,
        tp: TypeId,
    ) -> CheckResult2<VarianceFlags> {
        let marker_super = self.marker_super_type;
        let marker_sub = self.marker_sub_type;
        let marker_other = self.marker_other_type;
        let type_with_super = self.create_marker_type(symbol, tp, marker_super)?;
        let type_with_sub = self.create_marker_type(symbol, tp, marker_sub)?;
        let mut bits = 0;
        if self.is_type_assignable_to(type_with_sub, type_with_super)? {
            bits |= VarianceFlags::COVARIANT.bits();
        }
        if self.is_type_assignable_to(type_with_super, type_with_sub)? {
            bits |= VarianceFlags::CONTRAVARIANT.bits();
        }
        if bits == VarianceFlags::BIVARIANT.bits() {
            let type_with_other = self.create_marker_type(symbol, tp, marker_other)?;
            if self.is_type_assignable_to(type_with_other, type_with_super)? {
                bits = VarianceFlags::INDEPENDENT.bits();
            }
        }
        Ok(VarianceFlags::from_bits(bits))
    }

    /// tsc-port: createMarkerType @6.0.3
    /// tsc-hash: 417c67e9d5d3bf13a2b68381267251412fbf6fb9a4b780fcc65f34c5c4df6261
    /// tsc-span: _tsc.js:67360-67369
    pub(crate) fn create_marker_type(
        &mut self,
        symbol: SymbolId,
        source_tp: TypeId,
        target_marker: TypeId,
    ) -> CheckResult2<TypeId> {
        let mapper = self.make_unary_type_mapper(source_tp, target_marker);
        let ty = self.get_declared_type_of_symbol_for_variance(symbol)?;
        if ty == self.tables.intrinsics.error {
            return Ok(ty);
        }
        let result = if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(tsrs2_types::SymbolFlags::TYPE_ALIAS)
        {
            let type_parameters = self
                .links
                .symbol(symbol)
                .type_parameters
                .clone()
                .unwrap_or_default();
            let arguments = self.instantiate_types(&type_parameters, mapper)?;
            self.get_type_alias_instantiation(
                symbol,
                Some(&arguments),
                /*alias_symbol*/ None,
                /*alias_type_arguments*/ None,
            )?
        } else {
            let type_parameters = match &self.tables.type_of(ty).data {
                TypeData::GenericType {
                    type_parameters, ..
                } => type_parameters.to_vec(),
                _ => unreachable!(
                    "variance measurement runs over generic class/interface declared types"
                ),
            };
            let arguments = self.instantiate_types(&type_parameters, mapper)?;
            self.tables.create_type_reference(ty, &arguments)
        };
        self.marker_types.insert(result);
        Ok(result)
    }

    /// getDeclaredTypeOfSymbol's variance slice: getVariancesWorker
    /// only measures class/interface targets and alias symbols.
    pub(crate) fn get_declared_type_of_symbol_for_variance(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(tsrs2_types::SymbolFlags::CLASS | tsrs2_types::SymbolFlags::INTERFACE) {
            return self.get_declared_type_of_class_or_interface(symbol);
        }
        if flags.intersects(tsrs2_types::SymbolFlags::TYPE_ALIAS) {
            return self.get_declared_type_of_type_alias(symbol);
        }
        unreachable!("variance symbols are class/interface/alias by caller guarantee: {flags:?}")
    }

    /// tsc-port: isMarkerType @6.0.3
    /// tsc-hash: d70559b4cb00c972ab785482390423f7a6b140cd24e0cec0a5c651a465ee545e
    /// tsc-span: _tsc.js:67370-67372
    pub(crate) fn is_marker_type(&self, ty: TypeId) -> bool {
        self.marker_types.contains(&ty)
    }

    /// tsc-port: getTypeParameterModifiers @6.0.3
    /// tsc-hash: 4d3743d83604dfbfe4837773b9ca468725d514ec6b76b25d406a7e715cbf9bca
    /// tsc-span: _tsc.js:67373-67376
    ///
    /// getEffectiveModifierFlags reduces to the syntactic flags in TS
    /// files (JSDoc modifiers never parse). Marker parameters are
    /// symbol-less and answer None.
    pub(crate) fn get_type_parameter_modifiers(&self, tp: TypeId) -> ModifierFlags {
        let Some(symbol) = self.tables.type_of(tp).symbol else {
            return ModifierFlags::NONE;
        };
        let mut modifiers = 0;
        for &declaration in &self.binder.symbol(symbol).declarations {
            modifiers |= node_util::get_syntactic_modifier_flags(
                self.binder.source_of_node(declaration),
                declaration,
            )
            .bits();
        }
        ModifierFlags::from_bits(
            modifiers
                & (ModifierFlags::IN.bits()
                    | ModifierFlags::OUT.bits()
                    | ModifierFlags::CONST.bits()),
        )
    }

    /// tsc-port: hasCovariantVoidArgument @6.0.3
    /// tsc-hash: 4f70dbe428ba400f2aa48b3c78027ffedb002523b48d3e14bb768aa3ba5cf9c2
    /// tsc-span: _tsc.js:67377-67384
    pub(crate) fn has_covariant_void_argument(
        &self,
        type_arguments: &[TypeId],
        variances: &[VarianceFlags],
    ) -> bool {
        for (i, &variance) in variances.iter().enumerate() {
            if variance.bits() & VarianceFlags::VARIANCE_MASK.bits()
                == VarianceFlags::COVARIANT.bits()
                && self
                    .tables
                    .flags_of(type_arguments[i])
                    .intersects(TypeFlags::VOID)
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, RelationComparisonResult, VarianceFlags};

    use crate::links::LinkSlot;
    use crate::relate::RelationKind;
    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), run)
    }

    fn annotation_type(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let annotation = find_probe_annotation(state.binder.source(0), name)
            .expect("declared var with annotation");
        state
            .get_type_from_type_node(annotation)
            .expect("annotation resolves")
    }

    fn measured_variances(state: &CheckerState, name: &str) -> Vec<VarianceFlags> {
        let symbol = *state.globals.get(name).expect("global interface symbol");
        match &state.links.symbol(symbol).variances {
            LinkSlot::Resolved(list) => list.to_vec(),
            other => panic!("variances not measured for {name}: {other:?}"),
        }
    }

    #[test]
    fn structural_measurement_covers_the_four_shapes() {
        with_state(
            "interface Out2<T> { x: T }\n\
             interface Contra<T> { f: (x: T) => void }\n\
             interface Inv<T> { f: (x: T) => T }\n\
             interface Empty<T> { }\n\
             declare var a: Out2<\"a\">;\ndeclare var b: Out2<string>;\n\
             declare var c: Contra<\"a\">;\ndeclare var d: Contra<string>;\n\
             declare var e: Inv<\"a\">;\ndeclare var f: Inv<string>;\n\
             declare var g: Empty<\"a\">;\ndeclare var h: Empty<string>;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                // Covariant: Out2<"a"> → Out2<string>, not back.
                assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
                assert_eq!(state.is_type_assignable_to(b, a), Ok(false));
                assert_eq!(
                    measured_variances(state, "Out2"),
                    vec![VarianceFlags::COVARIANT]
                );
                // Contravariant: Contra<string> → Contra<"a">, not back.
                let c = annotation_type(state, "c");
                let d = annotation_type(state, "d");
                assert_eq!(state.is_type_assignable_to(d, c), Ok(true));
                assert_eq!(state.is_type_assignable_to(c, d), Ok(false));
                assert_eq!(
                    measured_variances(state, "Contra"),
                    vec![VarianceFlags::CONTRAVARIANT]
                );
                // Invariant: neither direction.
                let e = annotation_type(state, "e");
                let f = annotation_type(state, "f");
                assert_eq!(state.is_type_assignable_to(e, f), Ok(false));
                assert_eq!(state.is_type_assignable_to(f, e), Ok(false));
                assert_eq!(
                    measured_variances(state, "Inv"),
                    vec![VarianceFlags::INVARIANT]
                );
                // Independent (67335-67337: bivariant probes promote):
                // unused parameters relate regardless of arguments.
                let g = annotation_type(state, "g");
                let h = annotation_type(state, "h");
                assert_eq!(state.is_type_assignable_to(g, h), Ok(true));
                assert_eq!(state.is_type_assignable_to(h, g), Ok(true));
                assert_eq!(
                    measured_variances(state, "Empty"),
                    vec![VarianceFlags::INDEPENDENT]
                );
            },
        );
    }

    #[test]
    fn modifier_fast_path_skips_measurement() {
        with_state(
            "interface O<out T> { x: T }\ninterface I<in T> { f: (x: T) => void }\n\
             interface IO<in out T> { x: T }\n\
             declare var a: O<\"a\">;\ndeclare var b: O<string>;\n\
             declare var c: IO<\"a\">;\ndeclare var d: IO<string>;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
                assert_eq!(
                    measured_variances(state, "O"),
                    vec![VarianceFlags::COVARIANT]
                );
                // in out → Invariant without probes (67326-67328).
                let c = annotation_type(state, "c");
                let d = annotation_type(state, "d");
                assert_eq!(state.is_type_assignable_to(c, d), Ok(false));
                assert_eq!(state.is_type_assignable_to(d, c), Ok(false));
                assert_eq!(
                    measured_variances(state, "IO"),
                    vec![VarianceFlags::INVARIANT]
                );
                // `in T` never measured until something relates it —
                // the slot stays vacant, proving the fast path.
                let i_symbol = *state.globals.get("I").expect("interface I");
                assert!(matches!(
                    state.links.symbol(i_symbol).variances,
                    LinkSlot::Vacant
                ));
            },
        );
    }

    #[test]
    fn alias_variances_drive_the_same_alias_fast_path() {
        with_state(
            "type Box<T> = { x: T };\n\
             declare var a: Box<\"a\">;\ndeclare var b: Box<string>;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                assert_eq!(state.is_type_assignable_to(a, b), Ok(true));
                assert_eq!(state.is_type_assignable_to(b, a), Ok(false));
                let box_symbol = *state.globals.get("Box").expect("alias Box");
                assert_eq!(
                    match &state.links.symbol(box_symbol).variances {
                        LinkSlot::Resolved(list) => list.to_vec(),
                        other => panic!("alias variances unmeasured: {other:?}"),
                    },
                    vec![VarianceFlags::COVARIANT]
                );
            },
        );
    }

    #[test]
    fn template_members_mark_unreliable_variance_and_cache_entries() {
        with_state(
            "interface Tmpl<T extends string> { x: `a${T}` }\n\
             declare var a: Tmpl<\"a\">;\ndeclare var b: Tmpl<string>;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                let _ = state.is_type_assignable_to(a, b);
                let variances = measured_variances(state, "Tmpl");
                assert_eq!(variances.len(), 1);
                assert!(
                    variances[0].intersects(VarianceFlags::UNRELIABLE),
                    "template-vs-template relations under measurement fire the \
                     unreliable marker (66279): {variances:?}"
                );
                // The measurement's inner relation writes persisted the
                // ReportsUnreliable bit into the assignable cache
                // (65853/65865) — the 5.3b format extension.
                assert!(
                    state
                        .relations
                        .cache(RelationKind::Assignable)
                        .values()
                        .any(|entry| entry
                            .intersects(RelationComparisonResult::REPORTS_UNRELIABLE)),
                    "no cache entry carries ReportsUnreliable"
                );
            },
        );
    }
}
