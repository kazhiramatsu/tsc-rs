//! M6 7.1: the inference data model (m6-inference-calls-steps.md 7.1)
//! — InferenceInfo/InferenceContext (tsc 68238-68330) with the
//! fixing/non-fixing Deferred mapper pair and the
//! createOuterReturnMapper cache slot (63385).
//!
//! Contexts are arena-allocated on CheckerState so InferenceContextId
//! equality IS tsc's context object identity, exactly like the
//! `mappers` arena — and InferenceInfoId likewise gives the info
//! objects tsc identity (thunk captures, mergeInferences slot
//! replacement, detached arrays). The arena is E-class speculation state
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

use std::collections::HashMap;

use tsrs2_syntax::{escape_leading_underscores, NodeId, SyntaxKind};
use tsrs2_types::{
    ContextFlags, ExpandingFlags, InferenceFlags, InferencePriority, IntersectionFlags,
    LiteralValue, ObjectFlags, SymbolFlags, TypeData, TypeFlags, TypeId, UnionReduction,
    VarianceFlags,
};

use crate::instantiate::{DeferredMapperTargets, MapperId, TypeMapper};
use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, IndexInfo, SignatureId, Unsupported};
use crate::variance::VariancesResult;

/// Arena id — see the module doc for the identity/rollback contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InferenceContextId(pub u32);

/// Arena id for an InferenceInfo — tsc infos are GC objects whose
/// IDENTITY is load-bearing (thunk captures, mergeInferences slot
/// replacement, 7.4's detached higher-order arrays), so the port
/// stores them in `CheckerState::inference_info_arena` (E-class,
/// append-only) and passes ids everywhere tsc passes the object.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InferenceInfoId(pub u32);

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
/// Infos have OBJECT IDENTITY (`InferenceInfoId` into the E-class
/// info arena), exactly like tsc's GC objects: `inferences` holds the
/// LIVE slots (tsc `context.inferences` — mergeInferences 80836
/// rewrites these at 7.4), while `mapper_sources`/`mapper_infos` are
/// the CREATION-TIME capture shared by the fixing/non-fixing mapper
/// pair — tsc's makeDeferredTypeMapper sources array plus the
/// per-slot info objects the thunks close over (68258-68278; both
/// mappers are built from the same array inside
/// createInferenceContextWorker before the context escapes, so one
/// shared capture is exact). Post-merge, tsc's split state — the
/// detached thunk object keeps isFixed=true while the fresh live row
/// starts isFixed=false (hasInferenceCandidates 80822 never consults
/// isFixed) — falls out structurally: the thunk bit rides
/// `mapper_infos[i]`, the 68710/69266 live-row reads ride
/// `inferences[i]`, and the two coincide exactly until a merge
/// replaces the slot id.
#[derive(Clone, Debug)]
pub(crate) struct InferenceContext {
    pub(crate) inferences: Vec<InferenceInfoId>,
    /// Creation-time makeDeferredTypeMapper capture (see above):
    /// `map(context.inferences, i => i.typeParameter)` ...
    pub(crate) mapper_sources: Vec<TypeId>,
    /// ... and the thunk-captured info objects, one per slot.
    pub(crate) mapper_infos: Vec<InferenceInfoId>,
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
/// A free function over an id list (plus the info arena) because
/// 7.2's inferTypes call sites also run it on detached arrays (the
/// higher-order path's local `inferences`, 80786), not only on
/// context-attached ones — a detached tsc array is a `Vec<
/// InferenceInfoId>` here, sharing the same objects.
pub(crate) fn clear_cached_inferences(arena: &mut [InferenceInfo], infos: &[InferenceInfoId]) {
    for &id in infos {
        let inference = &mut arena[id.0 as usize];
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

    /// tsrs-native: arena accessor (infos are GC objects in tsc).
    pub(crate) fn inference_info(&self, id: InferenceInfoId) -> &InferenceInfo {
        &self.inference_info_arena[id.0 as usize]
    }

    /// tsrs-native: arena accessor (infos are GC objects in tsc).
    pub(crate) fn inference_info_mut(&mut self, id: InferenceInfoId) -> &mut InferenceInfo {
        &mut self.inference_info_arena[id.0 as usize]
    }

    /// tsrs-native: arena allocation — tsc object creation.
    pub(crate) fn alloc_inference_info(&mut self, info: InferenceInfo) -> InferenceInfoId {
        let id = InferenceInfoId(self.inference_info_arena.len() as u32);
        self.inference_info_arena.push(info);
        id
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
            .map(|&tp| self.alloc_inference_info(create_inference_info(tp)))
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
            let slots = ctx.inferences.clone();
            let signature = ctx.signature;
            let flags = ctx.flags | extra_flags;
            let compare_types = ctx.compare_types;
            let inferences = slots
                .iter()
                .map(|&slot| {
                    let cloned = clone_inference_info(self.inference_info(slot));
                    self.alloc_inference_info(cloned)
                })
                .collect();
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
        inferences: Vec<InferenceInfoId>,
        signature: Option<SignatureId>,
        flags: InferenceFlags,
        compare_types: CompareTypesFn,
    ) -> InferenceContextId {
        let id = InferenceContextId(self.inference_context_arena.len() as u32);
        // 68254-68255: both mappers capture the SAME inferences array
        // at creation — sources = map(inferences, i.typeParameter),
        // thunks close over the per-slot info objects.
        let mapper_sources = inferences
            .iter()
            .map(|&info| self.inference_info(info).type_parameter)
            .collect();
        let mapper_infos = inferences.clone();
        let mapper = self.alloc_mapper(TypeMapper::Deferred(
            DeferredMapperTargets::InferenceFixing(id),
        ));
        let non_fixing_mapper = self.alloc_mapper(TypeMapper::Deferred(
            DeferredMapperTargets::InferenceNonFixing(id),
        ));
        self.inference_context_arena.push(InferenceContext {
            inferences,
            mapper_sources,
            mapper_infos,
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
        let slots = ctx.inferences.clone();
        let signature = ctx.signature;
        let flags = ctx.flags;
        let compare_types = ctx.compare_types;
        let candidate_slots: Vec<InferenceInfoId> = slots
            .iter()
            .copied()
            .filter(|&slot| has_inference_candidates(self.inference_info(slot)))
            .collect();
        let inferences: Vec<InferenceInfoId> = candidate_slots
            .iter()
            .map(|&slot| {
                let cloned = clone_inference_info(self.inference_info(slot));
                self.alloc_inference_info(cloned)
            })
            .collect();
        if inferences.is_empty() {
            return None;
        }
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
    pub(crate) fn has_inference_candidates_or_default(&self, info: InferenceInfoId) -> bool {
        let info = self.inference_info(info);
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
                    // 68296 `inferTypes(context.inferences, type,
                    // contextualType)` — the live slots, re-read per
                    // call exactly as tsc re-evaluates the member
                    // expression.
                    let inferences = self.inference_context(context).inferences.clone();
                    self.infer_types(
                        &inferences,
                        site.ty,
                        contextual_type,
                        InferencePriority::NONE,
                        false,
                    )?;
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
    /// dispatches here when `ty` matched the mapper's creation-time
    /// sources[index]. Order is load-bearing — drain the
    /// intra-expression sites and clear cached inferences BEFORE
    /// setting is_fixed (the row being fixed is still unfixed at
    /// clear time, so its own stale inferred_type is dropped too),
    /// then resolve.
    ///
    /// The `is_fixed` test-and-set rides the thunk-CAPTURED info
    /// (`mapper_infos[index]`, tsc's closure over the creation-time
    /// object), while clearCachedInferences and getInferredType read
    /// the LIVE slots — identical until mergeInferences (7.4)
    /// replaces a slot id, and tsc-exact after.
    pub(crate) fn fixing_mapper_target(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> CheckResult2<TypeId> {
        let captured = self.inference_context(context).mapper_infos[index];
        if !self.inference_info(captured).is_fixed {
            self.infer_from_intra_expression_sites(context)?;
            // 68264: clearCachedInferences(context.inferences) — the
            // LIVE slots, not the capture.
            clear_cached_inferences(
                &mut self.inference_info_arena,
                &self.inference_context_arena[context.0 as usize].inferences,
            );
            self.inference_info_mut(captured).is_fixed = true;
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

    /// tsc-port: isTypeOrBaseIdenticalTo @6.0.3
    /// tsc-hash: 919de3d454f063e3817c9b6dcbb5b996714dfee56928f1a59f42ba1277836df9
    /// tsc-span: _tsc.js:69234-69236
    pub(crate) fn is_type_or_base_identical_to(
        &mut self,
        s: TypeId,
        t: TypeId,
    ) -> CheckResult2<bool> {
        if t == self.tables.intrinsics.missing {
            return Ok(s == t);
        }
        Ok(self.is_type_identical_to(s, t)?
            || (self.tables.flags_of(t).intersects(TypeFlags::STRING)
                && self
                    .tables
                    .flags_of(s)
                    .intersects(TypeFlags::STRING_LITERAL))
            || (self.tables.flags_of(t).intersects(TypeFlags::NUMBER)
                && self
                    .tables
                    .flags_of(s)
                    .intersects(TypeFlags::NUMBER_LITERAL)))
    }

    /// tsc-port: isTypeCloselyMatchedBy @6.0.3
    /// tsc-hash: 9e6c16ca3142c2941d70635429d3c6c52cb8c0c4f851646ff4947b18e54109e5
    /// tsc-span: _tsc.js:69237-69239
    pub(crate) fn is_type_closely_matched_by(&self, s: TypeId, t: TypeId) -> bool {
        let s_ty = self.tables.type_of(s);
        let t_ty = self.tables.type_of(t);
        (s_ty.flags.intersects(TypeFlags::OBJECT)
            && t_ty.flags.intersects(TypeFlags::OBJECT)
            && s_ty.symbol.is_some()
            && s_ty.symbol == t_ty.symbol)
            || (s_ty.alias_symbol.is_some()
                && s_ty.alias_type_arguments.is_some()
                && s_ty.alias_symbol == t_ty.alias_symbol)
    }

    /// tsc-port: isTypeParameterAtTopLevel @6.0.3
    /// tsc-hash: 8fc9224bccca52f75df1302daf69a97ddcc67b5b7d4b5f132424c29be7b9a8d6
    /// tsc-span: _tsc.js:68349-68351
    ///
    /// The depth<3 conditional-type probe is an M8 escape (the flag is
    /// unconstructible until conditional types land).
    pub(crate) fn is_type_parameter_at_top_level(
        &mut self,
        ty: TypeId,
        tp: TypeId,
        depth: usize,
    ) -> CheckResult2<bool> {
        if ty == tp {
            return Ok(true);
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let members = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("UnionOrIntersection flag implies member data"),
            };
            for member in members {
                if self.is_type_parameter_at_top_level(member, tp, depth)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if depth < 3 && flags.intersects(TypeFlags::CONDITIONAL) {
            return Err(Unsupported::new(
                "isTypeParameterAtTopLevel conditional branches (M8 — the flag is \
                 unconstructible before conditional types land)",
            ));
        }
        Ok(false)
    }

    /// tsc-port: createEmptyObjectTypeFromStringLiteral @6.0.3
    /// tsc-hash: db0404479692e816441b1bbbe284f68806d6e11a3a0bf7977006a948be35372d
    /// tsc-span: _tsc.js:68356-68385
    ///
    /// The literal-keyof arm's reverse shape: every StringLiteral
    /// member of the (forEachType-distributed union) source becomes an
    /// any-typed transient property — declarations copied from the
    /// literal's symbol — and a plain-string source contributes a
    /// string→emptyObjectType index signature instead. Map-overwrite
    /// semantics ride IndexMap::insert (same-position replace), and
    /// setStructuredTypeMembers' getNamedMembers projection is the
    /// full table: escaped literal names can never take the reserved
    /// exactly-two-underscore shape.
    pub(crate) fn create_empty_object_type_from_string_literal(&mut self, ty: TypeId) -> TypeId {
        let id = self.create_resolved_empty_anonymous_type(None);
        let members_id = self
            .links
            .ty(id)
            .resolved_members
            .resolved()
            .expect("created resolved above");
        // forEachType (61513): Union distributes, everything else runs
        // the callback once.
        let source_members = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("Union flag implies member data"),
            }
        } else {
            vec![ty]
        };
        for t in source_members {
            if !self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::STRING_LITERAL)
            {
                continue;
            }
            let TypeData::Literal {
                value: LiteralValue::String(value),
            } = &self.tables.type_of(t).data
            else {
                unreachable!("StringLiteral flag implies string data");
            };
            let name = escape_leading_underscores(value);
            let literal_prop = self
                .binder
                .create_symbol(SymbolFlags::PROPERTY, name.clone());
            self.links.set_symbol_type(
                self.speculation_depth,
                literal_prop,
                LinkSlot::Resolved(self.tables.intrinsics.any),
            );
            if let Some(symbol) = self.tables.type_of(t).symbol {
                let declarations = self.binder.symbol(symbol).declarations.clone();
                let value_declaration = self.binder.symbol(symbol).value_declaration;
                let prop = self.binder.symbol_mut(literal_prop);
                prop.declarations = declarations;
                prop.value_declaration = value_declaration;
            }
            self.members_mut(members_id)
                .members
                .insert(name, literal_prop);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::STRING) {
            let index_info = IndexInfo {
                key_type: self.tables.intrinsics.string,
                value_type: self.empty_object_type,
                is_readonly: false,
                declaration: None,
                components: None,
                is_enum_number_index_info: false,
            };
            self.members_mut(members_id).index_infos.push(index_info);
        }
        let properties: Vec<_> = self
            .members_of(members_id)
            .members
            .values()
            .copied()
            .collect();
        self.members_mut(members_id).properties = properties;
        id
    }

    /// tsc-port: hasSkipDirectInferenceFlag @6.0.3
    /// tsc-hash: acf0e7bd86bab58da75c3a803292e066114e5df6b23cfa64ebff9bacb7805004
    /// tsc-span: _tsc.js:68509-68511
    ///
    /// Constant false: the only writer of links.skipDirectInference is
    /// runWithInferenceBlockedFromSourceNode (46950-46977), a
    /// services-only entry (completions' getResolvedSignature probe)
    /// the conformance driver never reaches — same disposition as the
    /// blockedStringType read in expr.rs's string-literal arm.
    #[allow(dead_code)] // consumer: 7.2d inferFromProperties (69174)
    pub(crate) fn has_skip_direct_inference_flag(&self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// tsc-port: isFromInferenceBlockedSource @6.0.3
    /// tsc-hash: 145bbe111d3b19425b1d192487d5a5a9f00f93a7f9e35dc3356a73cede4efb96
    /// tsc-span: _tsc.js:68512-68514
    ///
    /// Constant false for the same reason as
    /// `has_skip_direct_inference_flag` above: no declaration can
    /// carry the flag while its only writer is services-only.
    pub(crate) fn is_from_inference_blocked_source(&self, ty: TypeId) -> bool {
        let _ = ty;
        false
    }

    /// tsc-port: inferTypes @6.0.3
    /// tsc-hash: 87c1353bf4aba29de6b61ebe8198ffb59d14c6c05594bf44535b642745b062cc
    /// tsc-span: _tsc.js:68637-68645
    ///
    /// The candidate collector's entry: tsc's inferences-array-first
    /// signature with the `priority = 0, contravariant = false`
    /// defaults spelled out at every call site. Context-attached
    /// callers clone the context's (Copy) id vec; detached arrays
    /// (inferReverseMappedTypeWorker 68438, the 7.4 higher-order path
    /// 80788) pass their own Vec — never a throwaway arena context.
    /// The closure family (68646-69233) lives on `InferTypesWalker`.
    pub(crate) fn infer_types(
        &mut self,
        inferences: &[InferenceInfoId],
        original_source: TypeId,
        original_target: TypeId,
        priority: InferencePriority,
        contravariant: bool,
    ) -> CheckResult2<()> {
        let mut walker = InferTypesWalker {
            st: self,
            inferences: inferences.to_vec(),
            original_target,
            priority,
            contravariant,
            bivariant: false,
            propagation_type: None,
            inference_priority: InferencePriority::MAX_VALUE,
            visited: HashMap::new(),
            source_stack: Vec::new(),
            target_stack: Vec::new(),
            expanding_flags: ExpandingFlags::NONE,
        };
        walker.infer_from_types(original_source, original_target)
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

/// The inferFromMatchingTypes `matches` parameter (68859): tsc passes
/// one of three predicate references — isTypeOrBaseIdenticalTo /
/// isTypeCloselyMatchedBy / isTypeIdenticalTo; the port dispatches
/// over the closed set (CompareTypesFn precedent). Union pass 1/2 =
/// 68673-68674, intersection pass = 68688.
#[derive(Clone, Copy, Debug)]
enum TypeMatcher {
    OrBaseIdenticalTo,
    CloselyMatchedBy,
    IdenticalTo,
}

/// The inferTypes closure family (68646-69233): one walker per
/// inferTypes invocation, carrying tsc's captured locals (68638-68644)
/// plus the two entry parameters the closures mutate via save/restore
/// (`priority`, `contravariant`). Everything here is walker-local and
/// dies with an Err unwind — the RelationChecker discipline
/// (engine.rs) — so none of it joins the UnwindSnapshot census; the
/// only durable writes go through the E-class info arena.
struct InferTypesWalker<'r, 'a> {
    st: &'r mut CheckerState<'a>,
    /// The `inferences` argument array as an entry-time id snapshot:
    /// slot identity can only change via mergeInferences (80836),
    /// which runs between inferTypes invocations, never inside one.
    inferences: Vec<InferenceInfoId>,
    original_target: TypeId,
    priority: InferencePriority,
    contravariant: bool,
    bivariant: bool,
    propagation_type: Option<TypeId>,
    /// 68640: min-tracked priority of every inference actually landed;
    /// MaxValue until the first candidate records.
    inference_priority: InferencePriority,
    /// 68641: lazily created in tsc; HashMap::new() allocates nothing
    /// until the first insert, so a plain map is the same. Keyed by
    /// the invokeOnce `source.id + "," + target.id` pair.
    visited: HashMap<(TypeId, TypeId), InferencePriority>,
    source_stack: Vec<TypeId>,
    target_stack: Vec<TypeId>,
    expanding_flags: ExpandingFlags,
}

/// The invokeOnce `action` parameter (68833): tsc passes one of three
/// closure references — inferToConditionalType /
/// inferFromGenericMappedTypes / inferFromObjectTypes; the port
/// dispatches over the closed set (the TypeMatcher precedent). The
/// mapped/object actions arrive at 7.2d.
#[derive(Clone, Copy, Debug)]
enum InferAction {
    ToConditionalType,
}

impl InferTypesWalker<'_, '_> {
    /// tsrs-native: member access for UnionOrIntersection types (tsc
    /// `type.types`), the engine.rs union_members shape.
    fn types_of(&self, ty: TypeId) -> Vec<TypeId> {
        match &self.st.tables.type_of(ty).data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("UnionOrIntersection flag implies member data"),
        }
    }

    /// tsrs-native: clearCachedInferences over the walker's array —
    /// the free-fn split lets the arena borrow stay disjoint from the
    /// id list.
    fn clear_cached(&mut self) {
        clear_cached_inferences(&mut self.st.inference_info_arena, &self.inferences);
    }

    /// tsrs-native: TypeMatcher dispatch (see the enum).
    fn matches_pair(&mut self, s: TypeId, t: TypeId, matcher: TypeMatcher) -> CheckResult2<bool> {
        match matcher {
            TypeMatcher::OrBaseIdenticalTo => self.st.is_type_or_base_identical_to(s, t),
            TypeMatcher::CloselyMatchedBy => Ok(self.st.is_type_closely_matched_by(s, t)),
            TypeMatcher::IdenticalTo => self.st.is_type_identical_to(s, t),
        }
    }

    /// tsc-port: inferFromTypes @6.0.3
    /// tsc-hash: 48b6e375e3eb768298b7554c55ba6f7b45710573f07bb56e9ff78ff819b48328
    /// tsc-span: _tsc.js:68646-68814
    ///
    /// The dispatch spine, arms in source order. 7.2a stages the tail:
    /// the literal-keyof arm is 7.2b, inferToConditionalType rides
    /// invokeOnce at 7.2b, inferToTemplateLiteralType is 7.2c, and the
    /// reduced/apparent object tail (inferFromObjectTypes) is 7.2d —
    /// each a named escape below until its commit. The Substitution
    /// source arm is a genuine M8 escape (unconstructible flag).
    ///
    /// Load-bearing shape notes:
    /// - The TypeVariable block (68701-68769) returns ONLY when an
    ///   inference slot matched; the simplification fallback falls
    ///   through into the 68770 chain (e.g. an indexed-access pair
    ///   reaches the pairwise arm after failing to simplify).
    /// - Arms 5/6 (union/intersection reduction) rewrite source and
    ///   target in place before every later arm reads them.
    fn infer_from_types(&mut self, source: TypeId, target: TypeId) -> CheckResult2<()> {
        let mut source = source;
        let mut target = target;
        // 68647: `|| isNoInferType(target)` is constant-false — NoInfer
        // Substitution types are unconstructible until M8 (the
        // getIndexType precedent, indexed.rs).
        if !self.st.could_contain_type_variables(target) {
            return Ok(());
        }
        if source == self.st.tables.intrinsics.wildcard
            || source == self.st.tables.intrinsics.blocked_string
        {
            // 68650-68655: infer target-to-target under the
            // propagation type so nested type variables receive the
            // original marker source.
            let save_propagation_type = self.propagation_type;
            self.propagation_type = Some(source);
            let result = self.infer_from_types(target, target);
            self.propagation_type = save_propagation_type;
            return result;
        }
        if let Some(alias) = self.st.tables.type_of(source).alias_symbol {
            if Some(alias) == self.st.tables.type_of(target).alias_symbol {
                if let Some(source_args) =
                    self.st.tables.type_of(source).alias_type_arguments.clone()
                {
                    // 68658-68663: infer between the (filled) alias
                    // argument lists under the alias' measured
                    // variances.
                    let target_args = self.st.tables.type_of(target).alias_type_arguments.clone();
                    let params = self.st.links.symbol(alias).type_parameters.clone();
                    let min_params = self.st.get_min_type_argument_count(params.as_deref());
                    let in_js = self
                        .st
                        .binder
                        .symbol(alias)
                        .value_declaration
                        .is_some_and(|declaration| self.st.is_in_js_file(declaration));
                    let source_types = self
                        .st
                        .fill_missing_type_arguments(
                            Some(&source_args),
                            params.as_deref(),
                            min_params,
                            in_js,
                        )?
                        .expect("present arguments fill to a present list (68660)");
                    let target_types = self
                        .st
                        .fill_missing_type_arguments(
                            target_args.as_deref(),
                            params.as_deref(),
                            min_params,
                            in_js,
                        )?
                        .expect("shared alias symbol implies argument lists (68661)");
                    let variances = match self.st.get_alias_variances(alias)? {
                        VariancesResult::Known(variances) => variances,
                        // In-measurement recursion: tsc reads the
                        // links.variances = emptyArray placeholder.
                        VariancesResult::InProgress => Box::default(),
                    };
                    self.infer_from_type_arguments(&source_types, &target_types, &variances)?;
                }
                return Ok(());
            }
        }
        if source == target
            && self
                .st
                .tables
                .flags_of(source)
                .intersects(TypeFlags::UNION_OR_INTERSECTION)
        {
            // 68667-68671: identical union/intersection — infer each
            // member into itself.
            for t in self.types_of(source) {
                self.infer_from_types(t, t)?;
            }
            return Ok(());
        }
        if self.st.tables.flags_of(target).intersects(TypeFlags::UNION) {
            // 68673-68684: strip identical then closely-matched pairs;
            // a fully-stripped source infers the remainder as a naked
            // type variable.
            let initial_sources = if self.st.tables.flags_of(source).intersects(TypeFlags::UNION) {
                self.types_of(source)
            } else {
                vec![source]
            };
            let (temp_sources, temp_targets) = self.infer_from_matching_types(
                initial_sources,
                self.types_of(target),
                TypeMatcher::OrBaseIdenticalTo,
            )?;
            let (sources, targets) = self.infer_from_matching_types(
                temp_sources,
                temp_targets,
                TypeMatcher::CloselyMatchedBy,
            )?;
            if targets.is_empty() {
                return Ok(());
            }
            target = self
                .st
                .get_union_type_ex(&targets, UnionReduction::Literal)?;
            if sources.is_empty() {
                self.infer_with_priority(source, target, InferencePriority::NAKED_TYPE_VARIABLE)?;
                return Ok(());
            }
            source = self
                .st
                .get_union_type_ex(&sources, UnionReduction::Literal)?;
        } else if self
            .st
            .tables
            .flags_of(target)
            .intersects(TypeFlags::INTERSECTION)
            && {
                let mut every_non_generic_object = true;
                for member in self.types_of(target) {
                    // 62918 isNonGenericObjectType
                    let non_generic_object = self
                        .st
                        .tables
                        .flags_of(member)
                        .intersects(TypeFlags::OBJECT)
                        && !self.st.is_generic_mapped_type_state(member);
                    if !non_generic_object {
                        every_non_generic_object = false;
                        break;
                    }
                }
                !every_non_generic_object
            }
        {
            // 68685-68694: reduce non-union sources against a partly
            // generic intersection target to the identical parts.
            if !self.st.tables.flags_of(source).intersects(TypeFlags::UNION) {
                let initial_sources = if self
                    .st
                    .tables
                    .flags_of(source)
                    .intersects(TypeFlags::INTERSECTION)
                {
                    self.types_of(source)
                } else {
                    vec![source]
                };
                let (sources, targets) = self.infer_from_matching_types(
                    initial_sources,
                    self.types_of(target),
                    TypeMatcher::IdenticalTo,
                )?;
                if sources.is_empty() || targets.is_empty() {
                    return Ok(());
                }
                source = self
                    .st
                    .get_intersection_type(&sources, IntersectionFlags::NONE)?;
                target = self
                    .st
                    .get_intersection_type(&targets, IntersectionFlags::NONE)?;
            }
        }
        if self
            .st
            .tables
            .flags_of(target)
            .intersects(TypeFlags::INDEXED_ACCESS | TypeFlags::SUBSTITUTION)
        {
            // 68695-68699: the isNoInferType guard is constant-false
            // (M8, as at the entry gate).
            target = self.st.get_actual_type_variable(target)?;
        }
        if self
            .st
            .tables
            .flags_of(target)
            .intersects(TypeFlags::TYPE_VARIABLE)
        {
            if self.st.is_from_inference_blocked_source(source) {
                return Ok(());
            }
            if let Some(info_id) = self.get_inference_info_for_type(target) {
                if self
                    .st
                    .tables
                    .object_flags_of(source)
                    .intersects(ObjectFlags::NON_INFERRABLE_TYPE)
                    || source == self.st.tables.intrinsics.non_inferrable_any
                {
                    return Ok(());
                }
                if !self.st.inference_info(info_id).is_fixed {
                    let candidate = self.propagation_type.unwrap_or(source);
                    if candidate == self.st.tables.intrinsics.blocked_string {
                        return Ok(());
                    }
                    // 68715-68720: a LOWER priority resets the record.
                    let reset = match self.st.inference_info(info_id).priority {
                        None => true,
                        Some(existing) => self.priority < existing,
                    };
                    if reset {
                        let info = self.st.inference_info_mut(info_id);
                        info.candidates = None;
                        info.contra_candidates = None;
                        info.top_level = true;
                        info.priority = Some(self.priority);
                    }
                    // 68721-68731: equal priority accumulates (unique).
                    if Some(self.priority) == self.st.inference_info(info_id).priority {
                        if self.contravariant && !self.bivariant {
                            let already = self
                                .st
                                .inference_info(info_id)
                                .contra_candidates
                                .as_deref()
                                .is_some_and(|contra| contra.contains(&candidate));
                            if !already {
                                self.st
                                    .inference_info_mut(info_id)
                                    .contra_candidates
                                    .get_or_insert_with(Vec::new)
                                    .push(candidate);
                                self.clear_cached();
                            }
                        } else {
                            let already = self
                                .st
                                .inference_info(info_id)
                                .candidates
                                .as_deref()
                                .is_some_and(|candidates| candidates.contains(&candidate));
                            if !already {
                                self.st
                                    .inference_info_mut(info_id)
                                    .candidates
                                    .get_or_insert_with(Vec::new)
                                    .push(candidate);
                                self.clear_cached();
                            }
                        }
                    }
                    // 68732-68735: record-time top-level demotion
                    // against the ORIGINAL target (not a threaded
                    // flag — threading one diverges).
                    if !self.priority.intersects(InferencePriority::RETURN_TYPE)
                        && self
                            .st
                            .tables
                            .flags_of(target)
                            .intersects(TypeFlags::TYPE_PARAMETER)
                        && self.st.inference_info(info_id).top_level
                        && !self.st.is_type_parameter_at_top_level(
                            self.original_target,
                            target,
                            0,
                        )?
                    {
                        self.st.inference_info_mut(info_id).top_level = false;
                        self.clear_cached();
                    }
                }
                self.inference_priority = self.inference_priority.min(self.priority);
                return Ok(());
            }
            // 68740-68769: no slot — try simplifying; on failure fall
            // THROUGH to the 68770 chain with the original pair.
            let simplified = self.st.get_simplified_type(target, false)?;
            if simplified != target {
                self.infer_from_types(source, simplified)?;
            } else if self
                .st
                .tables
                .flags_of(target)
                .intersects(TypeFlags::INDEXED_ACCESS)
            {
                let TypeData::IndexedAccess {
                    object_type,
                    index_type,
                    ..
                } = self.st.tables.type_of(target).data
                else {
                    unreachable!("IndexedAccess flag implies data");
                };
                let index_type = self.st.get_simplified_type(index_type, false)?;
                if self
                    .st
                    .tables
                    .flags_of(index_type)
                    .intersects(TypeFlags::INSTANTIABLE)
                {
                    let object_type = self.st.get_simplified_type(object_type, false)?;
                    if let Some(simplified2) =
                        self.st
                            .distribute_index_over_object_type(object_type, index_type, false)?
                    {
                        if simplified2 != target {
                            self.infer_from_types(source, simplified2)?;
                        }
                    }
                }
            }
        }
        // 68770-68813: the terminal arm chain (exactly one fires).
        let source_object_flags = self.st.tables.object_flags_of(source);
        let target_object_flags = self.st.tables.object_flags_of(target);
        let source_flags = self.st.tables.flags_of(source);
        let target_flags = self.st.tables.flags_of(target);
        let matching_references = source_object_flags.intersects(ObjectFlags::REFERENCE)
            && target_object_flags.intersects(ObjectFlags::REFERENCE)
            && (self.st.tables.reference_target(source) == self.st.tables.reference_target(target)
                || self.st.is_array_type(source)? && self.st.is_array_type(target)?)
            && !(self.st.links.ty(source).deferred_node.is_some()
                && self.st.links.ty(target).deferred_node.is_some());
        if matching_references {
            // 68770-68771: matching references infer pairwise under the
            // target's measured variances.
            let source_arguments = self.st.get_type_arguments(source)?;
            let target_arguments = self.st.get_type_arguments(target)?;
            let reference_target = self.st.tables.reference_target(source);
            let variances = match self.st.get_variances(reference_target)? {
                VariancesResult::Known(variances) => variances,
                VariancesResult::InProgress => Box::default(),
            };
            self.infer_from_type_arguments(&source_arguments, &target_arguments, &variances)?;
        } else if source_flags.intersects(TypeFlags::INDEX)
            && target_flags.intersects(TypeFlags::INDEX)
        {
            // 68772-68773: keyof operands infer contravariantly.
            let TypeData::Index {
                ty: source_inner, ..
            } = self.st.tables.type_of(source).data
            else {
                unreachable!("Index flag implies data");
            };
            let TypeData::Index {
                ty: target_inner, ..
            } = self.st.tables.type_of(target).data
            else {
                unreachable!("Index flag implies data");
            };
            self.infer_from_contravariant_types(source_inner, target_inner)?;
        } else if (self.st.is_literal_type(source) || source_flags.intersects(TypeFlags::STRING))
            && target_flags.intersects(TypeFlags::INDEX)
        {
            // 68774-68776: a (union of) string literal(s) or string
            // against `keyof T` infers the reverse empty-object shape
            // contravariantly at LiteralKeyof priority.
            let empty = self.st.create_empty_object_type_from_string_literal(source);
            let TypeData::Index {
                ty: target_inner, ..
            } = self.st.tables.type_of(target).data
            else {
                unreachable!("Index flag implies data");
            };
            self.infer_from_contravariant_types_with_priority(
                empty,
                target_inner,
                InferencePriority::LITERAL_KEYOF,
            )?;
        } else if source_flags.intersects(TypeFlags::INDEXED_ACCESS)
            && target_flags.intersects(TypeFlags::INDEXED_ACCESS)
        {
            // 68777-68779: object and index types infer pairwise.
            let TypeData::IndexedAccess {
                object_type: source_object,
                index_type: source_index,
                ..
            } = self.st.tables.type_of(source).data
            else {
                unreachable!("IndexedAccess flag implies data");
            };
            let TypeData::IndexedAccess {
                object_type: target_object,
                index_type: target_index,
                ..
            } = self.st.tables.type_of(target).data
            else {
                unreachable!("IndexedAccess flag implies data");
            };
            self.infer_from_types(source_object, target_object)?;
            self.infer_from_types(source_index, target_index)?;
        } else if source_flags.intersects(TypeFlags::STRING_MAPPING)
            && target_flags.intersects(TypeFlags::STRING_MAPPING)
        {
            // 68780-68783: same intrinsic mapping symbol → operands.
            if self.st.tables.type_of(source).symbol == self.st.tables.type_of(target).symbol {
                let TypeData::StringMapping { ty: source_inner } =
                    self.st.tables.type_of(source).data
                else {
                    unreachable!("StringMapping flag implies data");
                };
                let TypeData::StringMapping { ty: target_inner } =
                    self.st.tables.type_of(target).data
                else {
                    unreachable!("StringMapping flag implies data");
                };
                self.infer_from_types(source_inner, target_inner)?;
            }
        } else if source_flags.intersects(TypeFlags::SUBSTITUTION) {
            return Err(Unsupported::new(
                "inferFromTypes Substitution-source arm (M8 — Substitution types are \
                 unconstructible before their type nodes land)",
            ));
        } else if target_flags.intersects(TypeFlags::CONDITIONAL) {
            // 68786: routed through invokeOnce (the action body is the
            // dormant M8 escape — no Conditional type is constructible
            // before M8's type nodes).
            self.invoke_once(source, target, InferAction::ToConditionalType)?;
        } else if target_flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let member_types = self.types_of(target);
            self.infer_to_multiple_types(source, &member_types, target_flags)?;
        } else if source_flags.intersects(TypeFlags::UNION) {
            // 68791-68795: distribute a union source over the target.
            for source_type in self.types_of(source) {
                self.infer_from_types(source_type, target)?;
            }
        } else if target_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            return Err(Unsupported::new("inferToTemplateLiteralType (M6 7.2c)"));
        } else {
            return Err(Unsupported::new(
                "inferFromTypes object tail — getReducedType/getApparentType + \
                 inferFromObjectTypes (M6 7.2d)",
            ));
        }
        Ok(())
    }

    /// tsc-port: inferWithPriority @6.0.3
    /// tsc-hash: be44c8fe440eb312916cf24bb641c78ce6064083703ba1db2664eb1e21feabe7
    /// tsc-span: _tsc.js:68815-68820
    fn infer_with_priority(
        &mut self,
        source: TypeId,
        target: TypeId,
        new_priority: InferencePriority,
    ) -> CheckResult2<()> {
        let save_priority = self.priority;
        self.priority |= new_priority;
        let result = self.infer_from_types(source, target);
        self.priority = save_priority;
        result
    }

    /// tsc-port: inferFromContravariantTypesWithPriority @6.0.3
    /// tsc-hash: 0131aa72c4f68f16f6549cde874df10be775f4feee110045b613b1c530955085
    /// tsc-span: _tsc.js:68821-68826
    fn infer_from_contravariant_types_with_priority(
        &mut self,
        source: TypeId,
        target: TypeId,
        new_priority: InferencePriority,
    ) -> CheckResult2<()> {
        let save_priority = self.priority;
        self.priority |= new_priority;
        let result = self.infer_from_contravariant_types(source, target);
        self.priority = save_priority;
        result
    }

    /// tsc-port: inferToMultipleTypesWithPriority @6.0.3
    /// tsc-hash: 4991227f792f48ec39032a10a770ce401747830dfff0986f12a0a54aac743480
    /// tsc-span: _tsc.js:68827-68832
    #[allow(dead_code)] // sole consumer: the dormant inferToConditionalType body (69019, M8)
    fn infer_to_multiple_types_with_priority(
        &mut self,
        source: TypeId,
        targets: &[TypeId],
        target_flags: TypeFlags,
        new_priority: InferencePriority,
    ) -> CheckResult2<()> {
        let save_priority = self.priority;
        self.priority |= new_priority;
        let result = self.infer_to_multiple_types(source, targets, target_flags);
        self.priority = save_priority;
        result
    }

    /// tsc-port: invokeOnce @6.0.3
    /// tsc-hash: c16739c2347cd9cf605ea953aaab71f5324c1e4c662a4e1c75eb95c2cf4e570a
    /// tsc-span: _tsc.js:68833-68858
    ///
    /// Pair-memoized action dispatch with the depth-2 expansion guard
    /// (isDeeplyNestedType over the walker stacks). An Err from the
    /// action propagates without running the postlude, exactly as a
    /// tsc throw would skip it — the walker (visited map, stacks) dies
    /// with the unwind, so no durable state is left inconsistent.
    fn invoke_once(
        &mut self,
        source: TypeId,
        target: TypeId,
        action: InferAction,
    ) -> CheckResult2<()> {
        let key = (source, target);
        if let Some(&status) = self.visited.get(&key) {
            self.inference_priority = self.inference_priority.min(status);
            return Ok(());
        }
        self.visited.insert(key, InferencePriority::CIRCULARITY);
        let save_inference_priority = self.inference_priority;
        self.inference_priority = InferencePriority::MAX_VALUE;
        let save_expanding_flags = self.expanding_flags;
        self.source_stack.push(source);
        self.target_stack.push(target);
        if self
            .st
            .is_deeply_nested_type(source, &self.source_stack, self.source_stack.len(), 2)
        {
            self.expanding_flags |= ExpandingFlags::SOURCE;
        }
        if self
            .st
            .is_deeply_nested_type(target, &self.target_stack, self.target_stack.len(), 2)
        {
            self.expanding_flags |= ExpandingFlags::TARGET;
        }
        if self.expanding_flags != ExpandingFlags::BOTH {
            match action {
                InferAction::ToConditionalType => self.infer_to_conditional_type(source, target)?,
            }
        } else {
            self.inference_priority = InferencePriority::CIRCULARITY;
        }
        self.target_stack.pop();
        self.source_stack.pop();
        self.expanding_flags = save_expanding_flags;
        self.visited.insert(key, self.inference_priority);
        self.inference_priority = self.inference_priority.min(save_inference_priority);
        Ok(())
    }

    /// tsc-port: inferFromMatchingTypes @6.0.3
    /// tsc-hash: aa516228e1bf3ddc4ba2a715d2d7ec78fbf257b7d386ee80f9c59a4c7efc8cee
    /// tsc-span: _tsc.js:68859-68875
    ///
    /// Infers between every matching pair and returns the unmatched
    /// remainders. tsc's undefined-until-appended matched arrays are
    /// empty vecs here — emptiness and undefined coincide because tsc
    /// only creates them via appendIfUnique.
    fn infer_from_matching_types(
        &mut self,
        sources: Vec<TypeId>,
        targets: Vec<TypeId>,
        matcher: TypeMatcher,
    ) -> CheckResult2<(Vec<TypeId>, Vec<TypeId>)> {
        let mut matched_sources: Vec<TypeId> = Vec::new();
        let mut matched_targets: Vec<TypeId> = Vec::new();
        for &t in &targets {
            for &s in &sources {
                if self.matches_pair(s, t, matcher)? {
                    self.infer_from_types(s, t)?;
                    if !matched_sources.contains(&s) {
                        matched_sources.push(s);
                    }
                    if !matched_targets.contains(&t) {
                        matched_targets.push(t);
                    }
                }
            }
        }
        Ok((
            if matched_sources.is_empty() {
                sources
            } else {
                sources
                    .into_iter()
                    .filter(|s| !matched_sources.contains(s))
                    .collect()
            },
            if matched_targets.is_empty() {
                targets
            } else {
                targets
                    .into_iter()
                    .filter(|t| !matched_targets.contains(t))
                    .collect()
            },
        ))
    }

    /// tsc-port: inferFromTypeArguments @6.0.3
    /// tsc-hash: b4d0b2ebcb9d2b0de689aa2a0e25ce26e6872585093ae317901b6da790c86a7b
    /// tsc-span: _tsc.js:68876-68885
    fn infer_from_type_arguments(
        &mut self,
        source_types: &[TypeId],
        target_types: &[TypeId],
        variances: &[VarianceFlags],
    ) -> CheckResult2<()> {
        let count = source_types.len().min(target_types.len());
        for i in 0..count {
            if i < variances.len()
                && (variances[i].bits() & VarianceFlags::VARIANCE_MASK.bits())
                    == VarianceFlags::CONTRAVARIANT.bits()
            {
                self.infer_from_contravariant_types(source_types[i], target_types[i])?;
            } else {
                self.infer_from_types(source_types[i], target_types[i])?;
            }
        }
        Ok(())
    }

    /// tsc-port: inferFromContravariantTypes @6.0.3
    /// tsc-hash: a8074c8258a769404cf35e6e2c37cadef6768bed1da2946e5dbe3d4315fe1027
    /// tsc-span: _tsc.js:68886-68890
    ///
    /// A toggle, not a set — nested contravariant positions flip back
    /// to covariant.
    fn infer_from_contravariant_types(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<()> {
        self.contravariant = !self.contravariant;
        let result = self.infer_from_types(source, target);
        self.contravariant = !self.contravariant;
        result
    }

    /// tsc-port: getInferenceInfoForType @6.0.3
    /// tsc-hash: 454be8d3105da69089cbcabb7759be1dec137fd7cd578ebd8cb56ed0a46c2aae
    /// tsc-span: _tsc.js:68898-68907
    ///
    /// Scans the walker's array (the `inferences` closure capture);
    /// returns the arena id where tsc returns the info object.
    fn get_inference_info_for_type(&self, ty: TypeId) -> Option<InferenceInfoId> {
        if self
            .st
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::TYPE_VARIABLE)
        {
            for &id in &self.inferences {
                if self.st.inference_info(id).type_parameter == ty {
                    return Some(id);
                }
            }
        }
        None
    }

    /// tsc-port: getSingleTypeVariableFromIntersectionTypes @6.0.3
    /// tsc-hash: b67232065b5fe1d714228f0c4aa174cc82bb5b1f214447186e3302fe9e207baf
    /// tsc-span: _tsc.js:68908-68918
    fn get_single_type_variable_from_intersection_types(&self, types: &[TypeId]) -> Option<TypeId> {
        let mut type_variable: Option<TypeId> = None;
        for &ty in types {
            let t = if self
                .st
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::INTERSECTION)
            {
                self.types_of(ty)
                    .into_iter()
                    .find(|&member| self.get_inference_info_for_type(member).is_some())
            } else {
                None
            };
            let t = t?;
            if type_variable.is_some_and(|type_variable| t != type_variable) {
                return None;
            }
            type_variable = Some(t);
        }
        type_variable
    }

    /// tsc-port: inferToMultipleTypes @6.0.3
    /// tsc-hash: 4773f9f3a82c98855f33df80002d728e6e84ed097889ef2b09978ce7de7d2cf4
    /// tsc-span: _tsc.js:68919-68971
    fn infer_to_multiple_types(
        &mut self,
        source: TypeId,
        targets: &[TypeId],
        target_flags: TypeFlags,
    ) -> CheckResult2<()> {
        let mut type_variable_count = 0usize;
        if target_flags.intersects(TypeFlags::UNION) {
            // 68921-68940: per-source match tracking decides whether
            // the unmatched remainder funnels into a single naked
            // type variable.
            let mut naked_type_variable: Option<TypeId> = None;
            let sources = if self.st.tables.flags_of(source).intersects(TypeFlags::UNION) {
                self.types_of(source)
            } else {
                vec![source]
            };
            let mut matched = vec![false; sources.len()];
            let mut inference_circularity = false;
            for &t in targets {
                if self.get_inference_info_for_type(t).is_some() {
                    naked_type_variable = Some(t);
                    type_variable_count += 1;
                } else {
                    for i in 0..sources.len() {
                        let save_inference_priority = self.inference_priority;
                        self.inference_priority = InferencePriority::MAX_VALUE;
                        self.infer_from_types(sources[i], t)?;
                        if self.inference_priority == self.priority {
                            matched[i] = true;
                        }
                        inference_circularity = inference_circularity
                            || self.inference_priority == InferencePriority::CIRCULARITY;
                        self.inference_priority =
                            self.inference_priority.min(save_inference_priority);
                    }
                }
            }
            if type_variable_count == 0 {
                // 68941-68947: a type variable shared by every
                // intersection constituent still receives a naked
                // inference.
                if let Some(intersection_type_variable) =
                    self.get_single_type_variable_from_intersection_types(targets)
                {
                    self.infer_with_priority(
                        source,
                        intersection_type_variable,
                        InferencePriority::NAKED_TYPE_VARIABLE,
                    )?;
                }
                return Ok(());
            }
            if type_variable_count == 1 && !inference_circularity {
                let unmatched: Vec<TypeId> = sources
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|&(i, _)| !matched[i])
                    .map(|(_, s)| s)
                    .collect();
                if !unmatched.is_empty() {
                    let union = self
                        .st
                        .get_union_type_ex(&unmatched, UnionReduction::Literal)?;
                    self.infer_from_types(
                        union,
                        naked_type_variable.expect("count == 1 implies a recorded variable"),
                    )?;
                    return Ok(());
                }
            }
        } else {
            // 68955-68963: intersection targets infer member-wise;
            // type variables are only counted here.
            for &t in targets {
                if self.get_inference_info_for_type(t).is_some() {
                    type_variable_count += 1;
                } else {
                    self.infer_from_types(source, t)?;
                }
            }
        }
        // 68964-68970: unions take any type-variable count; an
        // intersection requires exactly one.
        if if target_flags.intersects(TypeFlags::INTERSECTION) {
            type_variable_count == 1
        } else {
            type_variable_count > 0
        } {
            for &t in targets {
                if self.get_inference_info_for_type(t).is_some() {
                    self.infer_with_priority(source, t, InferencePriority::NAKED_TYPE_VARIABLE)?;
                }
            }
        }
        Ok(())
    }

    /// tsc-port: inferToConditionalType @6.0.3
    /// tsc-hash: bf377141643390f5d80731fa855630df43df7fee74e32c6d56c2fbb8fea2f7aa
    /// tsc-span: _tsc.js:69011-69021
    ///
    /// DORMANT (doc 7.2 arm dispositions): the dispatch guard requires
    /// a Conditional target and TypeData has no Conditional variant
    /// until M8 lands the type nodes, so the deepest portable point is
    /// this body escape — the checkType/extendsType/true/false reads
    /// and the ContravariantConditional split get re-cut against
    /// source and pinned when the constructors go live. The
    /// inferToMultipleTypesWithPriority helper it dispatches through
    /// is already ported above.
    fn infer_to_conditional_type(&mut self, source: TypeId, target: TypeId) -> CheckResult2<()> {
        let _ = (source, target);
        Err(Unsupported::new(
            "inferToConditionalType body (M8 — Conditional TypeData is unconstructible \
             before conditional type nodes land)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{
        CompilerOptions, ContextFlags, IndexFlags, InferenceFlags, InferencePriority, ObjectFlags,
        SymbolFlags, TypeFlags, TypeId, UnionReduction,
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

    /// The info behind LIVE slot `index` of `ctx` (tsc
    /// `context.inferences[index]`).
    fn slot<'x>(
        state: &'x CheckerState,
        ctx: super::InferenceContextId,
        index: usize,
    ) -> &'x super::InferenceInfo {
        state.inference_info(state.inference_context(ctx).inferences[index])
    }

    fn slot_mut<'x>(
        state: &'x mut CheckerState<'_>,
        ctx: super::InferenceContextId,
        index: usize,
    ) -> &'x mut super::InferenceInfo {
        let id = state.inference_context(ctx).inferences[index];
        state.inference_info_mut(id)
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
                // 68254-68255: the mapper pair's creation capture
                // mirrors the slots and their type parameters.
                assert_eq!(context.mapper_infos, context.inferences);
                assert_eq!(context.mapper_sources, vec![t, u]);
                assert_eq!(context.flags.bits(), InferenceFlags::NO_DEFAULT.bits());
                assert_eq!(context.compare_types, CompareTypesFn::Assignable);
                assert!(context.signature.is_none());
                assert!(context.return_mapper.is_none());
                assert!(context.inferred_type_parameters.is_none());
                assert!(context.intra_expression_inference_sites.is_none());
                assert!(context.outer_return_mapper.is_none());
                let mapper = context.mapper;
                let non_fixing = context.non_fixing_mapper;
                for (index, tp) in [t, u].into_iter().enumerate() {
                    let info = slot(state, ctx, index);
                    assert_eq!(info.type_parameter, tp);
                    assert!(info.candidates.is_none());
                    assert!(info.contra_candidates.is_none());
                    assert!(info.inferred_type.is_none());
                    assert!(info.priority.is_none());
                    assert!(info.top_level, "createInferenceInfo topLevel: true (68307)");
                    assert!(!info.is_fixed, "createInferenceInfo isFixed: false (68308)");
                    assert!(info.implied_arity.is_none());
                }
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
                slot_mut(state, ctx, 0).candidates = Some(vec![string]);
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
                // cloneInferenceContext clones the INFOS (fresh
                // objects: distinct ids from the original's slots);
                // lazily-attached context fields do not survive.
                assert_ne!(
                    cloned.inferences[0],
                    state.inference_context(ctx).inferences[0]
                );
                assert!(cloned.return_mapper.is_none());
                assert!(cloned.intra_expression_inference_sites.is_none());
                assert!(cloned.outer_return_mapper.is_none());
                // Fresh mapper pair over the CLONE.
                let clone_mapper = cloned.mapper;
                assert_eq!(slot(state, clone, 0).candidates, Some(vec![string]));
                match state.mapper(clone_mapper) {
                    TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                        assert_eq!(*id, clone)
                    }
                    other => panic!("clone mapper shape: {other:?}"),
                }
                // cloneInferenceInfo slices the candidate arrays: a
                // later push into the original is invisible to the
                // clone (68315 `.slice()`).
                slot_mut(state, ctx, 0)
                    .candidates
                    .as_mut()
                    .expect("candidates present")
                    .push(number);
                assert_eq!(
                    slot(state, clone, 0)
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
                slot_mut(state, ctx, 1).contra_candidates = Some(vec![string]);
                let part = state
                    .clone_inferred_part_of_context(ctx)
                    .expect("one candidate row");
                assert_eq!(state.inference_context(part).inferences.len(), 1);
                assert_eq!(slot(state, part, 0).type_parameter, u);
                assert_eq!(slot(state, part, 0).contra_candidates, Some(vec![string]));
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
                assert!(!slot(state, ctx, 0).is_fixed);
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
                slot_mut(state, ctx, 0).inferred_type = Some(string);
                slot_mut(state, ctx, 1).inferred_type = Some(string);
                let fixing = state.inference_context(ctx).mapper;
                let err = state.get_mapped_type(t, fixing).expect_err("7.3 stub");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                // 68263-68265 order: clearCachedInferences runs while
                // the row is still unfixed (its own stale cache
                // drops), THEN isFixed is set, THEN resolution.
                assert!(slot(state, ctx, 0).is_fixed);
                assert!(slot(state, ctx, 0).inferred_type.is_none());
                // Other unfixed rows lose their cache too.
                assert!(!slot(state, ctx, 1).is_fixed);
                assert!(slot(state, ctx, 1).inferred_type.is_none());
                // A second dispatch on the SAME (now fixed) row skips
                // the drain/clear preamble entirely (68262 guard).
                slot_mut(state, ctx, 1).inferred_type = Some(string);
                let err = state
                    .get_mapped_type(t, fixing)
                    .expect_err("7.3 stub again");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                assert_eq!(
                    slot(state, ctx, 1).inferred_type,
                    Some(string),
                    "fixed-row dispatch must not re-clear other caches"
                );
            },
        );
    }

    #[test]
    fn fixing_dispatch_consults_creation_capture_after_slot_replacement() {
        // The mergeInferences shape (80836 `target[i] = source[i]`),
        // simulated ahead of its 7.4 port: replace a fixed-but-
        // candidateless LIVE slot with a fresh info. tsc's thunk
        // closes over the CREATION-TIME object (68261-68267), so the
        // second fixing dispatch skips the preamble — the fresh live
        // row must stay unfixed (the 68710 candidate gate reopens)
        // and keep its cache (no clearCachedInferences run).
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                let fixing = state.inference_context(ctx).mapper;
                let _ = state.get_mapped_type(t, fixing).expect_err("7.3 stub");
                assert!(slot(state, ctx, 0).is_fixed);
                // 80786: a fresh candidate-bearing info replaces the
                // slot (isFixed starts false).
                let mut fresh = super::create_inference_info(t);
                fresh.candidates = Some(vec![string]);
                fresh.inferred_type = Some(string);
                let fresh_id = state.alloc_inference_info(fresh);
                state.inference_context_mut(ctx).inferences[0] = fresh_id;
                let _ = state.get_mapped_type(t, fixing).expect_err("7.3 stub");
                assert!(
                    !slot(state, ctx, 0).is_fixed,
                    "live merged row stays unfixed — tsc's detached capture absorbs the fix"
                );
                assert_eq!(
                    slot(state, ctx, 0).inferred_type,
                    Some(string),
                    "preamble skip must not clear the merged row's cache"
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
                assert!(state
                    .inference_context(ctx)
                    .intra_expression_inference_sites
                    .is_none());
                assert!(slot(state, ctx, 0).is_fixed);
            },
        );
    }

    #[test]
    fn fixing_dispatch_mid_drain_unwind_keeps_sites_and_fix_pending() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>() { var v: T extends string ? string : number = 1; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let literal = node_of_kind(state, tsrs2_syntax::SyntaxKind::NumericLiteral);
                let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
                state.add_intra_expression_inference_site(ctx, literal, string);
                let fixing = state.inference_context(ctx).mapper;
                // The annotated initializer HAS a contextual type
                // whose resolution unwinds (conditional-type
                // annotation — the M8 family), so the drain Errs
                // mid-loop, BEFORE the 68297 clear and the 68265 fix.
                // (A resolvable annotation no longer unwinds here:
                // 7.2's inferTypes returns Ok for variable-free
                // targets, so this pin rides the M8 escape until
                // conditional types land — re-point it then.)
                let err = state.get_mapped_type(t, fixing).expect_err("M8 escape");
                assert!(err.reason.contains("conditional types"), "{}", err.reason);
                assert!(state
                    .inference_context(ctx)
                    .intra_expression_inference_sites
                    .is_some());
                assert!(!slot(state, ctx, 0).is_fixed);
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
                    !slot(state, ctx, 0).is_fixed,
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
                slot_mut(state, ctx, 0).candidates = Some(vec![string]);
                state.push_inference_context(var_decl, Some(ctx));
                // 73444-73445: Signature flags + a candidate-bearing
                // row instantiate through the NON-fixing mapper,
                // whose resolution is the 7.3 stub today.
                let err = state
                    .instantiate_contextual_type(Some(t), var_decl, ContextFlags::SIGNATURE)
                    .expect_err("reaches the 7.3 stub through the non-fixing mapper");
                assert!(err.reason.contains("getInferredType"), "{}", err.reason);
                assert!(
                    !slot(state, ctx, 0).is_fixed,
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

    // ---- 7.2a: inferTypes / inferFromTypes spine ----

    /// A detached single-info array — the inferReverseMappedTypeWorker
    /// 68438 `inferTypes([inference], ...)` seam shape.
    fn detached_info(state: &mut CheckerState, tp: TypeId) -> super::InferenceInfoId {
        let info = super::create_inference_info(tp);
        state.alloc_inference_info(info)
    }

    #[test]
    fn infer_types_records_covariant_candidate() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                state
                    .infer_types(&[info], string, t, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
                assert!(info.contra_candidates.is_none());
                assert_eq!(info.priority, Some(InferencePriority::NONE));
                assert!(info.top_level, "T at top level of T (68732)");
                assert!(info.inferred_type.is_none());
            },
        );
    }

    #[test]
    fn infer_types_contravariant_entry_records_contra_candidate() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                // 68714: contravariant && !bivariant → contra side.
                state
                    .infer_types(&[info], string, t, InferencePriority::NONE, true)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert_eq!(info.contra_candidates.as_deref(), Some(&[string][..]));
                assert!(info.candidates.is_none());
            },
        );
    }

    #[test]
    fn equal_priority_candidates_append_unique_in_insertion_order() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let number = state.tables.intrinsics.number;
                let info = detached_info(state, t);
                for source in [string, string, number] {
                    state
                        .infer_types(&[info], source, t, InferencePriority::NONE, false)
                        .expect("live spine");
                }
                // 68727 `!contains(...)` + append: unique, in order.
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[string, number][..])
                );
            },
        );
    }

    #[test]
    fn lower_priority_resets_and_higher_priority_is_ignored() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let number = state.tables.intrinsics.number;
                let boolean = state.tables.intrinsics.boolean;
                let info = detached_info(state, t);
                state
                    .infer_types(&[info], string, t, InferencePriority::RETURN_TYPE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).priority,
                    Some(InferencePriority::RETURN_TYPE)
                );
                // 68715: numerically-lower priority wipes the record.
                state
                    .infer_types(&[info], number, t, InferencePriority::NONE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[number][..])
                );
                assert_eq!(
                    state.inference_info(info).priority,
                    Some(InferencePriority::NONE)
                );
                // 68721: a higher priority neither resets nor appends.
                state
                    .infer_types(&[info], boolean, t, InferencePriority::RETURN_TYPE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[number][..])
                );
                assert_eq!(
                    state.inference_info(info).priority,
                    Some(InferencePriority::NONE)
                );
            },
        );
    }

    #[test]
    fn fixed_info_skips_recording() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                state.inference_info_mut(info).is_fixed = true;
                state
                    .infer_types(&[info], string, t, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert!(info.candidates.is_none(), "68710 isFixed gate");
                assert!(info.priority.is_none());
            },
        );
    }

    #[test]
    fn non_inferrable_any_source_skips_recording() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let source = state.tables.intrinsics.non_inferrable_any;
                let info = detached_info(state, t);
                state
                    .infer_types(&[info], source, t, InferencePriority::NONE, false)
                    .expect("live spine");
                assert!(state.inference_info(info).candidates.is_none());
            },
        );
    }

    #[test]
    fn wildcard_propagation_records_the_marker_source() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let wildcard = state.tables.intrinsics.wildcard;
                let info = detached_info(state, t);
                // 68650-68655: target-to-target under the propagation
                // type — T receives the wildcard marker itself.
                state
                    .infer_types(&[info], wildcard, t, InferencePriority::NONE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[wildcard][..])
                );
            },
        );
    }

    #[test]
    fn blocked_string_candidate_is_skipped_before_the_priority_reset() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let blocked = state.tables.intrinsics.blocked_string;
                let info = detached_info(state, t);
                state
                    .infer_types(&[info], blocked, t, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                // 68712-68714: the return fires BEFORE 68715's reset,
                // so priority stays unrecorded too.
                assert!(info.candidates.is_none());
                assert!(info.priority.is_none());
            },
        );
    }

    #[test]
    fn union_target_unmatched_remainder_infers_into_the_naked_variable() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let number = state.tables.intrinsics.number;
                let info = detached_info(state, t);
                let target = state
                    .get_union_type_ex(&[t, number], UnionReduction::Literal)
                    .expect("union");
                // 68948-68953: one naked variable, nothing matched →
                // the unmatched remainder infers PLAIN (no priority
                // elevation).
                state
                    .infer_types(&[info], string, target, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
                assert_eq!(info.priority, Some(InferencePriority::NONE));
            },
        );
    }

    #[test]
    fn union_target_fully_matched_source_records_naked_priority() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                let target = state
                    .get_union_type_ex(&[t, string], UnionReduction::Literal)
                    .expect("union");
                // 68674-68682: the identical member strips, sources
                // empty → NakedTypeVariable inference of the ORIGINAL
                // source into the remainder.
                state
                    .infer_types(&[info], string, target, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
                assert_eq!(info.priority, Some(InferencePriority::NAKED_TYPE_VARIABLE));
            },
        );
    }

    #[test]
    fn intersection_target_fully_matched_source_aborts() {
        with_program_state(
            &[("a.ts", "function f<T>() { var x: T & string; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                let annotation = annotation_of_var(state, "x");
                let target = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection annotation");
                // 68688-68689: the identical member consumes the whole
                // source → sources empty → abort with no record (the
                // asymmetric twin of the union NakedTypeVariable path).
                state
                    .infer_types(&[info], string, target, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert!(info.candidates.is_none());
                assert!(info.priority.is_none());
            },
        );
    }

    #[test]
    fn union_source_intersection_target_records_single_naked_variable() {
        with_program_state(
            &[("a.ts", "function f<T>() { var x: T & string; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let number = state.tables.intrinsics.number;
                let boolean = state.tables.intrinsics.boolean;
                let info = detached_info(state, t);
                let annotation = annotation_of_var(state, "x");
                let target = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection annotation");
                let source = state
                    .get_union_type_ex(&[number, boolean], UnionReduction::Literal)
                    .expect("union");
                // A union source skips the 68685 reduction; the
                // intersection branch of inferToMultipleTypes counts
                // exactly one type variable (68964) and lands a naked
                // inference on it.
                state
                    .infer_types(&[info], source, target, InferencePriority::NONE, false)
                    .expect("live spine");
                let info = state.inference_info(info);
                assert_eq!(info.candidates.as_deref(), Some(&[source][..]));
                assert_eq!(info.priority, Some(InferencePriority::NAKED_TYPE_VARIABLE));
            },
        );
    }

    #[test]
    fn identical_union_source_and_target_infer_members_into_themselves() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                let union = state
                    .get_union_type_ex(&[t, string], UnionReduction::Literal)
                    .expect("union");
                // 68667-68671: source === target → per-member (t, t),
                // so T records ITSELF as its candidate.
                state
                    .infer_types(&[info], union, union, InferencePriority::NONE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[t][..])
                );
            },
        );
    }

    #[test]
    fn same_alias_reference_infers_between_argument_lists() {
        with_program_state(
            &[(
                "a.ts",
                "type Box<B> = { v: B };\nfunction f<T>() { var a: Box<T>; var b: Box<string>; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let string = state.tables.intrinsics.string;
                let info = detached_info(state, t);
                let target_annotation = annotation_of_var(state, "a");
                let target = state
                    .get_type_from_type_node(target_annotation)
                    .expect("Box<T>");
                let source_annotation = annotation_of_var(state, "b");
                let source = state
                    .get_type_from_type_node(source_annotation)
                    .expect("Box<string>");
                // 68657-68663: same alias symbol → pairwise argument
                // inference under the alias' measured variances.
                state
                    .infer_types(&[info], source, target, InferencePriority::NONE, false)
                    .expect("live spine");
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[string][..])
                );
            },
        );
    }

    // ---- 7.2b: the literal-keyof arm ----

    /// The empty-object reverse shape recorded for `"a"` vs `keyof T`
    /// (68774-68776): a contra candidate at LiteralKeyof priority
    /// whose members table holds an any-typed `a`.
    #[test]
    fn string_literal_against_keyof_records_reverse_empty_object() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let info = detached_info(state, t);
                let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
                let lit = state.tables.get_string_literal_type("a");
                state
                    .infer_types(&[info], lit, keyof_t, InferencePriority::NONE, false)
                    .expect("live arm");
                let (contra, priority, top_level) = {
                    let info = state.inference_info(info);
                    (
                        info.contra_candidates
                            .clone()
                            .expect("contravariant record"),
                        info.priority,
                        info.top_level,
                    )
                };
                assert_eq!(priority, Some(InferencePriority::LITERAL_KEYOF));
                assert!(
                    state.inference_info(info).candidates.is_none(),
                    "the toggled entry lands on the contra side (68722)"
                );
                assert!(
                    !top_level,
                    "T is not at top level of `keyof T` — record-time demotion (68732)"
                );
                let [empty] = contra[..] else {
                    panic!("exactly one contra candidate");
                };
                assert!(state.tables.flags_of(empty).intersects(TypeFlags::OBJECT));
                assert!(state
                    .tables
                    .object_flags_of(empty)
                    .intersects(ObjectFlags::ANONYMOUS));
                let members = state
                    .links
                    .ty(empty)
                    .resolved_members
                    .resolved()
                    .expect("created resolved");
                let resolved = state.members_of(members);
                assert_eq!(resolved.properties.len(), 1);
                assert!(resolved.index_infos.is_empty());
                let prop = *resolved.members.get("a").expect("member `a`");
                assert_eq!(
                    state.links.symbol(prop).type_of_symbol.resolved(),
                    Some(state.tables.intrinsics.any),
                    "literalProp.links.type = anyType (68364)"
                );
            },
        );
    }

    /// A plain-string source contributes only the string→emptyObject
    /// index signature (68371-68376).
    #[test]
    fn plain_string_against_keyof_builds_index_signature_shape() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let info = detached_info(state, t);
                let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
                let string = state.tables.intrinsics.string;
                state
                    .infer_types(&[info], string, keyof_t, InferencePriority::NONE, false)
                    .expect("live arm");
                let contra = state
                    .inference_info(info)
                    .contra_candidates
                    .clone()
                    .expect("contravariant record");
                let [empty] = contra[..] else {
                    panic!("exactly one contra candidate");
                };
                let members = state
                    .links
                    .ty(empty)
                    .resolved_members
                    .resolved()
                    .expect("created resolved");
                let resolved = state.members_of(members);
                assert!(resolved.properties.is_empty());
                let [ref info] = resolved.index_infos[..] else {
                    panic!("exactly one index info");
                };
                assert_eq!(info.key_type, state.tables.intrinsics.string);
                assert_eq!(info.value_type, state.empty_object_type);
                assert!(!info.is_readonly);
            },
        );
    }

    /// forEachType distribution + the StringLiteral filter + leading-
    /// underscore escaping: `"a" | "__x" | 1` keeps the string members
    /// (escaped) and drops the number literal (68359-68361).
    #[test]
    fn literal_union_against_keyof_filters_and_escapes_members() {
        with_program_state(
            &[("a.ts", GENERIC_SRC)],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let info = detached_info(state, t);
                let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
                let lit_a = state.tables.get_string_literal_type("a");
                let lit_dunder = state.tables.get_string_literal_type("__x");
                let lit_one = state.tables.get_number_literal_type(1.0);
                let union = state
                    .get_union_type_ex(&[lit_a, lit_dunder, lit_one], UnionReduction::Literal)
                    .expect("literal union");
                state
                    .infer_types(&[info], union, keyof_t, InferencePriority::NONE, false)
                    .expect("live arm");
                let contra = state
                    .inference_info(info)
                    .contra_candidates
                    .clone()
                    .expect("contravariant record");
                let [empty] = contra[..] else {
                    panic!("exactly one contra candidate");
                };
                let members = state
                    .links
                    .ty(empty)
                    .resolved_members
                    .resolved()
                    .expect("created resolved");
                let resolved = state.members_of(members);
                assert_eq!(
                    resolved.members.keys().cloned().collect::<Vec<_>>(),
                    vec!["a".to_owned(), "___x".to_owned()],
                    "union order kept, number literal dropped, __ escaped"
                );
                assert!(
                    resolved.index_infos.is_empty(),
                    "no String member in the union"
                );
            },
        );
    }
}
