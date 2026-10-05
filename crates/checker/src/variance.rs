//! Variance measurement (M4 5.3b): getVariances/getVariancesWorker
//! over marker-type probes, createMarkerType, and the helpers the
//! relateVariances arms consume. The measured lists live in
//! SymbolLinks.variances. A measurement in progress is an entry of
//! `CheckerState::variance_stack` (tsgo's `varianceStack`); an empty
//! stored list is tsgo's empty slice, which the getVariances call sites
//! answer with Ternary.Unknown.

use tsc_binder::{node_util, SymbolId};
use tsc_types::{
    ModifierFlags, ObjectFlags, RelationComparisonResult, TypeData, TypeFlags, TypeId,
    VarianceFlags,
};

use crate::links::LinkSlot;
use crate::state::{CheckAbort, CheckResult, CheckerState};
use tsc_types::perf::{self, PerfCounter};

/// tsc arrayVariances (46460): `[VarianceFlags.Covariant]` — shared by
/// both global array types and every tuple target
/// (typeArgumentsRelatedTo pads missing entries covariantly).
pub(crate) const ARRAY_VARIANCES: &[VarianceFlags] = &[VarianceFlags::COVARIANT];

/// A getVariances answer. `Empty` is tsgo's `len(variances) == 0` at the
/// call sites (checker/relater.go:3427, 3866): the generic type has no
/// variance information, because its own measurement asked for it.
#[derive(Clone, Debug)]
pub(crate) enum VariancesResult {
    Empty,
    Known(Box<[VarianceFlags]>),
}

/// tsgo VarianceStackEntry (checker/checker.go): a generic type whose
/// variances are being measured, with the type parameters to measure.
#[derive(Clone, Debug)]
pub(crate) struct VarianceStackEntry {
    pub(crate) symbol: SymbolId,
    pub(crate) type_parameters: Box<[TypeId]>,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getVariances @6.0.3
    /// tsc-hash: 1e9d0e5ee768931179190817e1e6d172a87e0cf756d67f73d9eac22fde95e9ac
    /// tsc-span: _tsc.js:67306-67308
    pub(crate) fn get_variances(&mut self, ty: TypeId) -> CheckResult<VariancesResult> {
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
    pub(crate) fn get_alias_variances(&mut self, symbol: SymbolId) -> CheckResult<VariancesResult> {
        let type_parameters = self
            .links
            .symbol_cold()
            .type_parameters
            .get(symbol)
            .clone()
            .unwrap_or_default();
        self.get_variances_worker(symbol, &type_parameters)
    }

    /// tsgo-port: Checker.getVariancesWorker @7.1
    /// (checker/relater.go:1334-1434)
    ///
    /// tsc 6.0 marked a measurement in progress in the links and let the
    /// generic type that was entered first measure the others of its cycle
    /// from inside, so the variances depended on which type was compared
    /// first. tsgo keeps the measurements in progress on a stack: when one
    /// asks for a type that is already on it, the measurement restarts from
    /// the type of that cycle with the smallest symbol, and every entry
    /// point gives the same variances.
    ///
    /// The tracing push/pop is elided. On CheckAbort unwind the stack entry
    /// is popped, the resolutionStart save is restored and the empty marker
    /// of the abandoned measurement is cleared (tsc cannot fail here).
    fn get_variances_worker(
        &mut self,
        symbol: SymbolId,
        type_parameters: &[TypeId],
    ) -> CheckResult<VariancesResult> {
        if let Some(stored) = self.stored_variances(symbol) {
            return Ok(stored);
        }
        match self
            .variance_stack
            .iter()
            .position(|entry| entry.symbol == symbol)
        {
            None => self.measure_variances(symbol, type_parameters)?,
            Some(stack_index) => {
                // A circularity. The variances depend on where the cycle is
                // entered, so the measurement restarts from the generic type
                // with the smallest symbol in the circular region.
                perf::bump(PerfCounter::SentinelVarianceInProgress);
                let mut min_index = stack_index;
                {
                    let order = crate::type_order::order_ctx!(self);
                    for index in stack_index + 1..self.variance_stack.len() {
                        if tsc_types::TypeOrderContext::compare_symbols(
                            &order,
                            Some(self.variance_stack[index].symbol),
                            Some(self.variance_stack[min_index].symbol),
                        ) == std::cmp::Ordering::Less
                        {
                            min_index = index;
                        }
                    }
                }
                if min_index > stack_index {
                    let saved = std::mem::take(&mut self.variance_stack);
                    let entry = saved[min_index].clone();
                    let restarted = self.get_variances_worker(entry.symbol, &entry.type_parameters);
                    self.variance_stack = saved;
                    restarted?;
                }
                // An empty list marks that this type's variances cannot be
                // computed here; its type arguments relate covariantly.
                if self.stored_variances(symbol).is_none() {
                    self.links.set_symbol_variances(
                        self.speculation_depth,
                        symbol,
                        LinkSlot::Resolved(Box::default()),
                    );
                }
            }
        }
        Ok(self
            .stored_variances(symbol)
            .expect("both arms store the symbol's variances"))
    }

    /// The stored list as a getVariances answer; `None` is tsgo's nil
    /// `links.variances`.
    fn stored_variances(&self, symbol: SymbolId) -> Option<VariancesResult> {
        match self.links.symbol_cold().variances.get(symbol) {
            LinkSlot::Resolved(list) if list.is_empty() => Some(VariancesResult::Empty),
            LinkSlot::Resolved(list) => Some(VariancesResult::Known(list.clone())),
            LinkSlot::Resolving => unreachable!("variances have no in-progress sentinel"),
            LinkSlot::Vacant => None,
        }
    }

    /// The measuring arm of getVariancesWorker (relater.go:1351-1409).
    fn measure_variances(
        &mut self,
        symbol: SymbolId,
        type_parameters: &[TypeId],
    ) -> CheckResult<()> {
        let save_resolution_start = self.resolution_start;
        if self.variance_stack.is_empty() {
            self.resolution_start = self.resolution_targets.len();
        }
        self.variance_stack.push(VarianceStackEntry {
            symbol,
            type_parameters: type_parameters.into(),
        });
        let mut variances: Vec<VarianceFlags> = Vec::with_capacity(type_parameters.len());
        let mut failure: Option<CheckAbort> = None;
        for &tp in type_parameters {
            match self.measure_type_parameter_variance(symbol, tp) {
                Ok(variance) => {
                    // A measurement restarted for a circularity may have
                    // stored this type's variances already.
                    if matches!(
                        self.stored_variances(symbol),
                        Some(VariancesResult::Known(_))
                    ) {
                        break;
                    }
                    variances.push(variance);
                }
                Err(err) => {
                    failure = Some(err);
                    break;
                }
            }
        }
        self.variance_stack.pop();
        if self.variance_stack.is_empty() {
            self.resolution_start = save_resolution_start;
        }
        let restarted = matches!(
            self.stored_variances(symbol),
            Some(VariancesResult::Known(_))
        );
        match failure {
            Some(err) => {
                if !restarted {
                    self.links.clear_symbol_variances(symbol);
                }
                Err(err)
            }
            None => {
                // Store the results unless a restarted computation has
                // already stored them.
                if !restarted {
                    self.links.set_symbol_variances(
                        self.speculation_depth,
                        symbol,
                        LinkSlot::Resolved(variances.into()),
                    );
                }
                Ok(())
            }
        }
    }

    /// One iteration of the measuring loop (relater.go:1358-1394): the
    /// in/out modifier fast path, else marker measurement with the
    /// reliability flags cleared, which it reads and puts back.
    fn measure_type_parameter_variance(
        &mut self,
        symbol: SymbolId,
        tp: TypeId,
    ) -> CheckResult<VarianceFlags> {
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
        let save_reliability_flags = self.reliability_flags;
        self.reliability_flags = RelationComparisonResult::NONE;
        let outcome = self.measure_type_parameter_variance_worker(symbol, tp);
        let reliability_flags = self.reliability_flags;
        self.reliability_flags = save_reliability_flags;
        let unmeasurable =
            reliability_flags.intersects(RelationComparisonResult::REPORTS_UNMEASURABLE);
        let unreliable = reliability_flags.intersects(RelationComparisonResult::REPORTS_UNRELIABLE);
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
    ) -> CheckResult<VarianceFlags> {
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
    ) -> CheckResult<TypeId> {
        let mapper = self.make_unary_type_mapper(source_tp, target_marker);
        let ty = self.get_declared_type_of_symbol_for_variance(symbol)?;
        if ty == self.tables.intrinsics.error {
            return Ok(ty);
        }
        let result = if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(tsc_types::SymbolFlags::TYPE_ALIAS)
        {
            let type_parameters = self
                .links
                .symbol_cold()
                .type_parameters
                .get(symbol)
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
    /// tsc-port: getDeclaredTypeOfSymbol @6.0.3
    /// tsc-hash: 04bcaad92415ace0f6299d6345cbed24b36475891a61b84d6ab0efe78a103eaf
    /// tsc-span: _tsc.js:57502-57504
    pub(crate) fn get_declared_type_of_symbol_for_variance(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult<TypeId> {
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(tsc_types::SymbolFlags::CLASS | tsc_types::SymbolFlags::INTERFACE) {
            return self.get_declared_type_of_class_or_interface(symbol);
        }
        if flags.intersects(tsc_types::SymbolFlags::TYPE_ALIAS) {
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
    /// Marker parameters are symbol-less and answer None.
    pub(crate) fn get_type_parameter_modifiers(&self, tp: TypeId) -> ModifierFlags {
        let Some(symbol) = self.tables.type_of(tp).symbol else {
            return ModifierFlags::NONE;
        };
        let mut modifiers = 0;
        for &declaration in &self.binder.symbol(symbol).declarations {
            modifiers |= node_util::get_effective_modifier_flags(
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
#[path = "../tests/unit/variance/tests.rs"]
mod tests;
