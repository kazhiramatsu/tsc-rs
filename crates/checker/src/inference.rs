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
//! The collector (`infer_types`, 7.2) and the resolver
//! (`get_inferred_type` + the constraint clamp, 7.3) are live; 7.4
//! wires inferTypeArguments/chooseOverload on top of them (every
//! production pushInferenceContext site still passes None until
//! then).

use std::collections::HashMap;

use tsc_syntax::{escape_leading_underscores, NodeId, SyntaxKind};
use tsc_types::{
    ContextFlags, ElementFlags, ExpandingFlags, InferenceFlags, InferencePriority,
    IntersectionFlags, LiteralValue, ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction, VarianceFlags,
};

use crate::instantiate::{DeferredMapperTargets, MapperId, TypeMapper};
use crate::links::LinkSlot;
use crate::state::{CheckResult, CheckerState, IndexInfo, SignatureId, SignatureKind};
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
    pub(crate) priority: Option<InferencePriority>,
    pub(crate) top_level: bool,
    pub(crate) is_fixed: bool,
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
/// isRelatedToWorker (66368; conditional relation is live since 9.6d).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompareTypesFn {
    /// compareTypesAssignable — consumed by getInferredType's
    /// constraint clamp (69300-69306, stage 7.3).
    Assignable,
    /// signatureRelatedTo's isRelatedToWorker closure (67070-67080),
    /// handed through compareSignaturesRelated 64507 into
    /// instantiateSignatureInContextOf (M6 7.5 head rebuild). The
    /// closure's frame lives OUTSIDE the context — the walker loans
    /// it and PARKS the loan on `relation_frame_loan` for the whole
    /// iSICO call (a parameter cannot reach the re-entrant
    /// getInferredType resolutions through the non-fixing mapper's
    /// deferred thunks — M6 7.5d review fix). Consuming this variant
    /// with no parked loan is a programmer error (iSICO's context is
    /// frame-local and nothing re-reads it after iSICO returns).
    RelationFrame,
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
    pub(crate) signature: Option<SignatureId>,
    pub(crate) flags: InferenceFlags,
    pub(crate) compare_types: CompareTypesFn,
    pub(crate) mapper: MapperId,
    pub(crate) non_fixing_mapper: MapperId,
    pub(crate) return_mapper: Option<MapperId>,
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
    /// CheckAbort abandons the whole surrounding resolution and the
    /// context with it (contexts are per-resolution transients).
    pub(crate) fn infer_from_intra_expression_sites(
        &mut self,
        context: InferenceContextId,
    ) -> CheckResult<()> {
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
    ) -> CheckResult<TypeId> {
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
    ) -> CheckResult<TypeId> {
        self.get_inferred_type(context, index)
    }

    /// tsc-port: createBackreferenceMapper @6.0.3
    /// tsc-hash: 39eeb2ff24d79f21daf1e82c48acf5c5600a14e982f49d1be0275188c3f8760f
    /// tsc-span: _tsc.js:63381-63384
    ///
    /// The default-instantiation shield (getInferredType 69289): every
    /// type parameter at or after `index` maps to unknown, so a
    /// parameter default can only see the inferences BEFORE it —
    /// forward references collapse instead of recursing. Reads the
    /// LIVE slots (`context.inferences`), not the creation capture.
    fn create_backreference_mapper(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> MapperId {
        let forward_inferences: Vec<InferenceInfoId> =
            self.inference_context(context).inferences[index..].to_vec();
        let sources: Vec<TypeId> = forward_inferences
            .iter()
            .map(|&info| self.inference_info(info).type_parameter)
            .collect();
        let targets = vec![self.tables.intrinsics.unknown; sources.len()];
        self.create_type_mapper(sources, Some(targets))
    }

    /// tsc-port: createOuterReturnMapper @6.0.3
    /// tsc-hash: dbf215149bf9450aedc8e51f8166a45bc93be51494c3b370b4273b05f4e529dd
    /// tsc-span: _tsc.js:63385-63387
    ///
    /// `outerReturnMapper ??=` — one merged mapper per context,
    /// cached on the context. Lives here rather than instantiate.rs
    /// because it is context-cache machinery (consumed by
    /// inferTypeArguments' phase-a2 return inference, 75958).
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
    ) -> CheckResult<bool> {
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
    pub(crate) fn is_type_parameter_at_top_level(
        &mut self,
        ty: TypeId,
        tp: TypeId,
        depth: usize,
    ) -> CheckResult<bool> {
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
            let true_type = self.get_true_type_from_conditional_type(ty)?;
            if self.is_type_parameter_at_top_level(true_type, tp, depth + 1)? {
                return Ok(true);
            }
            let false_type = self.get_false_type_from_conditional_type(ty)?;
            return self.is_type_parameter_at_top_level(false_type, tp, depth + 1);
        }
        Ok(false)
    }

    /// tsc-port: isTypeParameterAtTopLevelInReturnType @6.0.3
    /// tsc-hash: d3223619e7c24199528e5f9f3485c0b57cc298794abef84767a3fd97576a8aca
    /// tsc-span: _tsc.js:68352-68355
    ///
    /// The widen-literals gate's syntactic probe: a predicate signature
    /// walks the predicate's type (None type → false), everything else
    /// walks the return type.
    fn is_type_parameter_at_top_level_in_return_type(
        &mut self,
        signature: SignatureId,
        type_parameter: TypeId,
    ) -> CheckResult<bool> {
        if let Some(type_predicate) = self.get_type_predicate_of_signature(signature)? {
            return match type_predicate.ty {
                Some(predicate_type) => {
                    self.is_type_parameter_at_top_level(predicate_type, type_parameter, 0)
                }
                None => Ok(false),
            };
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        self.is_type_parameter_at_top_level(return_type, type_parameter, 0)
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
            let Some(value) = value.to_utf8() else {
                // The binder's symbol table is UTF-8 keyed. Do not
                // alias an unpaired-surrogate literal to replacement
                // text or to an escaped spelling.
                continue;
            };
            let name = escape_leading_underscores(&value);
            let literal_prop = self
                .binder
                .create_symbol(SymbolFlags::PROPERTY, name.clone());
            self.links.set_fresh_symbol_type(
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

    /// tsc-port: getTypeFromInference @6.0.3
    /// tsc-hash: 78fc093a33fc40d4cd89c737b1115002b1cdaff32c2d7448de73181a06503317
    /// tsc-span: _tsc.js:68506-68508
    ///
    /// The signature-less resolution arm (getInferredType 69293):
    /// covariant candidates union under Subtype reduction,
    /// contravariant candidates intersect, neither → None (JS
    /// undefined, folded to the AnyDefault/unknown default by the
    /// caller).
    fn get_type_from_inference(
        &mut self,
        inference: InferenceInfoId,
    ) -> CheckResult<Option<TypeId>> {
        if let Some(candidates) = self.inference_info(inference).candidates.clone() {
            return Ok(Some(
                self.get_union_type_ex(&candidates, UnionReduction::Subtype)?,
            ));
        }
        if let Some(contra_candidates) = self.inference_info(inference).contra_candidates.clone() {
            return Ok(Some(self.get_intersection_type(
                &contra_candidates,
                IntersectionFlags::NONE,
            )?));
        }
        Ok(None)
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
    ) -> CheckResult<()> {
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

    /// tsc-port: inferTypeForHomomorphicMappedType @6.0.3
    /// tsc-hash: a9eb863be94f7bac6a664c5a17382d2b6bc49583f831479de4827200ae07732a
    /// tsc-span: _tsc.js:68386-68394
    pub(crate) fn infer_type_for_homomorphic_mapped_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        constraint: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let key = (source, target, constraint);
        if let Some(cached) = self.reverse_homomorphic_mapped_cache.get(&key) {
            return Ok(*cached);
        }
        let reversed = self.create_reverse_mapped_type(source, target, constraint)?;
        self.reverse_homomorphic_mapped_cache.insert(key, reversed);
        Ok(reversed)
    }

    /// tsc-port: isPartiallyInferableType @6.0.3
    /// tsc-hash: 83b3b49b9e9ff1e05c6d4329e2cc159c1a01293d24153bdf88698bb4233cd7d2
    /// tsc-span: _tsc.js:68395-68397
    fn is_partially_inferable_type(&mut self, ty: TypeId) -> CheckResult<bool> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::NON_INFERRABLE_TYPE)
        {
            return Ok(true);
        }
        if self.is_object_literal_type(ty) {
            for property in self.get_properties_of_type(ty)? {
                let property_type = self.get_type_of_symbol(property)?;
                if self.is_partially_inferable_type(property_type)? {
                    return Ok(true);
                }
            }
        }
        if self.tables.is_tuple_type(ty) {
            for element in self.get_type_arguments(ty)? {
                if self.is_partially_inferable_type(element)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: createReverseMappedType @6.0.3
    /// tsc-hash: 277f8b1a9d8c23df175dd5dedf8d81cfb5567154f767cc11fb26c7d132c557a8
    /// tsc-span: _tsc.js:68398-68426
    fn create_reverse_mapped_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        constraint: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let string = self.tables.intrinsics.string;
        let has_string_index = self.get_index_info_of_type(source, string)?.is_some();
        let properties = self.get_properties_of_type(source)?;
        if !has_string_index
            && (properties.is_empty() || !self.is_partially_inferable_type(source)?)
        {
            return Ok(None);
        }

        if self.is_array_type(source)? {
            let element = self
                .get_type_arguments(source)?
                .first()
                .copied()
                .expect("array references carry an element type");
            let Some(reversed_element) =
                self.infer_reverse_mapped_type(element, target, constraint)?
            else {
                return Ok(None);
            };
            let readonly = self.is_readonly_array_type(source)?;
            return self.create_array_type(reversed_element, readonly).map(Some);
        }

        if self.tables.is_tuple_type(source) {
            let source_elements = self.get_type_arguments(source)?;
            let mut reversed_elements = Vec::with_capacity(source_elements.len());
            for element in source_elements {
                let Some(reversed) = self.infer_reverse_mapped_type(element, target, constraint)?
                else {
                    return Ok(None);
                };
                reversed_elements.push(reversed);
            }
            let tuple_target = self.tables.reference_target(source);
            let TypeData::TupleTarget(data) = self.tables.type_of(tuple_target).data.clone() else {
                unreachable!("tuple source has a tuple target");
            };
            let element_flags = if self
                .get_mapped_type_modifiers(target)
                .intersects(tsc_types::MappedTypeModifiers::INCLUDE_OPTIONAL)
            {
                data.element_flags
                    .iter()
                    .map(|flags| {
                        if flags.intersects(ElementFlags::OPTIONAL) {
                            ElementFlags::REQUIRED
                        } else {
                            *flags
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                data.element_flags.to_vec()
            };
            return self
                .create_tuple_type_forced(
                    &reversed_elements,
                    Some(&element_flags),
                    data.readonly,
                    data.labeled_element_declarations.as_deref(),
                )
                .map(Some);
        }

        Ok(Some(
            self.tables
                .create_reverse_mapped_type(source, target, constraint),
        ))
    }

    /// tsc-port: inferReverseMappedTypeWorker @6.0.3
    /// tsc-hash: 3d69e84dbaf8816b638243a583d8d06009ca51e398b488dbcdac86089189712f
    /// tsc-span: _tsc.js:68434-68440
    fn infer_reverse_mapped_type_worker(
        &mut self,
        source_type: TypeId,
        target: TypeId,
        constraint: TypeId,
    ) -> CheckResult<TypeId> {
        let TypeData::Index {
            ty: constraint_type,
            ..
        } = self.tables.type_of(constraint).data
        else {
            unreachable!("reverse mapped inference constraint is an index type");
        };
        let mapped_parameter = self.get_type_parameter_from_mapped_type(target)?;
        let type_parameter = self.get_indexed_access_type(
            constraint_type,
            mapped_parameter,
            tsc_types::AccessFlags::NONE,
            None,
            None,
            None,
        )?;
        let template_type = self.get_template_type_from_mapped_type(target)?;
        let inference = self.alloc_inference_info(create_inference_info(type_parameter));
        self.infer_types(
            &[inference],
            source_type,
            template_type,
            InferencePriority::NONE,
            false,
        )?;
        let inferred = self
            .get_type_from_inference(inference)?
            .unwrap_or(self.tables.intrinsics.unknown);
        self.get_widened_type(inferred)
    }

    /// tsc-port: inferReverseMappedType @6.0.3
    /// tsc-hash: 47bc2d121727aacb819dd03fae053613dd3b3c2105a5e5e0e371f227c8bff20a
    /// tsc-span: _tsc.js:68441-68460
    pub(crate) fn infer_reverse_mapped_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        constraint: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let key = (source, target, constraint);
        if let Some(cached) = self.reverse_mapped_cache.get(&key) {
            return Ok(Some(cached.unwrap_or(self.tables.intrinsics.unknown)));
        }

        self.reverse_mapped_source_stack.push(source);
        self.reverse_mapped_target_stack.push(target);
        let saved_flags = self.reverse_expanding_flags;
        if self.is_deeply_nested_type(
            source,
            &self.reverse_mapped_source_stack,
            self.reverse_mapped_source_stack.len(),
            2,
        ) {
            self.reverse_expanding_flags |= ExpandingFlags::SOURCE;
        }
        if self.is_deeply_nested_type(
            target,
            &self.reverse_mapped_target_stack,
            self.reverse_mapped_target_stack.len(),
            2,
        ) {
            self.reverse_expanding_flags |= ExpandingFlags::TARGET;
        }
        let result = if self.reverse_expanding_flags != ExpandingFlags::BOTH {
            self.infer_reverse_mapped_type_worker(source, target, constraint)
                .map(Some)
        } else {
            Ok(None)
        };
        self.reverse_mapped_target_stack.pop();
        self.reverse_mapped_source_stack.pop();
        self.reverse_expanding_flags = saved_flags;
        let inferred = result?;
        self.reverse_mapped_cache.insert(key, inferred);
        Ok(inferred)
    }

    /// tsc-port: hasPrimitiveConstraint @6.0.3
    /// tsc-hash: c32a05bffced6341169f55c1becde664b444f157943fb77ed7b653ff9e09ef16
    /// tsc-span: _tsc.js:69240-69243
    fn has_primitive_constraint(&mut self, ty: TypeId) -> CheckResult<bool> {
        let Some(constraint) = self.get_constraint_of_type_parameter(ty)? else {
            return Ok(false);
        };
        if self
            .tables
            .flags_of(constraint)
            .intersects(TypeFlags::CONDITIONAL)
        {
            let constraint = self.get_default_constraint_of_conditional_type(constraint)?;
            return Ok(self.maybe_type_of_kind(
                constraint,
                TypeFlags::PRIMITIVE
                    | TypeFlags::INDEX
                    | TypeFlags::TEMPLATE_LITERAL
                    | TypeFlags::STRING_MAPPING,
            ));
        }
        Ok(self.maybe_type_of_kind(
            constraint,
            TypeFlags::PRIMITIVE
                | TypeFlags::INDEX
                | TypeFlags::TEMPLATE_LITERAL
                | TypeFlags::STRING_MAPPING,
        ))
    }

    /// tsc-port: unionObjectAndArrayLiteralCandidates @6.0.3
    /// tsc-hash: 1050c7b9b9828a62971f26f442fe29df1b33e2127f62d6633b03b2ac8a7440af
    /// tsc-span: _tsc.js:69250-69259
    ///
    /// Order is observable: the merged subtype-union of the literal
    /// candidates lands AFTER every non-literal candidate
    /// (concatenate(filter(...), [literalsType])).
    fn union_object_and_array_literal_candidates(
        &mut self,
        candidates: Vec<TypeId>,
    ) -> CheckResult<Vec<TypeId>> {
        if candidates.len() > 1 {
            let object_literals: Vec<TypeId> = candidates
                .iter()
                .copied()
                .filter(|&t| self.is_object_or_array_literal_type(t))
                .collect();
            if !object_literals.is_empty() {
                let literals_type =
                    self.get_union_type_ex(&object_literals, UnionReduction::Subtype)?;
                let mut result: Vec<TypeId> = candidates
                    .iter()
                    .copied()
                    .filter(|&t| !self.is_object_or_array_literal_type(t))
                    .collect();
                result.push(literals_type);
                return Ok(result);
            }
        }
        Ok(candidates)
    }

    /// tsc-port: getContravariantInference @6.0.3
    /// tsc-hash: b0c99208df0211cd206c6aff8b10cb54f167138438899d9f58fbcc92696501ad
    /// tsc-span: _tsc.js:69260-69262
    fn get_contravariant_inference(&mut self, inference: InferenceInfoId) -> CheckResult<TypeId> {
        let contra_candidates = self
            .inference_info(inference)
            .contra_candidates
            .clone()
            .expect("caller-guarded (69279: contraCandidates checked before the call)");
        if self
            .inference_info(inference)
            .priority
            .unwrap_or(InferencePriority::NONE)
            .intersects(InferencePriority::PRIORITY_IMPLIES_COMBINATION)
        {
            self.get_intersection_type(&contra_candidates, IntersectionFlags::NONE)
        } else {
            self.get_common_subtype(&contra_candidates)
        }
    }

    /// tsc-port: getCovariantInference @6.0.3
    /// tsc-hash: 223befe28c7043cabe04b0e85f88d1cc00d8e64919565301fec6cd60d4ceb42d
    /// tsc-span: _tsc.js:69263-69270
    ///
    /// The widen-literals RULE (checker-key §2.1): a primitive-ish (or
    /// const) constraint keeps literals at their regular form; a
    /// top-level inference widens unless the parameter sits at top
    /// level of an unfixed signature's return type. Evaluation order
    /// preserved — the return-type walk only runs when the fixed bit
    /// is clear.
    fn get_covariant_inference(
        &mut self,
        inference: InferenceInfoId,
        signature: SignatureId,
    ) -> CheckResult<TypeId> {
        let raw_candidates = self
            .inference_info(inference)
            .candidates
            .clone()
            .expect("caller-guarded (69277: candidates checked before the call)");
        let candidates = self.union_object_and_array_literal_candidates(raw_candidates)?;
        let type_parameter = self.inference_info(inference).type_parameter;
        let primitive_constraint = self.has_primitive_constraint(type_parameter)?
            || self.is_const_type_variable(Some(type_parameter), 0)?;
        let widen_literal_types = !primitive_constraint
            && self.inference_info(inference).top_level
            && (self.inference_info(inference).is_fixed
                || !self
                    .is_type_parameter_at_top_level_in_return_type(signature, type_parameter)?);
        let base_candidates = if primitive_constraint {
            candidates
                .iter()
                .map(|&t| self.tables.get_regular_type_of_literal_type(t))
                .collect::<Vec<_>>()
        } else if widen_literal_types {
            let mut widened = Vec::with_capacity(candidates.len());
            for &t in &candidates {
                widened.push(self.get_widened_literal_type(t)?);
            }
            widened
        } else {
            candidates
        };
        let unwidened_type = if self
            .inference_info(inference)
            .priority
            .unwrap_or(InferencePriority::NONE)
            .intersects(InferencePriority::PRIORITY_IMPLIES_COMBINATION)
        {
            self.get_union_type_ex(&base_candidates, UnionReduction::Subtype)?
        } else {
            self.get_common_supertype(&base_candidates)?
        };
        self.get_widened_type(unwidened_type)
    }

    /// tsrs-native: `context.compareTypes(...)` dispatch over the
    /// CompareTypesFn closed set (see the enum): tsc stores a function
    /// reference on the context. compareTypesAssignable's (68239)
    /// Ternary truthiness is exactly isTypeAssignableTo; the
    /// RelationFrame worker takes the PARKED loan
    /// (`relation_frame_loan`, see engine.rs RelationFrameLoan),
    /// re-assembles the loaning walker around the compare, and puts
    /// every mutation back — Err or not — so the walker's
    /// restore_frame sees the final state (truthiness = not False —
    /// Maybe counts as related, exactly tsc's `!compareTypes(...)`
    /// test). A RE-ENTRANT compare (the in-flight walk's lazy member
    /// instantiation resolving a forward slot through the deferred
    /// non-fixing mapper) finds the loan checked out and runs a FRESH
    /// sub-walk under the same relation/intersectionState — tsc
    /// re-enters isRelatedTo on the ambient closure frame there;
    /// Rust cannot alias the checked-out fields, so the fresh walk's
    /// maybe stack and budget start empty (verdict-visible only under
    /// in-flight-pair cycles/budget exhaustion; relation-cache
    /// commits still land on the state).
    fn compare_inference_types(
        &mut self,
        context: InferenceContextId,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<bool> {
        match self.inference_context(context).compare_types {
            CompareTypesFn::Assignable => self.is_type_assignable_to(source, target),
            CompareTypesFn::RelationFrame => {
                use crate::engine::{RelationChecker, RelationFrame, RelationFrameLoan};
                match std::mem::replace(&mut self.relation_frame_loan, RelationFrameLoan::None) {
                    RelationFrameLoan::Available(frame) => {
                        let RelationFrame {
                            relation,
                            maybe_keys,
                            maybe_keys_set,
                            source_stack,
                            target_stack,
                            maybe_count,
                            source_depth,
                            target_depth,
                            expanding_flags,
                            overflow,
                            relation_count,
                            intersection_state,
                        } = frame;
                        self.relation_frame_loan = RelationFrameLoan::InFlight {
                            relation,
                            intersection_state,
                        };
                        let mut checker = RelationChecker {
                            st: self,
                            relation,
                            maybe_keys,
                            maybe_keys_set,
                            source_stack,
                            target_stack,
                            maybe_count,
                            source_depth,
                            target_depth,
                            expanding_flags,
                            overflow,
                            relation_count,
                            error_state: Default::default(),
                        };
                        let related = checker.is_related_to(
                            source,
                            target,
                            tsc_types::RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        );
                        let RelationChecker {
                            maybe_keys,
                            maybe_keys_set,
                            source_stack,
                            target_stack,
                            maybe_count,
                            source_depth,
                            target_depth,
                            expanding_flags,
                            overflow,
                            relation_count,
                            ..
                        } = checker;
                        self.relation_frame_loan = RelationFrameLoan::Available(RelationFrame {
                            relation,
                            maybe_keys,
                            maybe_keys_set,
                            source_stack,
                            target_stack,
                            maybe_count,
                            source_depth,
                            target_depth,
                            expanding_flags,
                            overflow,
                            relation_count,
                            intersection_state,
                        });
                        Ok(related? != tsc_types::Ternary::FALSE)
                    }
                    RelationFrameLoan::InFlight {
                        relation,
                        intersection_state,
                    } => {
                        self.relation_frame_loan = RelationFrameLoan::InFlight {
                            relation,
                            intersection_state,
                        };
                        // Fresh sub-walk (see the fn comment): budget
                        // seeded exactly like a checkTypeRelatedTo
                        // entry; frame mutations are dropped (the
                        // in-flight owner's fields come back
                        // untouched).
                        let relation_count =
                            (16_000_000 - self.relations.cache(relation).len() as i64) >> 3;
                        let mut checker = RelationChecker {
                            st: self,
                            relation,
                            maybe_keys: Vec::new(),
                            maybe_keys_set: std::collections::HashSet::new(),
                            source_stack: Vec::new(),
                            target_stack: Vec::new(),
                            maybe_count: 0,
                            source_depth: 0,
                            target_depth: 0,
                            expanding_flags: tsc_types::ExpandingFlags::NONE,
                            overflow: false,
                            relation_count,
                            error_state: Default::default(),
                        };
                        let related = checker.is_related_to(
                            source,
                            target,
                            tsc_types::RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        )?;
                        Ok(related != tsc_types::Ternary::FALSE)
                    }
                    RelationFrameLoan::None => panic!(
                        "RelationFrame compare_types consumed without a parked frame loan — \
                         the B8 generic arm parks it around instantiateSignatureInContextOf \
                         (75910-75924) and nothing re-reads the context after iSICO returns"
                    ),
                }
            }
        }
    }

    /// tsc-port: getInferredType @6.0.3
    /// tsc-hash: d2c8c7eb89a8492d264bf6b61f2df05e3f930e7fb580cec3d3693b9907ab3f25
    /// tsc-span: _tsc.js:69271-69313
    ///
    /// Resolution of ONE live slot, memoized on the info. Write order
    /// is load-bearing: the pre-clamp memo (69296) lands BEFORE the
    /// constraint work so a re-entrant resolution through the
    /// non-fixing mapper (constraint/default instantiation below) sees
    /// the unclamped value instead of recursing forever; the clamp
    /// then overwrites (69309). ReturnType-priority violations FILTER
    /// to the compatible part (priority EQUALITY, not mask); all
    /// others go never → fallback → instantiated constraint.
    /// clearActiveMapperCaches (69310) runs on every miss — a fresh
    /// resolution invalidates every in-flight active-mapper cache.
    pub(crate) fn get_inferred_type(
        &mut self,
        context: InferenceContextId,
        index: usize,
    ) -> CheckResult<TypeId> {
        let inference = self.inference_context(context).inferences[index];
        if let Some(cached) = self.inference_info(inference).inferred_type {
            return Ok(cached);
        }
        let mut inferred_type: Option<TypeId> = None;
        let mut fallback_type: Option<TypeId> = None;
        if let Some(signature) = self.inference_context(context).signature {
            let inferred_covariant_type = if self.inference_info(inference).candidates.is_some() {
                Some(self.get_covariant_inference(inference, signature)?)
            } else {
                None
            };
            let inferred_contravariant_type =
                if self.inference_info(inference).contra_candidates.is_some() {
                    Some(self.get_contravariant_inference(inference)?)
                } else {
                    None
                };
            if inferred_covariant_type.is_some() || inferred_contravariant_type.is_some() {
                // 69281: preferCovariantType — JS precedence groups as
                // cov && (!contra || (!(cov & Never|Any) && some(...)
                // && every(...))).
                let prefer_covariant_type = match inferred_covariant_type {
                    None => false,
                    Some(cov) => {
                        if inferred_contravariant_type.is_none() {
                            true
                        } else if self
                            .tables
                            .flags_of(cov)
                            .intersects(TypeFlags::NEVER | TypeFlags::ANY)
                        {
                            false
                        } else {
                            // some(inference.contraCandidates, t =>
                            // isTypeAssignableTo(cov, t))
                            let contra_candidates = self
                                .inference_info(inference)
                                .contra_candidates
                                .clone()
                                .unwrap_or_default();
                            let mut some_contra_assignable = false;
                            for t in contra_candidates {
                                if self.is_type_assignable_to(cov, t)? {
                                    some_contra_assignable = true;
                                    break;
                                }
                            }
                            if !some_contra_assignable {
                                false
                            } else {
                                // every(context.inferences, other =>
                                //   (other !== inference &&
                                //    constraintOf(other.tp) !== inf.tp)
                                //   || every(other.candidates, t =>
                                //        isTypeAssignableTo(t, cov)))
                                // — && short-circuits: the constraint
                                // probe never runs on the row itself;
                                // None candidates ⇒ vacuous true
                                // (helper `every`, _tsc.js:80).
                                let inference_type_parameter =
                                    self.inference_info(inference).type_parameter;
                                let all_inferences: Vec<InferenceInfoId> =
                                    self.inference_context(context).inferences.clone();
                                let mut every_sibling_compatible = true;
                                'siblings: for other in all_inferences {
                                    if other != inference {
                                        let other_type_parameter =
                                            self.inference_info(other).type_parameter;
                                        if self.get_constraint_of_type_parameter(
                                            other_type_parameter,
                                        )? != Some(inference_type_parameter)
                                        {
                                            continue;
                                        }
                                    }
                                    if let Some(other_candidates) =
                                        self.inference_info(other).candidates.clone()
                                    {
                                        for t in other_candidates {
                                            if !self.is_type_assignable_to(t, cov)? {
                                                every_sibling_compatible = false;
                                                break 'siblings;
                                            }
                                        }
                                    }
                                }
                                every_sibling_compatible
                            }
                        }
                    }
                };
                inferred_type = if prefer_covariant_type {
                    inferred_covariant_type
                } else {
                    inferred_contravariant_type
                };
                fallback_type = if prefer_covariant_type {
                    inferred_contravariant_type
                } else {
                    inferred_covariant_type
                };
            } else if self
                .inference_context(context)
                .flags
                .intersects(InferenceFlags::NO_DEFAULT)
            {
                // 69285: silentNeverType carries NonInferrableType, so
                // the placeholder can never become a candidate later.
                inferred_type = Some(self.tables.intrinsics.silent_never);
            } else {
                let type_parameter = self.inference_info(inference).type_parameter;
                if let Some(default_type) = self.get_default_from_type_parameter(type_parameter)? {
                    // 69289: defaults instantiate under the
                    // backreference mapper merged with the non-fixing
                    // mapper — forward parameters collapse to unknown.
                    let backreference = self.create_backreference_mapper(context, index);
                    let non_fixing_mapper = self.inference_context(context).non_fixing_mapper;
                    let merged = self.merge_type_mappers(Some(backreference), non_fixing_mapper);
                    inferred_type = Some(self.instantiate_type(default_type, Some(merged))?);
                }
            }
        } else {
            inferred_type = self.get_type_from_inference(inference)?;
        }
        // 69296: the pre-clamp memo write (see the fn comment).
        let memo = match inferred_type {
            Some(t) => t,
            None => {
                let any_default = self
                    .inference_context(context)
                    .flags
                    .intersects(InferenceFlags::ANY_DEFAULT);
                self.get_default_type_argument_type(any_default)
            }
        };
        self.inference_info_mut(inference).inferred_type = Some(memo);
        let type_parameter = self.inference_info(inference).type_parameter;
        if let Some(constraint) = self.get_constraint_of_type_parameter(type_parameter)? {
            let non_fixing_mapper = self.inference_context(context).non_fixing_mapper;
            let instantiated_constraint =
                self.instantiate_type(constraint, Some(non_fixing_mapper))?;
            if let Some(t) = inferred_type {
                let constraint_with_this =
                    self.get_type_with_this_argument(instantiated_constraint, Some(t), false)?;
                if !self.compare_inference_types(context, t, constraint_with_this)? {
                    let filtered_by_constraint = if self.inference_info(inference).priority
                        == Some(InferencePriority::RETURN_TYPE)
                    {
                        self.filter_type_with(t, |state, member| {
                            state.compare_inference_types(context, member, constraint_with_this)
                        })?
                    } else {
                        self.tables.intrinsics.never
                    };
                    inferred_type = if !self
                        .tables
                        .flags_of(filtered_by_constraint)
                        .intersects(TypeFlags::NEVER)
                    {
                        Some(filtered_by_constraint)
                    } else {
                        None
                    };
                }
            }
            if inferred_type.is_none() {
                inferred_type = Some(match fallback_type {
                    Some(fallback) => {
                        let fallback_with_this = self.get_type_with_this_argument(
                            instantiated_constraint,
                            Some(fallback),
                            false,
                        )?;
                        if self.compare_inference_types(context, fallback, fallback_with_this)? {
                            fallback
                        } else {
                            instantiated_constraint
                        }
                    }
                    None => instantiated_constraint,
                });
            }
            self.inference_info_mut(inference).inferred_type = inferred_type;
        }
        self.clear_active_mapper_caches();
        Ok(self
            .inference_info(inference)
            .inferred_type
            .expect("memo written above (69296/69309)"))
    }

    /// tsc-port: getInferredTypes @6.0.3
    /// tsc-hash: d08f983f8d34190b05cc662bae55c77da341bccf5e43a9b8aeae1ae186d961f5
    /// tsc-span: _tsc.js:69317-69323
    ///
    /// Slot order = type-parameter order; resolution of slot i can
    /// re-enter later slots through the non-fixing mapper, so the
    /// loop's per-index call rides the memo.
    pub(crate) fn get_inferred_types(
        &mut self,
        context: InferenceContextId,
    ) -> CheckResult<Vec<TypeId>> {
        let len = self.inference_context(context).inferences.len();
        let mut result = Vec::with_capacity(len);
        for index in 0..len {
            result.push(self.get_inferred_type(context, index)?);
        }
        Ok(result)
    }

    /// tsc-port: applyToParameterTypes @6.0.3
    /// tsc-hash: 95daf9e8bfe59dd9deb8b5b454837cd07bf3d0d1a3119f554a2d6e316135be3c
    /// tsc-span: _tsc.js:68198-68223
    ///
    /// The STATE-LEVEL twin of the walker method (7.2d decision: the
    /// walker's copy is hard-bound to its 69199 callback; the 7.4
    /// callers — instantiateSignatureInContextOf 75915 and the
    /// higher-order path 80788 — run OUTSIDE a walker and parameterize
    /// the callback as (inferences, priority, contravariant) instead).
    pub(crate) fn apply_to_parameter_types_with_inferences(
        &mut self,
        inferences: &[InferenceInfoId],
        source: SignatureId,
        target: SignatureId,
        priority: InferencePriority,
        contravariant: bool,
    ) -> CheckResult<()> {
        let source_count = self.get_parameter_count(source)?;
        let target_count = self.get_parameter_count(target)?;
        let source_rest_type = self.get_effective_rest_type(source)?;
        let target_rest_type = self.get_effective_rest_type(target)?;
        let target_non_rest_count = if target_rest_type.is_some() {
            target_count - 1
        } else {
            target_count
        };
        let param_count = if source_rest_type.is_some() {
            target_non_rest_count
        } else {
            source_count.min(target_non_rest_count)
        };
        if let Some(source_this_type) = self.get_this_type_of_signature(source)? {
            if let Some(target_this_type) = self.get_this_type_of_signature(target)? {
                self.infer_types(
                    inferences,
                    source_this_type,
                    target_this_type,
                    priority,
                    contravariant,
                )?;
            }
        }
        for i in 0..param_count {
            let source_type = self.get_type_at_position(source, i)?;
            let target_type = self.get_type_at_position(target, i)?;
            self.infer_types(
                inferences,
                source_type,
                target_type,
                priority,
                contravariant,
            )?;
        }
        if let Some(target_rest_type) = target_rest_type {
            // 68215-68221: readonly when the rest variable is const
            // and nothing in it is a mutable array shape.
            let readonly = self.is_const_type_variable(Some(target_rest_type), 0)? && {
                let members = if self
                    .tables
                    .flags_of(target_rest_type)
                    .intersects(TypeFlags::UNION)
                {
                    match &self.tables.type_of(target_rest_type).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        _ => vec![target_rest_type],
                    }
                } else {
                    vec![target_rest_type]
                };
                let mut some_mutable = false;
                for member in members {
                    if self.is_mutable_array_like_type(member)? {
                        some_mutable = true;
                        break;
                    }
                }
                !some_mutable
            };
            let source_rest = self.get_rest_type_at_position(source, param_count, readonly)?;
            self.infer_types(
                inferences,
                source_rest,
                target_rest_type,
                priority,
                contravariant,
            )?;
        }
        Ok(())
    }

    /// tsc-port: applyToReturnTypes @6.0.3
    /// tsc-hash: 1bb818de205cee65e351fad046065911d45a080a9f63dffde44b2a5b45e42edb
    /// tsc-span: _tsc.js:68224-68237
    ///
    /// State-level twin — see applyToParameterTypes above.
    pub(crate) fn apply_to_return_types_with_inferences(
        &mut self,
        inferences: &[InferenceInfoId],
        source: SignatureId,
        target: SignatureId,
        priority: InferencePriority,
    ) -> CheckResult<()> {
        if let Some(target_predicate) = self.get_type_predicate_of_signature(target)? {
            if let Some(source_predicate) = self.get_type_predicate_of_signature(source)? {
                if std::mem::discriminant(&source_predicate.kind)
                    == std::mem::discriminant(&target_predicate.kind)
                    && source_predicate.parameter_index == target_predicate.parameter_index
                {
                    if let (Some(source_type), Some(target_type)) =
                        (source_predicate.ty, target_predicate.ty)
                    {
                        self.infer_types(inferences, source_type, target_type, priority, false)?;
                        return Ok(());
                    }
                }
            }
        }
        let target_return_type = self.get_return_type_of_signature(target)?;
        if self.could_contain_type_variables(target_return_type) {
            let source_return_type = self.get_return_type_of_signature(source)?;
            self.infer_types(
                inferences,
                source_return_type,
                target_return_type,
                priority,
                false,
            )?;
        }
        Ok(())
    }

    /// tsc-port: hasOverlappingInferences @6.0.3
    /// tsc-hash: fc4f59c2527192297325fd8ea05ad66315c384dded4e7d01a4ddbaf30d9cfd64
    /// tsc-span: _tsc.js:80828-80835
    pub(crate) fn has_overlapping_inferences(
        &self,
        a: &[InferenceInfoId],
        b: &[InferenceInfoId],
    ) -> bool {
        for i in 0..a.len() {
            if has_inference_candidates(self.inference_info(a[i]))
                && has_inference_candidates(self.inference_info(b[i]))
            {
                return true;
            }
        }
        false
    }

    /// tsc-port: mergeInferences @6.0.3
    /// tsc-hash: 58ff430f5184937cd04bc91b54663733c9696afb72987a748affb5865b3c8aea
    /// tsc-span: _tsc.js:80836-80842
    ///
    /// The LIVE-slot id rewrite (7.1 identity model): the context's
    /// `inferences[i]` takes the detached row's id while the mapper
    /// capture keeps the creation-time infos — tsc's post-merge split
    /// (detached thunk stays fixed, fresh live row unfixed) holds by
    /// construction (pinned:
    /// fixing_dispatch_consults_creation_capture_after_slot_replacement).
    pub(crate) fn merge_inferences(
        &mut self,
        context: InferenceContextId,
        source: &[InferenceInfoId],
    ) {
        for (i, &source_slot) in source.iter().enumerate() {
            let target_slot = self.inference_context(context).inferences[i];
            if !has_inference_candidates(self.inference_info(target_slot))
                && has_inference_candidates(self.inference_info(source_slot))
            {
                self.inference_context_mut(context).inferences[i] = source_slot;
            }
        }
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
    FromGenericMappedTypes,
    FromObjectTypes,
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
    fn matches_pair(&mut self, s: TypeId, t: TypeId, matcher: TypeMatcher) -> CheckResult<bool> {
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
    /// each a named escape below until its commit. General
    /// Substitution sources infer first from their base, then from the
    /// base/constraint intersection at SubstituteSource priority.
    ///
    /// Load-bearing shape notes:
    /// - The TypeVariable block (68701-68769) returns ONLY when an
    ///   inference slot matched; the simplification fallback falls
    ///   through into the 68770 chain (e.g. an indexed-access pair
    ///   reaches the pairwise arm after failing to simplify).
    /// - Arms 5/6 (union/intersection reduction) rewrite source and
    ///   target in place before every later arm reads them.
    fn infer_from_types(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let mut source = source;
        let mut target = target;
        if !self.st.could_contain_type_variables(target) || self.st.tables.is_no_infer_type(target)
        {
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
                    // A None fill (min_params > 0 with an absent
                    // target list) is the shape tsc TypeErrors on one
                    // call later (68877 targetTypes.length) —
                    // unreachable: bare generic-alias references
                    // become errorType and carry no aliasSymbol, so
                    // the expect is crash-equivalent transcription,
                    // not a deviation.
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
                        && !self.st.is_generic_mapped_type_state(member)?;
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
            if self.st.tables.is_no_infer_type(target) {
                return Ok(());
            }
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
            // 68740-68769: no slot — try simplifying, then ALWAYS
            // fall through to the 68770 chain with the original pair
            // (tsc runs the terminal arms even after a successful
            // simplified recursion — do not add an early return).
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
            let TypeData::Substitution(data) = self.st.tables.type_of(source).data.clone() else {
                unreachable!("Substitution flag implies data");
            };
            self.infer_from_types(data.base_type, target)?;
            let intersection = self.st.get_substitution_intersection(source)?;
            self.infer_with_priority(intersection, target, InferencePriority::SUBSTITUTE_SOURCE)?;
        } else if target_flags.intersects(TypeFlags::CONDITIONAL) {
            // 68786: routed through invokeOnce; conditional inference
            // is live in infer_to_conditional_type.
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
            // 68796-68797.
            self.infer_to_template_literal_type(source, target)?;
        } else {
            // 68798-68813: the reduced/apparent object tail.
            source = self.st.get_reduced_type(source)?;
            if self.st.is_generic_mapped_type_state(source)?
                && self.st.is_generic_mapped_type_state(target)?
            {
                self.invoke_once(source, target, InferAction::FromGenericMappedTypes)?;
            }
            if !(self.priority.intersects(InferencePriority::NO_CONSTRAINTS)
                && self
                    .st
                    .tables
                    .flags_of(source)
                    .intersects(TypeFlags::from_bits(
                        TypeFlags::INTERSECTION.bits() | TypeFlags::INSTANTIABLE.bits(),
                    )))
            {
                let apparent_source = self.st.get_apparent_type(source)?;
                if apparent_source != source
                    && !self
                        .st
                        .tables
                        .flags_of(apparent_source)
                        .intersects(TypeFlags::from_bits(
                            TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
                        ))
                {
                    return self.infer_from_types(apparent_source, target);
                }
                source = apparent_source;
            }
            if self
                .st
                .tables
                .flags_of(source)
                .intersects(TypeFlags::from_bits(
                    TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
                ))
            {
                self.invoke_once(source, target, InferAction::FromObjectTypes)?;
            }
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
    ) -> CheckResult<()> {
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
    ) -> CheckResult<()> {
        let save_priority = self.priority;
        self.priority |= new_priority;
        let result = self.infer_from_contravariant_types(source, target);
        self.priority = save_priority;
        result
    }

    /// tsc-port: inferToMultipleTypesWithPriority @6.0.3
    /// tsc-hash: 4991227f792f48ec39032a10a770ce401747830dfff0986f12a0a54aac743480
    /// tsc-span: _tsc.js:68827-68832
    fn infer_to_multiple_types_with_priority(
        &mut self,
        source: TypeId,
        targets: &[TypeId],
        target_flags: TypeFlags,
        new_priority: InferencePriority,
    ) -> CheckResult<()> {
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
    ) -> CheckResult<()> {
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
                InferAction::FromGenericMappedTypes => {
                    self.infer_from_generic_mapped_types(source, target)?
                }
                InferAction::FromObjectTypes => self.infer_from_object_types(source, target)?,
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
    ) -> CheckResult<(Vec<TypeId>, Vec<TypeId>)> {
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
    ) -> CheckResult<()> {
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
    ) -> CheckResult<()> {
        self.contravariant = !self.contravariant;
        let result = self.infer_from_types(source, target);
        self.contravariant = !self.contravariant;
        result
    }

    /// tsc-port: inferFromContravariantTypesIfStrictFunctionTypes @6.0.3
    /// tsc-hash: 24e682796a93c65a3c9beb1b2c5f181afab6e56e062bdbe9b3db6d5c64365e9e
    /// tsc-span: _tsc.js:68891-68897
    fn infer_from_contravariant_types_if_strict_function_types(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<()> {
        if self.st.strict_function_types
            || self.priority.intersects(InferencePriority::ALWAYS_STRICT)
        {
            self.infer_from_contravariant_types(source, target)
        } else {
            self.infer_from_types(source, target)
        }
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
    ) -> CheckResult<()> {
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
    fn infer_to_conditional_type(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let TypeData::Conditional(target_data) = self.st.tables.type_of(target).data.clone() else {
            unreachable!("Conditional target flag implies data");
        };
        if self
            .st
            .tables
            .flags_of(source)
            .intersects(TypeFlags::CONDITIONAL)
        {
            let TypeData::Conditional(source_data) = self.st.tables.type_of(source).data.clone()
            else {
                unreachable!("Conditional source flag implies data");
            };
            self.infer_from_types(source_data.check_type, target_data.check_type)?;
            self.infer_from_types(source_data.extends_type, target_data.extends_type)?;
            let source_true = self.st.get_true_type_from_conditional_type(source)?;
            let target_true = self.st.get_true_type_from_conditional_type(target)?;
            self.infer_from_types(source_true, target_true)?;
            let source_false = self.st.get_false_type_from_conditional_type(source)?;
            let target_false = self.st.get_false_type_from_conditional_type(target)?;
            self.infer_from_types(source_false, target_false)
        } else {
            let targets = [
                self.st.get_true_type_from_conditional_type(target)?,
                self.st.get_false_type_from_conditional_type(target)?,
            ];
            let priority = if self.contravariant {
                InferencePriority::CONTRAVARIANT_CONDITIONAL
            } else {
                InferencePriority::NONE
            };
            self.infer_to_multiple_types_with_priority(
                source,
                &targets,
                self.st.tables.flags_of(target),
                priority,
            )
        }
    }

    /// tsc-port: inferToTemplateLiteralType @6.0.3
    /// tsc-hash: 61f1cdc14dd0966d118f1069e70acc8930804fc7510291af95b4a6b11ce84e81
    /// tsc-span: _tsc.js:69022-69060
    ///
    /// Placeholder-wise inference from the inferTypesFromTemplateLiteralType
    /// match list (never-filled when unmatched but the target is all
    /// placeholders). A string-literal match against a type-variable
    /// placeholder consults the variable's base constraint and infers
    /// the COERCED form (number/bigint/boolean/enum literal, or the
    /// constraint member itself) when exactly the constraint admits it
    /// — the 69051 reduceLeft fold, split out as
    /// `template_constraint_match`.
    fn infer_to_template_literal_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<()> {
        let matches = self
            .st
            .infer_types_from_template_literal_type(source, target)?;
        let TypeData::TemplateLiteral { texts, types } =
            self.st.tables.type_of(target).data.clone()
        else {
            unreachable!("TemplateLiteral flag implies data");
        };
        if matches.is_none() && !texts.iter().all(|s| s.is_empty()) {
            return Ok(());
        }
        for (i, &target2) in types.iter().enumerate() {
            let source2 = match &matches {
                Some(matches) => matches[i],
                None => self.st.tables.intrinsics.never,
            };
            if self
                .st
                .tables
                .flags_of(source2)
                .intersects(TypeFlags::STRING_LITERAL)
                && self
                    .st
                    .tables
                    .flags_of(target2)
                    .intersects(TypeFlags::TYPE_VARIABLE)
            {
                // 69030-69031: the constraint comes from the matched
                // inference slot's type parameter.
                let constraint = match self.get_inference_info_for_type(target2) {
                    Some(info_id) => {
                        let type_parameter = self.st.inference_info(info_id).type_parameter;
                        self.st.get_base_constraint_of_type(type_parameter)?
                    }
                    None => None,
                };
                if let Some(constraint) = constraint {
                    if !self
                        .st
                        .tables
                        .flags_of(constraint)
                        .intersects(TypeFlags::ANY)
                    {
                        let constraint_types = if self
                            .st
                            .tables
                            .flags_of(constraint)
                            .intersects(TypeFlags::UNION)
                        {
                            self.types_of(constraint)
                        } else {
                            vec![constraint]
                        };
                        let mut all_type_flags = TypeFlags::from_bits(0);
                        for &t in &constraint_types {
                            all_type_flags = TypeFlags::from_bits(
                                all_type_flags.bits() | self.st.tables.flags_of(t).bits(),
                            );
                        }
                        if !all_type_flags.intersects(TypeFlags::STRING) {
                            let TypeData::Literal {
                                value: LiteralValue::String(str_value),
                            } = self.st.tables.type_of(source2).data.clone()
                            else {
                                unreachable!("StringLiteral flag implies string data");
                            };
                            let utf8_value = str_value.to_utf8();
                            // 69038-69050: coercion families the string
                            // can never round-trip through drop out.
                            if all_type_flags.intersects(TypeFlags::NUMBER_LIKE)
                                && !utf8_value.as_deref().is_some_and(|value| {
                                    self.st.is_valid_number_string(value, true)
                                })
                            {
                                all_type_flags = TypeFlags::from_bits(
                                    all_type_flags.bits() & !TypeFlags::NUMBER_LIKE.bits(),
                                );
                            }
                            if all_type_flags.intersects(TypeFlags::BIG_INT_LIKE)
                                && !utf8_value.as_deref().is_some_and(|value| {
                                    self.st.is_valid_big_int_string(value, true)
                                })
                            {
                                all_type_flags = TypeFlags::from_bits(
                                    all_type_flags.bits() & !TypeFlags::BIG_INT_LIKE.bits(),
                                );
                            }
                            let mut matching_type = self.st.tables.intrinsics.never;
                            for &right in &constraint_types {
                                matching_type = self.template_constraint_match(
                                    matching_type,
                                    right,
                                    source2,
                                    &str_value,
                                    all_type_flags,
                                )?;
                            }
                            if !self
                                .st
                                .tables
                                .flags_of(matching_type)
                                .intersects(TypeFlags::NEVER)
                            {
                                self.infer_from_types(matching_type, target2)?;
                                continue;
                            }
                        }
                    }
                }
            }
            self.infer_from_types(source2, target2)?;
        }
        Ok(())
    }

    /// The 69051 reduceLeft step: `left` is the best match so far
    /// (never = none), `right` the next constraint member. Each
    /// left-check keeps an earlier win of a MORE-preferred family;
    /// each right-check admits `right`'s family if the literal text
    /// round-trips into it. Written as the same ordered chain.
    fn template_constraint_match(
        &mut self,
        left: TypeId,
        right: TypeId,
        source: TypeId,
        str_value: &tsc_types::TemplateText,
        all_type_flags: TypeFlags,
    ) -> CheckResult<TypeId> {
        if !self.st.tables.flags_of(right).intersects(all_type_flags) {
            return Ok(left);
        }
        let utf8_value = str_value.to_utf8();
        let left_flags = self.st.tables.flags_of(left);
        let right_flags = self.st.tables.flags_of(right);
        if left_flags.intersects(TypeFlags::STRING) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::STRING) {
            Ok(source)
        } else if left_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::TEMPLATE_LITERAL)
            && self
                .st
                .is_type_matched_by_template_literal_type(source, right)?
        {
            Ok(source)
        } else if left_flags.intersects(TypeFlags::STRING_MAPPING) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::STRING_MAPPING) && {
            let symbol = self
                .st
                .tables
                .type_of(right)
                .symbol
                .expect("StringMapping carries its intrinsic alias symbol");
            let name = self.st.binder.symbol(symbol).escaped_name.clone();
            str_value
                == &crate::instantiate::apply_string_mapping(
                    crate::instantiate::intrinsic_type_kind(&name),
                    str_value,
                )
        } {
            Ok(source)
        } else if left_flags.intersects(TypeFlags::STRING_LITERAL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::STRING_LITERAL)
            && matches!(
                &self.st.tables.type_of(right).data,
                TypeData::Literal { value: LiteralValue::String(v) } if v == str_value
            )
        {
            Ok(right)
        } else if left_flags.intersects(TypeFlags::NUMBER) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::NUMBER) {
            Ok(utf8_value
                .as_deref()
                .map_or(left, |value| self.coerced_number_literal(value)))
        } else if left_flags.intersects(TypeFlags::ENUM) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::ENUM) {
            Ok(utf8_value
                .as_deref()
                .map_or(left, |value| self.coerced_number_literal(value)))
        } else if left_flags.intersects(TypeFlags::NUMBER_LITERAL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::NUMBER_LITERAL)
            && matches!(
                &self.st.tables.type_of(right).data,
                TypeData::Literal { value: LiteralValue::Number(v) }
                    if utf8_value.as_deref().and_then(crate::structural::js_string_to_number)
                        == Some(*v)
            )
        {
            Ok(right)
        } else if left_flags.intersects(TypeFlags::BIG_INT) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::BIG_INT) {
            match utf8_value.as_deref() {
                Some(value) => self.st.parse_big_int_literal_type(value),
                None => Ok(left),
            }
        } else if left_flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::BIG_INT_LITERAL)
            && matches!(
                &self.st.tables.type_of(right).data,
                TypeData::Literal { value: LiteralValue::BigInt(v) }
                    if utf8_value
                        .as_deref()
                        .is_some_and(|value| v.to_base10_string() == value)
            )
        {
            Ok(right)
        } else if left_flags.intersects(TypeFlags::BOOLEAN) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::BOOLEAN) {
            Ok(if str_value.eq_utf8("true") {
                self.st.tables.intrinsics.true_fresh
            } else if str_value.eq_utf8("false") {
                self.st.tables.intrinsics.false_fresh
            } else {
                self.st.tables.intrinsics.boolean
            })
        } else if left_flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::BOOLEAN_LITERAL)
            && self
                .intrinsic_name_of(right)
                .is_some_and(|name| str_value.eq_utf8(name))
        {
            Ok(right)
        } else if left_flags.intersects(TypeFlags::UNDEFINED) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::UNDEFINED)
            && self
                .intrinsic_name_of(right)
                .is_some_and(|name| str_value.eq_utf8(name))
        {
            Ok(right)
        } else if left_flags.intersects(TypeFlags::NULL) {
            Ok(left)
        } else if right_flags.intersects(TypeFlags::NULL)
            && self
                .intrinsic_name_of(right)
                .is_some_and(|name| str_value.eq_utf8(name))
        {
            Ok(right)
        } else {
            Ok(left)
        }
    }

    /// tsc `getNumberLiteralType(+str)` (69051): the NumberLike gate
    /// upstream only survives round-trip-valid strings, so the
    /// coercion cannot miss.
    fn coerced_number_literal(&mut self, str_value: &str) -> TypeId {
        let n = crate::structural::js_string_to_number(str_value)
            .expect("NumberLike survives only for round-trip-valid strings (69041)");
        self.st.tables.get_number_literal_type(n)
    }

    /// tsc `type.intrinsicName` reads on constraint members — None for
    /// non-intrinsic data, exactly as the property is undefined there.
    fn intrinsic_name_of(&self, ty: TypeId) -> Option<&str> {
        match &self.st.tables.type_of(ty).data {
            TypeData::Intrinsic { name, .. } => Some(name),
            _ => None,
        }
    }

    /// tsc-port: inferFromGenericMappedTypes @6.0.3
    /// tsc-hash: 43d46bba590ef8ed07cccef71609b0ec8b215d1343ac158b54caac6075f39580
    /// tsc-span: _tsc.js:69063-69069
    fn infer_from_generic_mapped_types(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<()> {
        let source_constraint = self.st.get_constraint_type_from_mapped_type(source)?;
        let target_constraint = self.st.get_constraint_type_from_mapped_type(target)?;
        self.infer_from_types(source_constraint, target_constraint)?;

        let source_template = self.st.get_template_type_from_mapped_type(source)?;
        let target_template = self.st.get_template_type_from_mapped_type(target)?;
        self.infer_from_types(source_template, target_template)?;

        let source_name = self.st.get_name_type_from_mapped_type(source)?;
        let target_name = self.st.get_name_type_from_mapped_type(target)?;
        if let (Some(source_name), Some(target_name)) = (source_name, target_name) {
            self.infer_from_types(source_name, target_name)?;
        }
        Ok(())
    }

    /// tsc-port: inferToMappedType @6.0.3
    /// tsc-hash: 011d195d72a9618a2fa383e72666fac77e894391307b119b5b7028d07eddab38
    /// tsc-span: _tsc.js:68972-69010
    fn infer_to_mapped_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        constraint_type: TypeId,
    ) -> CheckResult<bool> {
        let constraint_flags = self.st.tables.flags_of(constraint_type);
        if constraint_flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let mut result = false;
            for constraint in self.types_of(constraint_type) {
                result = self.infer_to_mapped_type(source, target, constraint)? || result;
            }
            return Ok(result);
        }
        if constraint_flags.intersects(TypeFlags::INDEX) {
            let TypeData::Index {
                ty: constraint_inner,
                ..
            } = self.st.tables.type_of(constraint_type).data
            else {
                unreachable!("index flag implies index data");
            };
            if let Some(inference) = self.get_inference_info_for_type(constraint_inner) {
                let is_fixed = self.st.inference_info(inference).is_fixed;
                if !is_fixed && !self.st.is_from_inference_blocked_source(source) {
                    if let Some(inferred) = self.st.infer_type_for_homomorphic_mapped_type(
                        source,
                        target,
                        constraint_type,
                    )? {
                        let priority = if self
                            .st
                            .tables
                            .object_flags_of(source)
                            .intersects(ObjectFlags::NON_INFERRABLE_TYPE)
                        {
                            InferencePriority::PARTIAL_HOMOMORPHIC_MAPPED_TYPE
                        } else {
                            InferencePriority::HOMOMORPHIC_MAPPED_TYPE
                        };
                        let parameter = self.st.inference_info(inference).type_parameter;
                        self.infer_with_priority(inferred, parameter, priority)?;
                    }
                }
            }
            return Ok(true);
        }
        if constraint_flags.intersects(TypeFlags::TYPE_PARAMETER) {
            let index_flags = if self.st.links.ty(source).pattern.is_some() {
                tsc_types::IndexFlags::NO_INDEX_SIGNATURES
            } else {
                tsc_types::IndexFlags::NONE
            };
            let source_index = self.st.get_index_type(source, index_flags)?;
            self.infer_with_priority(
                source_index,
                constraint_type,
                InferencePriority::MAPPED_TYPE_CONSTRAINT,
            )?;
            if let Some(extended) = self.st.get_constraint_of_type(constraint_type)? {
                if self.infer_to_mapped_type(source, target, extended)? {
                    return Ok(true);
                }
            }

            let mut source_types = Vec::new();
            for property in self.st.get_properties_of_type(source)? {
                source_types.push(self.st.get_type_of_symbol(property)?);
            }
            for info in self.st.get_index_infos_of_type(source)? {
                source_types.push(if info.is_enum_number_index_info {
                    self.st.tables.intrinsics.never
                } else {
                    info.value_type
                });
            }
            let union = self
                .st
                .get_union_type_ex(&source_types, UnionReduction::Literal)?;
            let template = self.st.get_template_type_from_mapped_type(target)?;
            self.infer_from_types(union, template)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: inferFromObjectTypes @6.0.3
    /// tsc-hash: 18d2ca4512c8f94287f9b7288ff06e8448caed9b1f63d82b4ebc19c37538e36d
    /// tsc-span: _tsc.js:69070-69169
    ///
    /// The structural body behind the object-tail invokeOnce: matching
    /// references infer pairwise; the Mapped-target block is an M8
    /// escape (inferToMappedType); the tuple ladder ports the JS slice-bound
    /// clamps observed by probe (probe-tuple.mjs, 2026-07-20 — incl.
    /// the recorded tsc-crash deviation on an undefined middle-slice
    /// source meeting a type-variable rest target, m8-readiness row 4).
    fn infer_from_object_types(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let source_object_flags = self.st.tables.object_flags_of(source);
        let target_object_flags = self.st.tables.object_flags_of(target);
        if source_object_flags.intersects(ObjectFlags::REFERENCE)
            && target_object_flags.intersects(ObjectFlags::REFERENCE)
            && (self.st.tables.reference_target(source) == self.st.tables.reference_target(target)
                || self.st.is_array_type(source)? && self.st.is_array_type(target)?)
        {
            // 69071-69074: pairwise under the target's variances (no
            // deferred-node exclusion on this inner path).
            let source_arguments = self.st.get_type_arguments(source)?;
            let target_arguments = self.st.get_type_arguments(target)?;
            let reference_target = self.st.tables.reference_target(source);
            let variances = match self.st.get_variances(reference_target)? {
                VariancesResult::Known(variances) => variances,
                VariancesResult::InProgress => Box::default(),
            };
            self.infer_from_type_arguments(&source_arguments, &target_arguments, &variances)?;
            return Ok(());
        }
        if self.st.is_generic_mapped_type_state(source)?
            && self.st.is_generic_mapped_type_state(target)?
        {
            self.infer_from_generic_mapped_types(source, target)?;
        }
        if target_object_flags.intersects(ObjectFlags::MAPPED)
            && !self.st.mapped_type_declaration_has_name_type(target)
        {
            let constraint = self.st.get_constraint_type_from_mapped_type(target)?;
            if self.infer_to_mapped_type(source, target, constraint)? {
                return Ok(());
            }
        }
        if !self.st.types_definitely_unrelated(source, target)? {
            if self.st.is_array_type(source)? || self.st.tables.is_tuple_type(source) {
                if self.st.tables.is_tuple_type(target) {
                    self.infer_to_tuple_target(source, target)?;
                    return Ok(());
                }
                if self.st.is_array_type(target)? {
                    self.infer_from_index_types(source, target)?;
                    return Ok(());
                }
            }
            self.infer_from_properties(source, target)?;
            self.infer_from_signatures(source, target, SignatureKind::Call)?;
            self.infer_from_signatures(source, target, SignatureKind::Construct)?;
            self.infer_from_index_types(source, target)?;
        }
        Ok(())
    }

    /// The 69089-69158 tuple-target ladder, split from
    /// `infer_from_object_types` for the early returns. Slice bounds
    /// follow JS Array.prototype.slice clamping (see slice_tuple_type
    /// and js_slice_bounds).
    #[allow(clippy::needless_range_loop)] // index walk over paired lists, ported as tsc wrote it
    fn infer_to_tuple_target(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let source_is_tuple = self.st.tables.is_tuple_type(source);
        let source_arity = self.st.get_type_reference_arity(source);
        let target_arity = self.st.get_type_reference_arity(target);
        let element_types = self.st.get_type_arguments(target)?;
        let target_target = self.st.tables.reference_target(target);
        let TypeData::TupleTarget(target_data) = self.st.tables.type_of(target_target).data.clone()
        else {
            unreachable!("tuple type targets a tuple target");
        };
        let element_flags = target_data.element_flags.clone();
        if source_is_tuple && self.st.is_tuple_type_structure_matching(source, target) {
            // 69094-69098: same shape — element-wise.
            for i in 0..target_arity {
                let source_element = self.st.get_type_arguments(source)?[i];
                self.infer_from_types(source_element, element_types[i])?;
            }
            return Ok(());
        }
        let (source_fixed_length, source_end_count, source_element_flags) = if source_is_tuple {
            let source_target = self.st.tables.reference_target(source);
            let TypeData::TupleTarget(source_data) =
                self.st.tables.type_of(source_target).data.clone()
            else {
                unreachable!("tuple type targets a tuple target");
            };
            (
                source_data.fixed_length,
                crate::structural::end_element_count(
                    &source_data.element_flags,
                    ElementFlags::FIXED,
                ),
                source_data.element_flags.to_vec(),
            )
        } else {
            (0, 0, Vec::new())
        };
        let start_length = if source_is_tuple {
            source_fixed_length.min(target_data.fixed_length)
        } else {
            0
        };
        let end_length = source_end_count.min(
            if target_data
                .combined_flags
                .intersects(ElementFlags::VARIABLE)
            {
                crate::structural::end_element_count(&element_flags, ElementFlags::FIXED)
            } else {
                0
            },
        );
        for i in 0..start_length {
            let source_element = self.st.get_type_arguments(source)?[i];
            self.infer_from_types(source_element, element_types[i])?;
        }
        if !source_is_tuple
            || source_arity as i64 - start_length as i64 - end_length as i64 == 1
                && source_element_flags[start_length].intersects(ElementFlags::REST)
        {
            // 69105-69109: a single rest (or plain array element)
            // distributes over the target's middle.
            let rest_type = self.st.get_type_arguments(source)?[start_length];
            for i in start_length..target_arity - end_length {
                let source2 = if element_flags[i].intersects(ElementFlags::VARIADIC) {
                    self.st.create_array_type(rest_type, false)?
                } else {
                    rest_type
                };
                self.infer_from_types(source2, element_types[i])?;
            }
        } else {
            let middle_length = target_arity - start_length - end_length;
            if middle_length == 2 {
                if element_flags[start_length].intersects(ElementFlags::VARIADIC)
                    && element_flags[start_length + 1].intersects(ElementFlags::VARIADIC)
                {
                    // 69111-69116: both variadic — gated on the 7.4
                    // impliedArity record (None until then).
                    if let Some(info_id) =
                        self.get_inference_info_for_type(element_types[start_length])
                    {
                        if let Some(implied_arity) = self.st.inference_info(info_id).implied_arity {
                            let first = self.st.slice_tuple_type(
                                source,
                                start_length,
                                (end_length + source_arity).saturating_sub(implied_arity),
                            )?;
                            self.infer_from_types(first, element_types[start_length])?;
                            let second = self.st.slice_tuple_type(
                                source,
                                start_length + implied_arity,
                                end_length,
                            )?;
                            self.infer_from_types(second, element_types[start_length + 1])?;
                        }
                    }
                } else if element_flags[start_length].intersects(ElementFlags::VARIADIC)
                    && element_flags[start_length + 1].intersects(ElementFlags::REST)
                {
                    // 69117-69123: variadic then rest — the variable's
                    // fixed tuple constraint implies the split arity.
                    let implied = self.middle_implied_arity(element_types[start_length])?;
                    if let Some(implied_arity) = implied {
                        let first = self.st.slice_tuple_type(
                            source,
                            start_length,
                            source_arity.saturating_sub(start_length + implied_arity),
                        )?;
                        self.infer_from_types(first, element_types[start_length])?;
                        let second = self.st.get_element_type_of_slice_of_tuple_type(
                            source,
                            start_length + implied_arity,
                            end_length,
                            /*writing*/ false,
                            /*no_reductions*/ false,
                        )?;
                        if !self.infer_from_middle_slice(second, element_types[start_length + 1])? {
                            return Ok(());
                        }
                    }
                } else if element_flags[start_length].intersects(ElementFlags::REST)
                    && element_flags[start_length + 1].intersects(ElementFlags::VARIADIC)
                {
                    // 69124-69139: rest then variadic — trailing slice
                    // bounds carry JS negative-index semantics.
                    let implied = self.middle_implied_arity(element_types[start_length + 1])?;
                    if let Some(implied_arity) = implied {
                        let end_index = source_arity as i64
                            - crate::structural::end_element_count(
                                &element_flags,
                                ElementFlags::FIXED,
                            ) as i64;
                        let start_index = end_index - implied_arity as i64;
                        let source_arguments = self.st.get_type_arguments(source)?;
                        let source_target = self.st.tables.reference_target(source);
                        let TypeData::TupleTarget(source_data) =
                            self.st.tables.type_of(source_target).data.clone()
                        else {
                            unreachable!("tuple type targets a tuple target");
                        };
                        let (from, to) = js_slice_bounds(source_arity, start_index, end_index);
                        let labels = source_data
                            .labeled_element_declarations
                            .as_ref()
                            .map(|declarations| declarations[from..to].to_vec());
                        let trailing_slice = self.st.create_tuple_type_forced(
                            &source_arguments[from..to],
                            Some(&source_data.element_flags[from..to]),
                            /*readonly*/ false,
                            labels.as_deref(),
                        )?;
                        let first = self.st.get_element_type_of_slice_of_tuple_type(
                            source,
                            start_length,
                            end_length + implied_arity,
                            /*writing*/ false,
                            /*no_reductions*/ false,
                        )?;
                        if !self.infer_from_middle_slice(first, element_types[start_length])? {
                            return Ok(());
                        }
                        self.infer_from_types(trailing_slice, element_types[start_length + 1])?;
                    }
                }
            } else if middle_length == 1
                && element_flags[start_length].intersects(ElementFlags::VARIADIC)
            {
                // 69140-69144: single variadic — SpeculativeTuple when
                // the target ends optional.
                let ends_in_optional =
                    target_data.element_flags[target_arity - 1].intersects(ElementFlags::OPTIONAL);
                let source_slice = self.st.slice_tuple_type(source, start_length, end_length)?;
                self.infer_with_priority(
                    source_slice,
                    element_types[start_length],
                    if ends_in_optional {
                        InferencePriority::SPECULATIVE_TUPLE
                    } else {
                        InferencePriority::NONE
                    },
                )?;
            } else if middle_length == 1
                && element_flags[start_length].intersects(ElementFlags::REST)
            {
                // 69145-69150.
                let rest_type = self.st.get_element_type_of_slice_of_tuple_type(
                    source,
                    start_length,
                    end_length,
                    /*writing*/ false,
                    /*no_reductions*/ false,
                )?;
                if let Some(rest_type) = rest_type {
                    self.infer_from_types(rest_type, element_types[start_length])?;
                }
            }
        }
        // 69153-69156: trailing fixed elements pair from the ends.
        for i in 0..end_length {
            let source_element = self.st.get_type_arguments(source)?[source_arity - i - 1];
            self.infer_from_types(source_element, element_types[target_arity - i - 1])?;
        }
        Ok(())
    }

    /// The shared variadic/rest middle-arm gate (69118-69120,
    /// 69125-69127): the adjacent type variable's base constraint must
    /// be a fixed-arity tuple; its fixedLength is the implied split.
    fn middle_implied_arity(&mut self, element_type: TypeId) -> CheckResult<Option<usize>> {
        let Some(info_id) = self.get_inference_info_for_type(element_type) else {
            return Ok(None);
        };
        let type_parameter = self.st.inference_info(info_id).type_parameter;
        let Some(constraint) = self.st.get_base_constraint_of_type(type_parameter)? else {
            return Ok(None);
        };
        if !self.st.tables.is_tuple_type(constraint) {
            return Ok(None);
        }
        let constraint_target = self.st.tables.reference_target(constraint);
        let TypeData::TupleTarget(data) = &self.st.tables.type_of(constraint_target).data else {
            unreachable!("tuple type targets a tuple target");
        };
        if data.combined_flags.intersects(ElementFlags::VARIABLE) {
            return Ok(None);
        }
        Ok(Some(data.fixed_length))
    }

    /// tsc passes an undefined middle slice straight into
    /// inferFromTypes, which survives ONLY on the 68647 head guard —
    /// `!couldContainTypeVariables(target) || isNoInferType(target)`
    /// — and TypeErrors otherwise: the recorded tsc-crash deviation
    /// (m8-readiness row 4, probe-tuple.mjs f6). The port preserves
    /// the harmless guard and finitely contains the crash shape at
    /// this call boundary: `false` stops the remaining tuple-target
    /// ladder without adding a candidate at or after the crash point.
    fn infer_from_middle_slice(
        &mut self,
        source: Option<TypeId>,
        target: TypeId,
    ) -> CheckResult<bool> {
        match source {
            Some(source) => {
                self.infer_from_types(source, target)?;
                Ok(true)
            }
            None => {
                if self.st.could_contain_type_variables(target)
                    && !self.st.tables.is_no_infer_type(target)
                {
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
        }
    }

    /// tsc-port: inferFromProperties @6.0.3
    /// tsc-hash: d7581e09cb3cca44f643758bd3a498be544bb10fb35e640192072263dab91a38
    /// tsc-span: _tsc.js:69170-69181
    fn infer_from_properties(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let properties = self.st.get_properties_of_object_type_owned(target)?;
        for target_prop in properties {
            let name = self.st.binder.symbol(target_prop).escaped_name.clone();
            let Some(source_prop) = self.st.get_property_of_type_full(source, &name)? else {
                continue;
            };
            // 69174: hasSkipDirectInferenceFlag over the declarations
            // (constant false — services-only writer, see 7.2a).
            let skipped = self
                .st
                .binder
                .symbol(source_prop)
                .declarations
                .clone()
                .into_iter()
                .any(|declaration| self.st.has_skip_direct_inference_flag(declaration));
            if skipped {
                continue;
            }
            let source_type = self.st.get_type_of_symbol(source_prop)?;
            let source_optional = self
                .st
                .symbol_flags(source_prop)
                .intersects(SymbolFlags::OPTIONAL);
            let source2 = self.st.remove_missing_type(source_type, source_optional);
            let target_type = self.st.get_type_of_symbol(target_prop)?;
            let target_optional = self
                .st
                .symbol_flags(target_prop)
                .intersects(SymbolFlags::OPTIONAL);
            let target2 = self.st.remove_missing_type(target_type, target_optional);
            self.infer_from_types(source2, target2)?;
        }
        Ok(())
    }

    /// tsc-port: inferFromSignatures @6.0.3
    /// tsc-hash: 3c9c197b8995762d52d08006185b593a5586f6b203cd785322eb6bb2f81a455a
    /// tsc-span: _tsc.js:69182-69193
    fn infer_from_signatures(
        &mut self,
        source: TypeId,
        target: TypeId,
        kind: SignatureKind,
    ) -> CheckResult<()> {
        let source_signatures = self.st.get_signatures_of_type(source, kind)?;
        let source_len = source_signatures.len();
        if source_len == 0 {
            return Ok(());
        }
        let target_signatures = self.st.get_signatures_of_type(target, kind)?;
        for (i, &target_signature) in target_signatures.iter().enumerate() {
            // 69188 Math.max(sourceLen - targetLen + i, 0).
            let source_index = (source_len + i).saturating_sub(target_signatures.len());
            let base = self
                .st
                .get_base_signature(source_signatures[source_index])?;
            let erased = self.st.get_erased_signature(target_signature)?;
            self.infer_from_signature(base, erased)?;
        }
        Ok(())
    }

    /// tsc-port: inferFromSignature @6.0.3
    /// tsc-hash: 145838046d69f50e8e077249a2a427fac62c7b46cac158e148f8eeb4653d3caf
    /// tsc-span: _tsc.js:69194-69203
    fn infer_from_signature(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult<()> {
        if !self
            .st
            .signature_of(source)
            .flags
            .intersects(SignatureFlags::IS_NON_INFERRABLE)
        {
            let save_bivariant = self.bivariant;
            let kind = self
                .st
                .signature_of(target)
                .declaration
                .map(|declaration| self.st.kind_of(declaration));
            // 69198: method/constructor targets infer bivariantly.
            self.bivariant = self.bivariant
                || matches!(
                    kind,
                    Some(SyntaxKind::MethodDeclaration)
                        | Some(SyntaxKind::MethodSignature)
                        | Some(SyntaxKind::Constructor)
                );
            let result = self.apply_to_parameter_types(source, target);
            self.bivariant = save_bivariant;
            result?;
        }
        self.apply_to_return_types(source, target)
    }

    /// tsc-port: applyToParameterTypes @6.0.3
    /// tsc-hash: 95daf9e8bfe59dd9deb8b5b454837cd07bf3d0d1a3119f554a2d6e316135be3c
    /// tsc-span: _tsc.js:68198-68223
    ///
    /// The callback is hard-bound to
    /// inferFromContravariantTypesIfStrictFunctionTypes — the only
    /// caller inside the walker (69199). 7.4's
    /// instantiateSignatureInContextOf caller runs OUTSIDE the walker
    /// and gets its own state-level application then.
    fn apply_to_parameter_types(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult<()> {
        let source_count = self.st.get_parameter_count(source)?;
        let target_count = self.st.get_parameter_count(target)?;
        let source_rest_type = self.st.get_effective_rest_type(source)?;
        let target_rest_type = self.st.get_effective_rest_type(target)?;
        let target_non_rest_count = if target_rest_type.is_some() {
            target_count - 1
        } else {
            target_count
        };
        let param_count = if source_rest_type.is_some() {
            target_non_rest_count
        } else {
            source_count.min(target_non_rest_count)
        };
        if let Some(source_this_type) = self.st.get_this_type_of_signature(source)? {
            if let Some(target_this_type) = self.st.get_this_type_of_signature(target)? {
                self.infer_from_contravariant_types_if_strict_function_types(
                    source_this_type,
                    target_this_type,
                )?;
            }
        }
        for i in 0..param_count {
            let source_type = self.st.get_type_at_position(source, i)?;
            let target_type = self.st.get_type_at_position(target, i)?;
            self.infer_from_contravariant_types_if_strict_function_types(source_type, target_type)?;
        }
        if let Some(target_rest_type) = target_rest_type {
            // 68215-68221: readonly when the rest variable is const
            // and nothing in it is a mutable array shape (someType
            // expanded — the port predicate is fallible).
            let mut some_mutable = false;
            if self.st.is_const_type_variable(Some(target_rest_type), 0)? {
                let members = if self
                    .st
                    .tables
                    .flags_of(target_rest_type)
                    .intersects(TypeFlags::UNION)
                {
                    self.types_of(target_rest_type)
                } else {
                    vec![target_rest_type]
                };
                for member in members {
                    if self.st.is_mutable_array_like_type(member)? {
                        some_mutable = true;
                        break;
                    }
                }
                let readonly = !some_mutable;
                let source_rest =
                    self.st
                        .get_rest_type_at_position(source, param_count, readonly)?;
                self.infer_from_contravariant_types_if_strict_function_types(
                    source_rest,
                    target_rest_type,
                )?;
            } else {
                let source_rest = self
                    .st
                    .get_rest_type_at_position(source, param_count, false)?;
                self.infer_from_contravariant_types_if_strict_function_types(
                    source_rest,
                    target_rest_type,
                )?;
            }
        }
        Ok(())
    }

    /// tsc-port: applyToReturnTypes @6.0.3
    /// tsc-hash: 1bb818de205cee65e351fad046065911d45a080a9f63dffde44b2a5b45e42edb
    /// tsc-span: _tsc.js:68224-68237
    ///
    /// tsc-port: typePredicateKindsMatch @6.0.3
    /// tsc-hash: 774c6b6013f1086ab88ed791ca3167f7350c1d1f076149294349f1e8bfa3b599
    /// tsc-span: _tsc.js:61610-61612
    ///
    /// The callback is hard-bound to inferFromTypes (69202).
    fn apply_to_return_types(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult<()> {
        if let Some(target_predicate) = self.st.get_type_predicate_of_signature(target)? {
            if let Some(source_predicate) = self.st.get_type_predicate_of_signature(source)? {
                if std::mem::discriminant(&source_predicate.kind)
                    == std::mem::discriminant(&target_predicate.kind)
                    && source_predicate.parameter_index == target_predicate.parameter_index
                {
                    if let (Some(source_type), Some(target_type)) =
                        (source_predicate.ty, target_predicate.ty)
                    {
                        self.infer_from_types(source_type, target_type)?;
                        return Ok(());
                    }
                }
            }
        }
        let target_return_type = self.st.get_return_type_of_signature(target)?;
        if self.st.could_contain_type_variables(target_return_type) {
            let source_return_type = self.st.get_return_type_of_signature(source)?;
            self.infer_from_types(source_return_type, target_return_type)?;
        }
        Ok(())
    }

    /// tsc-port: inferFromIndexTypes @6.0.3
    /// tsc-hash: d2c290c69b4d6765f25bc2fada5b162e904b4f299e8e46882a86d536e24657be
    /// tsc-span: _tsc.js:69204-69232
    ///
    /// The Mapped&Mapped homomorphic priority is written 1:1.
    fn infer_from_index_types(&mut self, source: TypeId, target: TypeId) -> CheckResult<()> {
        let priority2 = if self
            .st
            .tables
            .object_flags_of(source)
            .intersects(ObjectFlags::MAPPED)
            && self
                .st
                .tables
                .object_flags_of(target)
                .intersects(ObjectFlags::MAPPED)
        {
            InferencePriority::HOMOMORPHIC_MAPPED_TYPE
        } else {
            InferencePriority::NONE
        };
        let index_infos = self.st.get_index_infos_of_type(target)?;
        if self.st.is_object_type_with_inferable_index(source)? {
            for target_info in &index_infos {
                let mut prop_types: Vec<TypeId> = Vec::new();
                for prop in self.st.get_properties_of_type(source)? {
                    let literal = self.st.get_literal_type_from_property(
                        prop,
                        TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                        /*include_non_public*/ false,
                    )?;
                    if self
                        .st
                        .is_applicable_index_type(literal, target_info.key_type)?
                    {
                        let prop_type = self.st.get_type_of_symbol(prop)?;
                        prop_types.push(
                            if self.st.symbol_flags(prop).intersects(SymbolFlags::OPTIONAL) {
                                self.st.remove_missing_or_undefined_type(prop_type)?
                            } else {
                                prop_type
                            },
                        );
                    }
                }
                for info in self.st.get_index_infos_of_type(source)? {
                    if self
                        .st
                        .is_applicable_index_type(info.key_type, target_info.key_type)?
                    {
                        prop_types.push(info.value_type);
                    }
                }
                if !prop_types.is_empty() {
                    let union = self
                        .st
                        .get_union_type_ex(&prop_types, UnionReduction::Literal)?;
                    self.infer_with_priority(union, target_info.value_type, priority2)?;
                }
            }
        }
        for target_info in &index_infos {
            if let Some(source_info) = self
                .st
                .get_applicable_index_info(source, target_info.key_type)?
            {
                self.infer_with_priority(
                    source_info.value_type,
                    target_info.value_type,
                    priority2,
                )?;
            }
        }
        Ok(())
    }
}

/// JS Array.prototype.slice index clamping over a length-`len` list:
/// negative indexes count from the end; the pair is clamped to
/// [0, len] with an empty range when end <= start (the 69131-69137
/// trailing-slice bounds go negative for short sources — probed).
fn js_slice_bounds(len: usize, start: i64, end: i64) -> (usize, usize) {
    let clamp = |i: i64| -> usize {
        if i < 0 {
            (len as i64 + i).max(0) as usize
        } else {
            (i as usize).min(len)
        }
    };
    let from = clamp(start);
    let to = clamp(end).max(from);
    (from, to)
}

#[cfg(test)]
#[path = "../tests/unit/inference/tests.rs"]
mod tests;
