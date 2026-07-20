//! M6 7.1: the inference data model (m6-inference-calls-steps.md 7.1)
//! — InferenceInfo/InferenceContext (tsc 68238-68330) with the
//! fixing/non-fixing Deferred mapper pair and the
//! createOuterReturnMapper cache slot (63385).
//!
//! Contexts are arena-allocated on CheckerState so InferenceContextId
//! equality IS tsc's context object identity, exactly like the
//! `mappers` arena. The arena is E-class speculation state
//! (append-only, never truncated): tsc context mutations deliberately
//! SURVIVE failed candidate trials — chooseOverload's NORMAL-mode
//! re-run reuses the SAME context (76842-76844), so candidate
//! accumulation across trials is by design, not a leak.
//!
//! Frontier stubs behind this model (production-unreachable until 7.4
//! wires inferTypeArguments into resolveCall — every production
//! pushInferenceContext site still passes None): `infer_types` lands
//! at 7.2 (the candidate collector), `get_inferred_type` at 7.3
//! (resolution + the constraint clamp).

use tsrs2_syntax::{NodeId, SyntaxKind};
use tsrs2_types::{ContextFlags, InferenceFlags, InferencePriority, TypeId};

use crate::instantiate::{DeferredMapperTargets, MapperId, TypeMapper};
use crate::state::{CheckResult2, CheckerState, SignatureId, Unsupported};

/// Arena id — see the module doc for the identity/rollback contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InferenceContextId(pub u32);

/// tsc InferenceInfo (the createInferenceInfo 68300 literal).
///
/// `type_parameter` is the TypeParameter TYPE, not its symbol: the
/// deferred mappers' source scan compares type identities
/// (getMappedType 63341 `type === sources[i]`), and
/// createInferenceContext receives `signature.typeParameters` — a
/// type list (core-interfaces §6's SymbolId sketch is corrected
/// there).
///
/// `candidates`/`contra_candidates`: None = tsc undefined. tsc only
/// ever creates the arrays through appendIfUnique-style pushes, so a
/// present vec is non-empty — hasInferenceCandidates keys on
/// presence, not length.
#[derive(Clone, Debug)]
pub(crate) struct InferenceInfo {
    pub(crate) type_parameter: TypeId,
    pub(crate) candidates: Option<Vec<TypeId>>,
    pub(crate) contra_candidates: Option<Vec<TypeId>>,
    pub(crate) inferred_type: Option<TypeId>,
    #[allow(dead_code)] // consumer: 7.2 candidate recording (inferWithPriority)
    pub(crate) priority: Option<InferencePriority>,
    #[allow(dead_code)] // consumer: 7.2 isTypeParameterAtTopLevel record + 7.3 widen split
    pub(crate) top_level: bool,
    pub(crate) is_fixed: bool,
    #[allow(dead_code)] // consumer: 7.2 tuple-element inference (69113) + 7.4 (75969)
    pub(crate) implied_arity: Option<usize>,
}

/// intraExpressionInferenceSites element (68287 `{ node, type }`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntraExpressionInferenceSite {
    pub(crate) node: NodeId,
    pub(crate) ty: TypeId,
}

/// context.compareTypes — tsc stores a comparator function; the port
/// stores the closed set of comparators tsc ever passes. Only the
/// createInferenceContext default (68239 `compareTypes2 ||
/// compareTypesAssignable`) is constructible today; the two
/// non-default producers extend this enum when their stages land:
/// compareSignaturesRelated's relation-frame worker rides
/// instantiateSignatureInContextOf (64507, the M6 7.5 head rebuild)
/// and checkTypeRelatedTo's infer-source context passes its own
/// isRelatedToWorker (66368, M8 conditionals).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompareTypesFn {
    /// compareTypesAssignable — consumed by getInferredType's
    /// constraint clamp (69300-69306, stage 7.3).
    #[allow(dead_code)] // constructed by the 7.4 createInferenceContext callers
    Assignable,
}

/// tsc InferenceContext: the createInferenceContextWorker 68245
/// literal (`inferences`/`signature`/`flags`/`compareTypes`/`mapper`/
/// `nonFixingMapper`) plus the four lazily-attached fields —
/// `returnMapper` (inferTypeArguments 75960),
/// `intraExpressionInferenceSites` (68287),
/// `inferredTypeParameters` (80804), and the `outerReturnMapper`
/// cache slot (createOuterReturnMapper 63386).
///
/// `inferences` slot count and per-slot `type_parameter` are
/// CREATION-STABLE: the only post-creation slot write is
/// mergeInferences (80836 `target[i] = source[i]`), whose source
/// infos are built over the SAME type parameters
/// (80786 `createInferenceInfo(info.typeParameter)`) — so the
/// deferred mappers' dynamic source lookup (instantiate.rs) is
/// observationally identical to tsc's creation-time
/// `map(context.inferences, i => i.typeParameter)` snapshot.
///
/// CAUTION — that equivalence covers the SOURCE scan ONLY. tsc's
/// fixing thunks (68261-68267) also close over the per-slot
/// InferenceInfo OBJECT for the `isFixed` test-and-set, while the
/// port's fixing_mapper_target reads/writes the CURRENT slot. After
/// a mergeInferences slot replacement the two diverge: 80836
/// replaces fixed-but-candidateless rows too (hasInferenceCandidates
/// 80822 never consults isFixed; the fresh 80786 info starts
/// isFixed=false), leaving tsc split — the detached thunk object
/// stays isFixed=true (preamble skipped on the next fixing dispatch)
/// while the LIVE row stays unfixed (68710 keeps recording
/// candidates, 69266 widens as unfixed). A single live-slot bit
/// cannot represent that split; the 7.4 mergeInferences port must
/// carry the preamble-done state per creation-time info identity —
/// do NOT extend this equivalence argument to `is_fixed`.
#[derive(Clone, Debug)]
pub(crate) struct InferenceContext {
    pub(crate) inferences: Vec<InferenceInfo>,
    #[allow(dead_code)] // consumer: 7.4 inferTypeArguments (context.signature, 80781)
    pub(crate) signature: Option<SignatureId>,
    #[allow(dead_code)]
    // consumer: 7.3 NoDefault/AnyDefault resolution + 7.4 SkippedGenericFunction
    pub(crate) flags: InferenceFlags,
    #[allow(dead_code)] // consumer: 7.3 constraint clamp (69300-69306)
    pub(crate) compare_types: CompareTypesFn,
    pub(crate) mapper: MapperId,
    pub(crate) non_fixing_mapper: MapperId,
    pub(crate) return_mapper: Option<MapperId>,
    #[allow(dead_code)] // consumer: 7.4 chooseOverload via getSignatureInstantiation (76844)
    pub(crate) inferred_type_parameters: Option<Vec<TypeId>>,
    pub(crate) intra_expression_inference_sites: Option<Vec<IntraExpressionInferenceSite>>,
    pub(crate) outer_return_mapper: Option<MapperId>,
}

/// tsc-port: createInferenceInfo @6.0.3
/// tsc-hash: b8543167898e564c402412e78d583022b055ce90be42b406d1e6e65cd86b7ca4
/// tsc-span: _tsc.js:68300-68311
pub(crate) fn create_inference_info(type_parameter: TypeId) -> InferenceInfo {
    InferenceInfo {
        type_parameter,
        candidates: None,
        contra_candidates: None,
        inferred_type: None,
        priority: None,
        top_level: true,
        is_fixed: false,
        implied_arity: None,
    }
}

/// tsc-port: cloneInferenceInfo @6.0.3
/// tsc-hash: b2727c05ad747f673d134cbf87bedd45c7cdaee933753ff372d043eea42b3309
/// tsc-span: _tsc.js:68312-68323
///
/// Vec::clone is tsc's `.slice()` (fresh array, same elements);
/// None (undefined) passes through — the derived Clone is the exact
/// field-for-field copy.
pub(crate) fn clone_inference_info(inference: &InferenceInfo) -> InferenceInfo {
    inference.clone()
}

/// tsc-port: clearCachedInferences @6.0.3
/// tsc-hash: 4a40c69427fa90dd5e056a0db75857816296e9abd8914426f83215209a5410e7
/// tsc-span: _tsc.js:68279-68285
///
/// A free function over the inference slice because 7.2's inferTypes
/// call sites also run it on detached arrays (the higher-order path's
/// local `inferences`, 80786), not only on context-attached ones.
pub(crate) fn clear_cached_inferences(inferences: &mut [InferenceInfo]) {
    for inference in inferences {
        if !inference.is_fixed {
            inference.inferred_type = None;
        }
    }
}

/// tsc-port: hasInferenceCandidates @6.0.3
/// tsc-hash: 97e543d5df5fa2b530ef74413d28b145cca2471acc980367b325a14d7b932e3b
/// tsc-span: _tsc.js:80822-80824
pub(crate) fn has_inference_candidates(info: &InferenceInfo) -> bool {
    info.candidates.is_some() || info.contra_candidates.is_some()
}

impl<'a> CheckerState<'a> {
    /// tsrs-native: arena accessor (contexts are GC objects in tsc).
    pub(crate) fn inference_context(&self, id: InferenceContextId) -> &InferenceContext {
        &self.inference_context_arena[id.0 as usize]
    }

    /// tsrs-native: arena accessor (contexts are GC objects in tsc).
    pub(crate) fn inference_context_mut(
        &mut self,
        id: InferenceContextId,
    ) -> &mut InferenceContext {
        &mut self.inference_context_arena[id.0 as usize]
    }

    /// tsc-port: createInferenceContext @6.0.3
    /// tsc-hash: ad626687cae0e25a4f4a7bc1207da6be3340a2c91cd19e5cdcf1ab2925a8990b
    /// tsc-span: _tsc.js:68238-68240
    #[allow(dead_code)] // consumer: 7.4 inferTypeArguments/chooseOverload (75911/75957/76809/76947)
    pub(crate) fn create_inference_context(
        &mut self,
        type_parameters: &[TypeId],
        signature: Option<SignatureId>,
        flags: InferenceFlags,
        compare_types: Option<CompareTypesFn>,
    ) -> InferenceContextId {
        let inferences = type_parameters
            .iter()
            .map(|&tp| create_inference_info(tp))
            .collect();
        self.create_inference_context_worker(
            inferences,
            signature,
            flags,
            compare_types.unwrap_or(CompareTypesFn::Assignable),
        )
    }

    /// tsc-port: cloneInferenceContext @6.0.3
    /// tsc-hash: 5aa3854ba4be0abdcf2fdb0db180c640d4bf9ee27fa5c1fd8aa57be9e79dd3c9
    /// tsc-span: _tsc.js:68241-68243
    ///
    /// `context && ...` — None passes through. The clone starts from
    /// the cloned INFOS only: lazily-attached context fields
    /// (returnMapper, sites, inferredTypeParameters, outer cache) do
    /// not survive into the clone.
    pub(crate) fn clone_inference_context(
        &mut self,
        context: Option<InferenceContextId>,
        extra_flags: InferenceFlags,
    ) -> Option<InferenceContextId> {
        context.map(|id| {
            let ctx = self.inference_context(id);
            let inferences = ctx.inferences.iter().map(clone_inference_info).collect();
            let signature = ctx.signature;
            let flags = ctx.flags | extra_flags;
            let compare_types = ctx.compare_types;
            self.create_inference_context_worker(inferences, signature, flags, compare_types)
        })
    }

    /// tsc-port: createInferenceContextWorker @6.0.3
    /// tsc-hash: 803e3c0eb9aa71bf230c5ff225b334d6fdd7bf409ffd802623b8847ec88190f3
    /// tsc-span: _tsc.js:68244-68257
    ///
    /// tsc initializes mapper/nonFixingMapper to
    /// reportUnmeasurableMapper purely for object shape (its own
    /// 68251 comment) and overwrites both before the context escapes
    /// — unobservable, so the port allocates the Deferred pair
    /// directly (fixing first, matching 68254/68255 creation order).
    fn create_inference_context_worker(
        &mut self,
        inferences: Vec<InferenceInfo>,
        signature: Option<SignatureId>,
        flags: InferenceFlags,
        compare_types: CompareTypesFn,
    ) -> InferenceContextId {
        let id = InferenceContextId(self.inference_context_arena.len() as u32);
        let mapper = self.alloc_mapper(TypeMapper::Deferred(
            DeferredMapperTargets::InferenceFixing(id),
        ));
        let non_fixing_mapper = self.alloc_mapper(TypeMapper::Deferred(
            DeferredMapperTargets::InferenceNonFixing(id),
        ));
        self.inference_context_arena.push(InferenceContext {
            inferences,
            signature,
            flags,
            compare_types,
            mapper,
            non_fixing_mapper,
            return_mapper: None,
            inferred_type_parameters: None,
            intra_expression_inference_sites: None,
            outer_return_mapper: None,
        });
        id
    }

    /// tsc-port: cloneInferredPartOfContext @6.0.3
    /// tsc-hash: 275f26e3b1cc4ba518c7c218ced080fb34355ed6486b60ae64631a4095d185b6
    /// tsc-span: _tsc.js:68324-68327
    #[allow(dead_code)] // consumer: 7.4 returnMapper derivation (75960)
    pub(crate) fn clone_inferred_part_of_context(
        &mut self,
        context: InferenceContextId,
    ) -> Option<InferenceContextId> {
        let ctx = self.inference_context(context);
        let inferences: Vec<InferenceInfo> = ctx
            .inferences
            .iter()
            .filter(|info| has_inference_candidates(info))
            .map(clone_inference_info)
            .collect();
        if inferences.is_empty() {
            return None;
        }
        let signature = ctx.signature;
        let flags = ctx.flags;
        let compare_types = ctx.compare_types;
        Some(self.create_inference_context_worker(inferences, signature, flags, compare_types))
    }

    /// tsc-port: getMapperFromContext @6.0.3
    /// tsc-hash: 215681bda0692b7d5a62205f8b81998258ef2dbd6543d18c995c4529ab09ca1b
    /// tsc-span: _tsc.js:68328-68330
    pub(crate) fn get_mapper_from_context(
        &self,
        context: Option<InferenceContextId>,
    ) -> Option<MapperId> {
        context.map(|id| self.inference_context(id).mapper)
    }

    /// tsc-port: hasInferenceCandidatesOrDefault @6.0.3
    /// tsc-hash: eef4b0235e6b7525b6993feb5cf70616228c9e90ebae9f19790bf5a0f0cd5621
    /// tsc-span: _tsc.js:80825-80827
    pub(crate) fn has_inference_candidates_or_default(&self, info: &InferenceInfo) -> bool {
        info.candidates.is_some()
            || info.contra_candidates.is_some()
            || self.has_type_parameter_default(info.type_parameter)
    }

    /// tsc-port: addIntraExpressionInferenceSite @6.0.3
    /// tsc-hash: f190c5ebafcc465e2e77bcb7246e4693f5ccf8a5e618254c066958e83b8bf3f3
    /// tsc-span: _tsc.js:68286-68288
    ///
    /// Populated by object/array-literal/JSX checking (68286 callers,
    /// wired at 7.4); drained inside the fixing mapper before
    /// is_fixed is set; cleared without draining by
    /// checkExpressionWithContextualType (80567-80569).
    #[allow(dead_code)] // consumer: 7.4 object/array-literal/JSX site recording (68286 callers)
    pub(crate) fn add_intra_expression_inference_site(
        &mut self,
        context: InferenceContextId,
        node: NodeId,
        ty: TypeId,
    ) {
        self.inference_context_mut(context)
            .intra_expression_inference_sites
            .get_or_insert_with(Vec::new)
            .push(IntraExpressionInferenceSite { node, ty });
    }

    /// tsc-port: inferFromIntraExpressionSites @6.0.3
    /// tsc-hash: 8a7a8bea19f164faf65646b962689b6b31fd0470891914a6ce8f1e4c7225d6cf
    /// tsc-span: _tsc.js:68289-68299
    ///
    /// tsc clears the site list AFTER the full loop; an Err unwind
    /// mid-loop therefore leaves it in place — harmless, because
    /// Unsupported abandons the whole surrounding resolution and the
    /// context with it (contexts are per-resolution transients).
    pub(crate) fn infer_from_intra_expression_sites(
        &mut self,
        context: InferenceContextId,
    ) -> CheckResult2<()> {
        if self
            .inference_context(context)
            .intra_expression_inference_sites
            .is_some()
        {
            let sites = self
                .inference_context(context)
                .intra_expression_inference_sites
                .clone()
                .expect("checked Some above");
            for site in sites {
                let contextual_type = if self.kind_of(site.node) == SyntaxKind::MethodDeclaration {
                    self.get_contextual_type_for_object_literal_method(
                        site.node,
                        ContextFlags::NO_CONSTRAINTS,
                    )?
                } else {
                    self.get_contextual_type(site.node, ContextFlags::NO_CONSTRAINTS)?
                };
                if let Some(contextual_type) = contextual_type {
                    self.infer_types(context, site.ty, contextual_type)?;
                }
            }
            self.inference_context_mut(context)
                .intra_expression_inference_sites = None;
        }
        Ok(())
    }

    /// tsc-port: makeFixingMapperForContext @6.0.3
    /// tsc-hash: d8bccd84b8ba6a84e7fe16b9117aa296eab1453f625491bc0f58bfa4961e41f6
    /// tsc-span: _tsc.js:68258-68270
    ///
    /// The thunk body (68262-68267): get_mapped_type's Deferred arm
    /// dispatches here when `ty` matched inferences[index]'s type
    /// parameter. Order is load-bearing — drain the intra-expression
    /// sites and clear cached inferences BEFORE setting is_fixed
    /// (the row being fixed is still unfixed at clear time, so its
    /// own stale inferred_type is dropped too), then resolve.
    ///
    /// NOTE: the `is_fixed` test-and-set consults the CURRENT slot;
    /// tsc's thunk consults the creation-time info object. Equivalent
    /// only while nothing replaces slots — see the InferenceContext
    /// CAUTION before porting mergeInferences (7.4).
    pub(crate) fn fixing_mapper_target(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> CheckResult2<TypeId> {
        if !self.inference_context(context).inferences[index].is_fixed {
            self.infer_from_intra_expression_sites(context)?;
            clear_cached_inferences(&mut self.inference_context_mut(context).inferences);
            self.inference_context_mut(context).inferences[index].is_fixed = true;
        }
        self.get_inferred_type(context, index)
    }

    /// tsc-port: makeNonFixingMapperForContext @6.0.3
    /// tsc-hash: bb7541ff81ea6112f604b1135a6a73bf0633c2b11c892a6161e14869302e2f91
    /// tsc-span: _tsc.js:68271-68278
    pub(crate) fn non_fixing_mapper_target(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> CheckResult2<TypeId> {
        self.get_inferred_type(context, index)
    }

    /// tsc-port: createOuterReturnMapper @6.0.3
    /// tsc-hash: dbf215149bf9450aedc8e51f8166a45bc93be51494c3b370b4273b05f4e529dd
    /// tsc-span: _tsc.js:63385-63387
    ///
    /// `outerReturnMapper ??=` — one merged mapper per context,
    /// cached on the context. Lives here rather than instantiate.rs
    /// because it is context-cache machinery (consumed by
    /// inferTypeArguments' phase-a2 return inference, 75958).
    #[allow(dead_code)] // consumer: 7.4 inferTypeArguments phase-a2 (75958)
    pub(crate) fn create_outer_return_mapper(&mut self, context: InferenceContextId) -> MapperId {
        if let Some(cached) = self.inference_context(context).outer_return_mapper {
            return cached;
        }
        let return_mapper = self.inference_context(context).return_mapper;
        let clone = self
            .clone_inference_context(Some(context), InferenceFlags::NONE)
            .expect("Some in, Some out");
        let clone_mapper = self.inference_context(clone).mapper;
        let merged = self.merge_type_mappers(return_mapper, clone_mapper);
        self.inference_context_mut(context).outer_return_mapper = Some(merged);
        merged
    }

    /// tsc-deferred: M6 — inferTypes (68637), the stage 7.2 candidate
    /// collector; this stub is the fixing mapper's drain landing pad
    /// and is production-unreachable until 7.4 pushes real contexts.
    ///
    /// 7.2 re-cuts this seam: tsc's signature is inferences-ARRAY-
    /// first with `priority = 0, contravariant = false` defaults, and
    /// its callers include detached-array sites
    /// (inferReverseMappedTypeWorker 68438 `inferTypes([inference],
    /// ...)`; the 7.4 higher-order path 80788) plus non-default
    /// priorities (62679/66375) — widen the signature and pick the
    /// id-vs-slice receiver there (clear_cached_inferences above is
    /// already slice-shaped for that fork); do NOT wrap detached
    /// arrays in throwaway arena contexts.
    pub(crate) fn infer_types(
        &mut self,
        context: InferenceContextId,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<()> {
        let _ = (context, source, target);
        Err(Unsupported::new("inferTypes candidate collection (M6 7.2)"))
    }

    /// tsc-deferred: M6 — getInferredType (69271), the stage 7.3
    /// resolution + constraint clamp; until then any deferred-mapper
    /// dispatch that reaches resolution unwinds.
    pub(crate) fn get_inferred_type(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> CheckResult2<TypeId> {
        let _ = (context, index);
        Err(Unsupported::new(
            "getInferredType resolution and constraint clamp (M6 7.3)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{
        CompilerOptions, ContextFlags, InferenceFlags, SymbolFlags, TypeId, UnionReduction,
    };

    use super::CompareTypesFn;
    use crate::instantiate::{DeferredMapperTargets, TypeMapper};
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn declared_type_parameter(state: &mut CheckerState, name: &str) -> TypeId {
        let source = state.binder.source(0);
        let inside = source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::VariableDeclaration)
            .expect("var declaration");
        let symbol = state
            .resolve_name(
                Some(inside),
                name,
                SymbolFlags::TYPE_PARAMETER,
                None,
                false,
                false,
            )
            .expect("resolve_name")
            .expect("type parameter resolves");
        state.get_declared_type_of_type_parameter(symbol)
    }

    fn node_of_kind(state: &CheckerState, kind: tsrs2_syntax::SyntaxKind) -> tsrs2_syntax::NodeId {
        let source = state.binder.source(0);
        source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == kind)
            .expect("node of kind")
    }

    fn annotation_of_var(state: &CheckerState, name: &str) -> tsrs2_syntax::NodeId {
        crate::relpin::find_probe_annotation(state.binder.source(0), name)
            .expect("var with annotation")
    }

    const GENERIC_SRC: &str = "function f<T, U>() { var v: T; }\n";

    #[test]
    fn create_inference_context_initial_shape() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let ctx =
                    state.create_inference_context(&[t, u], None, InferenceFlags::NO_DEFAULT, None);
                let context = state.inference_context(ctx);
                assert_eq!(context.inferences.len(), 2);
                for (info, tp) in context.inferences.iter().zip([t, u]) {
                    assert_eq!(info.type_parameter, tp);
                    assert!(info.candidates.is_none());
                    assert!(info.contra_candidates.is_none());
                    assert!(info.inferred_type.is_none());
                    assert!(info.priority.is_none());
                    assert!(info.top_level, "createInferenceInfo topLevel: true (68307)");
                    assert!(!info.is_fixed, "createInferenceInfo isFixed: false (68308)");
                    assert!(info.implied_arity.is_none());
                }
                assert_eq!(context.flags.bits(), InferenceFlags::NO_DEFAULT.bits());
                assert_eq!(context.compare_types, CompareTypesFn::Assignable);
                assert!(context.signature.is_none());
                assert!(context.return_mapper.is_none());
                assert!(context.inferred_type_parameters.is_none());
                assert!(context.intra_expression_inference_sites.is_none());
                assert!(context.outer_return_mapper.is_none());
                let mapper = context.mapper;
                let non_fixing = context.non_fixing_mapper;
                // 68254-68255: the pair is Deferred over THIS context,
                // fixing first.
                match state.mapper(mapper) {
                    TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                        assert_eq!(*id, ctx)
                    }
                    other => panic!("fixing mapper shape: {other:?}"),
                }
                match state.mapper(non_fixing) {
                    TypeMapper::Deferred(DeferredMapperTargets::InferenceNonFixing(id)) => {
                        assert_eq!(*id, ctx)
                    }
                    other => panic!("non-fixing mapper shape: {other:?}"),
                }
            },
        );
    }

    #[test]
    fn clone_inference_context_deep_copies_infos_only() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let string = state.tables.intrinsics.string;
                let number = state.tables.intrinsics.number;
                let ctx =
                    state.create_inference_context(&[t, u], None, InferenceFlags::NO_DEFAULT, None);
                let var_decl = node_of_kind(state, tsrs2_syntax::SyntaxKind::VariableDeclaration);
                state.inference_context_mut(ctx).inferences[0].candidates = Some(vec![string]);
                state.inference_context_mut(ctx).return_mapper =
                    Some(state.make_unary_type_mapper(t, string));
                state.add_intra_expression_inference_site(ctx, var_decl, string);
                // None passes through (68242 `context && ...`).
                assert!(state
                    .clone_inference_context(None, InferenceFlags::NONE)
                    .is_none());
                let clone = state
                    .clone_inference_context(Some(ctx), InferenceFlags::SKIPPED_GENERIC_FUNCTION)
                    .expect("Some in, Some out");
                let cloned = state.inference_context(clone);
                // extraFlags OR onto the original's flags (68242).
                assert_eq!(
                    cloned.flags.bits(),
                    (InferenceFlags::NO_DEFAULT | InferenceFlags::SKIPPED_GENERIC_FUNCTION).bits()
                );
                assert_eq!(cloned.inferences[0].candidates, Some(vec![string]));
                // cloneInferenceContext clones the INFOS; lazily-
                // attached context fields do not survive.
                assert!(cloned.return_mapper.is_none());
                assert!(cloned.intra_expression_inference_sites.is_none());
                assert!(cloned.outer_return_mapper.is_none());
                // Fresh mapper pair over the CLONE.
                let clone_mapper = cloned.mapper;
                match state.mapper(clone_mapper) {
                    TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                        assert_eq!(*id, clone)
                    }
                    other => panic!("clone mapper shape: {other:?}"),
                }
                // cloneInferenceInfo slices the candidate arrays: a
                // later push into the original is invisible to the
                // clone (68315 `.slice()`).
                state.inference_context_mut(ctx).inferences[0]
                    .candidates
                    .as_mut()
                    .expect("candidates present")
                    .push(number);
                assert_eq!(
                    state.inference_context(clone).inferences[0]
                        .candidates
                        .as_ref()
                        .expect("cloned candidates")
                        .len(),
                    1
                );
            },
        );
    }

    #[test]
    fn clone_inferred_part_filters_to_candidate_rows() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let string = state.tables.intrinsics.string;
                let ctx = state.create_inference_context(&[t, u], None, InferenceFlags::NONE, None);
                // No candidates anywhere → undefined (68326).
                assert!(state.clone_inferred_part_of_context(ctx).is_none());
                state.inference_context_mut(ctx).inferences[1].contra_candidates =
                    Some(vec![string]);
                let part = state
                    .clone_inferred_part_of_context(ctx)
                    .expect("one candidate row");
                let cloned = state.inference_context(part);
                assert_eq!(cloned.inferences.len(), 1);
                assert_eq!(cloned.inferences[0].type_parameter, u);
                assert_eq!(cloned.inferences[0].contra_candidates, Some(vec![string]));
            },
        );
    }

    #[test]
    fn deferred_dispatch_identity_and_stub_frontier() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                let non_fixing = state.inference_context(ctx).non_fixing_mapper;
                // 63348: non-member types map to themselves.
                let mapped = state
                    .get_mapped_type(string, non_fixing)
                    .expect("identity on non-member");
                assert_eq!(mapped, string);
                // A member dispatches into the 7.3 resolution stub.
                let err = state.get_mapped_type(t, non_fixing).expect_err("7.3 stub");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                // The non-fixing thunk never fixes (68274-68275).
                assert!(!state.inference_context(ctx).inferences[0].is_fixed);
            },
        );
    }

    #[test]
    fn fixing_dispatch_clears_caches_and_fixes_before_resolution() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let string = state.tables.intrinsics.string;
                let ctx = state.create_inference_context(&[t, u], None, InferenceFlags::NONE, None);
                state.inference_context_mut(ctx).inferences[0].inferred_type = Some(string);
                state.inference_context_mut(ctx).inferences[1].inferred_type = Some(string);
                let fixing = state.inference_context(ctx).mapper;
                let err = state.get_mapped_type(t, fixing).expect_err("7.3 stub");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                let context = state.inference_context(ctx);
                // 68263-68265 order: clearCachedInferences runs while
                // the row is still unfixed (its own stale cache
                // drops), THEN isFixed is set, THEN resolution.
                assert!(context.inferences[0].is_fixed);
                assert!(context.inferences[0].inferred_type.is_none());
                // Other unfixed rows lose their cache too.
                assert!(!context.inferences[1].is_fixed);
                assert!(context.inferences[1].inferred_type.is_none());
                // A second dispatch on the SAME (now fixed) row skips
                // the drain/clear preamble entirely (68262 guard).
                state.inference_context_mut(ctx).inferences[1].inferred_type = Some(string);
                let err = state
                    .get_mapped_type(t, fixing)
                    .expect_err("7.3 stub again");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                assert_eq!(
                    state.inference_context(ctx).inferences[1].inferred_type,
                    Some(string),
                    "fixed-row dispatch must not re-clear other caches"
                );
            },
        );
    }

    #[test]
    fn fixing_dispatch_drains_sites_with_no_contextual_type() {
        with_program_state(
            &[("a.ts", "function f<T>() { var w = 1; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let literal = node_of_kind(state, tsrs2_syntax::SyntaxKind::NumericLiteral);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                // Lazy array creation (68287 `??=`).
                state.add_intra_expression_inference_site(ctx, literal, string);
                state.add_intra_expression_inference_site(ctx, literal, string);
                assert_eq!(
                    state
                        .inference_context(ctx)
                        .intra_expression_inference_sites
                        .as_ref()
                        .expect("lazily created")
                        .len(),
                    2
                );
                let fixing = state.inference_context(ctx).mapper;
                // `var w = 1` has no contextual type at the
                // initializer, so the drain loop completes without
                // touching inferTypes and clears the list (68297),
                // then resolution hits the 7.3 stub.
                let err = state.get_mapped_type(t, fixing).expect_err("7.3 stub");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                let context = state.inference_context(ctx);
                assert!(context.intra_expression_inference_sites.is_none());
                assert!(context.inferences[0].is_fixed);
            },
        );
    }

    #[test]
    fn fixing_dispatch_mid_drain_unwind_keeps_sites_and_fix_pending() {
        with_program_state(
            &[("a.ts", "function f<T>() { var v: number = 1; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let literal = node_of_kind(state, tsrs2_syntax::SyntaxKind::NumericLiteral);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                state.add_intra_expression_inference_site(ctx, literal, string);
                let fixing = state.inference_context(ctx).mapper;
                // The annotated initializer HAS a contextual type, so
                // the drain reaches the 7.2 inferTypes stub and
                // unwinds BEFORE the 68297 clear and the 68265 fix.
                let err = state.get_mapped_type(t, fixing).expect_err("7.2 stub");
                assert!(err.reason.contains("inferTypes"), "{}", err.reason);
                let context = state.inference_context(ctx);
                assert!(context.intra_expression_inference_sites.is_some());
                assert!(!context.inferences[0].is_fixed);
            },
        );
    }

    #[test]
    fn check_expression_with_contextual_type_clears_undrained_sites() {
        with_program_state(
            &[("a.ts", "function f<T>() { var w = 1; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let number = state.tables.intrinsics.number;
                let literal = node_of_kind(state, tsrs2_syntax::SyntaxKind::NumericLiteral);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                state.add_intra_expression_inference_site(ctx, literal, string);
                // 80566-80569: the sites are DISCARDED (not drained)
                // once the full expression has been checked.
                state
                    .check_expression_with_contextual_type(
                        literal,
                        number,
                        Some(ctx),
                        tsrs2_types::CheckMode::NORMAL,
                    )
                    .expect("literal checks");
                assert!(state
                    .inference_context(ctx)
                    .intra_expression_inference_sites
                    .is_none());
                assert!(
                    !state.inference_context(ctx).inferences[0].is_fixed,
                    "clear is not a drain — nothing fixed"
                );
            },
        );
    }

    #[test]
    fn inferential_annotated_arity_arm_unwinds_named_unsupported() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>() { var target: (a: number, b: string) => void; var g = (x: number) => 1; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let annotation = annotation_of_var(state, "target");
                let contextual = state.get_type_from_type_node(annotation).expect("fn type");
                let arrow = node_of_kind(state, tsrs2_syntax::SyntaxKind::ArrowFunction);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                // 79179-79182: non-context-sensitive, no own type
                // parameters, contextual arity 2 > own arity 1 — under
                // the 7.1-producible Inferential bit the arm is a
                // named 7.4 escape, not a silent no-op.
                //
                // The sibling context-sensitive arm shares the same
                // reason string, so pin the arm selection too: a fully
                // annotated arrow is NOT context-sensitive, making the
                // 79166 arm (and its identical Unsupported) unreachable
                // — the Err below can only come from the 79178 arm.
                assert!(
                    !state.is_context_sensitive(arrow),
                    "fully annotated arrow must take the 79178 arity arm"
                );
                let err = state
                    .check_expression_with_contextual_type(
                        arrow,
                        contextual,
                        Some(ctx),
                        tsrs2_types::CheckMode::NORMAL,
                    )
                    .expect_err("7.4 escape");
                assert!(
                    err.reason.contains("inferFromAnnotatedParametersAndReturn"),
                    "{}",
                    err.reason
                );
            },
        );
    }

    #[test]
    fn outer_return_mapper_merges_and_caches() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                // returnMapper None: mergeTypeMappers(undefined, m2)
                // = m2 — the clone's fixing mapper alone.
                let outer = state.create_outer_return_mapper(ctx);
                match state.mapper(outer) {
                    TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(clone_id)) => {
                        assert_ne!(*clone_id, ctx, "mapper belongs to the CLONE")
                    }
                    other => panic!("outer mapper shape: {other:?}"),
                }
                // 63386 `??=`: the second call is a cache hit — same
                // mapper, no new context cloned.
                let arena_len = state.inference_context_arena.len();
                let again = state.create_outer_return_mapper(ctx);
                assert_eq!(again, outer);
                assert_eq!(state.inference_context_arena.len(), arena_len);
                // With a returnMapper present the pair merges.
                let ctx2 = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                let ret = state.make_unary_type_mapper(t, string);
                state.inference_context_mut(ctx2).return_mapper = Some(ret);
                let merged = state.create_outer_return_mapper(ctx2);
                match state.mapper(merged) {
                    TypeMapper::Merged { mapper1, mapper2 } => {
                        assert_eq!(*mapper1, ret);
                        match state.mapper(*mapper2) {
                            TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                                assert_ne!(*id, ctx2)
                            }
                            other => panic!("merged rhs shape: {other:?}"),
                        }
                    }
                    other => panic!("merged mapper shape: {other:?}"),
                }
            },
        );
    }

    #[test]
    fn get_mapper_from_context_reads_fixing_mapper() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                assert!(state.get_mapper_from_context(None).is_none());
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                assert_eq!(
                    state.get_mapper_from_context(Some(ctx)),
                    Some(state.inference_context(ctx).mapper)
                );
            },
        );
    }

    #[test]
    fn instantiate_contextual_type_return_mapper_branch() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let any = state.tables.intrinsics.any;
                let false_regular = state.tables.intrinsics.false_regular;
                let true_regular = state.tables.intrinsics.true_regular;
                let var_decl = node_of_kind(state, tsrs2_syntax::SyntaxKind::VariableDeclaration);
                // returnMapper maps T to a union carrying BOTH regular
                // boolean literals — 73453-73454 filters them out.
                let union = state
                    .get_union_type_ex(&[false_regular, true_regular, string], UnionReduction::None)
                    .expect("union");
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                let ret = state.make_unary_type_mapper(t, union);
                state.inference_context_mut(ctx).return_mapper = Some(ret);
                state.push_inference_context(var_decl, Some(ctx));
                let out = state
                    .instantiate_contextual_type(Some(t), var_decl, ContextFlags::NONE)
                    .expect("instantiates");
                assert_eq!(out, Some(string));
                // An AnyOrUnknown instantiation falls through to the
                // identity read (73447 guard).
                let ctx_any =
                    state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                let ret_any = state.make_unary_type_mapper(t, any);
                state.inference_context_mut(ctx_any).return_mapper = Some(ret_any);
                state.pop_inference_context();
                state.push_inference_context(var_decl, Some(ctx_any));
                let out = state
                    .instantiate_contextual_type(Some(t), var_decl, ContextFlags::NONE)
                    .expect("falls through");
                assert_eq!(out, Some(t));
                state.pop_inference_context();
            },
        );
    }

    #[test]
    fn instantiate_contextual_type_signature_branch_consults_non_fixing_mapper() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let var_decl = node_of_kind(state, tsrs2_syntax::SyntaxKind::VariableDeclaration);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                state.inference_context_mut(ctx).inferences[0].candidates = Some(vec![string]);
                state.push_inference_context(var_decl, Some(ctx));
                // 73444-73445: Signature flags + a candidate-bearing
                // row instantiate through the NON-fixing mapper,
                // whose resolution is the 7.3 stub today.
                let err = state
                    .instantiate_contextual_type(Some(t), var_decl, ContextFlags::SIGNATURE)
                    .expect_err("reaches the 7.3 stub through the non-fixing mapper");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                assert!(
                    !state.inference_context(ctx).inferences[0].is_fixed,
                    "the Signature branch must NOT fix"
                );
                state.pop_inference_context();
            },
        );
    }

    #[test]
    fn context_arena_survives_rollback_while_stack_truncates() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let var_decl = node_of_kind(state, tsrs2_syntax::SyntaxKind::VariableDeclaration);
                let checkpoint = state.begin_speculation();
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                state.push_inference_context(var_decl, Some(ctx));
                state.rollback_speculation(checkpoint);
                // The node stack is A-class (truncated to the mark);
                // the arena is E-class — the context object survives
                // exactly like tsc's GC object would (chooseOverload
                // 76842 depends on trial-surviving context state).
                assert!(state.inference_contexts.is_empty());
                assert_eq!(state.inference_context_arena.len(), 1);
                assert_eq!(state.inference_context(ctx).inferences.len(), 1);
            },
        );
    }
}
