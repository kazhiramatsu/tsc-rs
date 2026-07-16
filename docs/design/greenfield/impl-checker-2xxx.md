# impl: checker for the 2XXX band (phases 4-8) — copy-level code + emission map

Companion to m3/m4/m5/m6 steps docs, sequenced by
2xxx-first-order.md. The algorithm SKELETONS for the four hot
subsystems are already copy-level in checker-key-functions.md (§1
relation, §2 inference, §3 overload, §4 flow) and
checker-foundations.md (§1 resolution stack, §3 contextual, §6
instantiation) — do not duplicate them; this doc adds (a) the module
layout and checker spine to paste, (b) port tables with the 2XXX
codes each function emits, and (c) §10, the emission-map inventory
that DEFINES "complete 2XXX".

Module layout (`crates/checker/src/`, checker.ts region order —
greenfield §5):

```
state.rs        // Checker struct, links tables, resolution stack
grammar.rs      // checkGrammar* family (phase 6/8 members)
symbols.rs      // resolveName, alias resolution, getTypeOfSymbol workers
declared.rs     // getDeclaredTypeOfSymbol family + base types (§3a/§4a)
type_nodes.rs   // getTypeFromTypeNode family + deferred type evaluation (§3b)
members.rs      // apparent types, resolveStructuredTypeMembers, property lookup
relations.rs    // phase 4 engine (m3 doc)
instantiate.rs  // TypeMapper + instantiateType + fillMissingTypeArguments
contextual.rs   // getContextualType/getContextualSignature (foundations §3)
infer.rs        // phase 7
calls.rs        // resolveCall/chooseOverload
exprs.rs        // checkExpression arms (incl. await/yield/async, §5 rows 13-14)
operators.rs    // binary/unary operator checks
jsx.rs          // §5b — .tsx checking (band codes 2604/2605/2657/2746…)
stmts.rs        // statements + declarations
classes.rs      // class/interface/heritage checks
iteration.rs    // getIterationTypesOf* family
widen.rs        // getWidenedType family (checker-foundations §5)
flow.rs         // phase 8 (checker-key §4)
display.rs      // typeToString/symbolToString (50717) — T2-phase placeholder;
                // nothing at T0 may depend on display strings
driver.rs       // checkSourceFileWorker, deferred nodes
```

State-inventory rule: the [COPY] spine below lists the load-bearing
state only. tsc's FULL checker-instance state is declared at the head
of `createTypeChecker` (`_tsc.js` 46438) — when a port needs a state
field not in the spine, add it to the Checker struct citing that
head, never as a module-local static.

## §1 [COPY] The checker spine (phase 5 opening)

```rust
pub struct Checker<'p> {
    pub program: &'p Program,             // files, options, binder output
    pub types: TypeTables,                // phase 4 crate
    pub diags: Vec<Diagnostic>,

    // links: ONE-WRITE tables (greenfield §4.3)
    pub node_links: SecondaryMap<NodeId, NodeLinks>,
    pub symbol_links: SecondaryMap<SymbolId, SymbolLinks>,
    pub type_links: SecondaryMap<TypeId, TypeLinks>,

    // resolution cycle stack (checker-foundations §1.2)
    pub resolution_targets: Vec<(ResolutionTarget, ResolutionKind)>,
    pub resolution_results: Vec<bool>,
    pub resolution_start: usize,

    // THE speculation rule: links writes assert depth == 0 or route
    // through a transaction dropped on rollback (greenfield §4.3)
    pub speculation_depth: u32,

    // relation engine state (checker-key §1.2)
    pub rel: RelationEngineState,
    // flow state (phase 8; checker-key §4.2)
    pub flow: FlowEngineState,
    // contextual-type stack (checker-foundations §3)
    pub contextual_nodes: Vec<(NodeId, TypeId, bool /*isCache*/)>,
    // inference contexts in flight (phase 7)
    pub inference_contexts: Vec<InferenceContext>,
    // per-deferred-node instantiation budget (checker-foundations §6)
    pub instantiation_count: u32,
    pub instantiation_depth: u32,
}

impl<'p> Checker<'p> {
    pub fn error_at(&mut self, file: usize, start: u32, length: u32,
            msg: &'static DiagnosticMessage, args: &[&str]) -> &mut Diagnostic {
        debug_assert!(self.speculation_depth == 0,
            "diagnostics may not be emitted during speculation; collect and replay");
        self.diags.push(Diagnostic::new(file, start, length, msg, args));
        self.diags.last_mut().unwrap()
    }
    /// error span for a node = getErrorSpanForNode (skips leading trivia;
    /// declaration nodes anchor at their NAME) — port the tsc helper,
    /// every 2XXX position flows through it.
    pub fn error_on_node(&mut self, node: NodeId,
            msg: &'static DiagnosticMessage, args: &[&str]) -> &mut Diagnostic {
        let (f, s, l) = self.get_error_span_for_node(node);
        self.error_at(f, s, l, msg, args)
    }
}
```

`push_type_resolution`/`pop_type_resolution`: paste from
checker-foundations §1.2 verbatim; `reportCircularityError`
(`_tsc.js` 56893) emits the 2502/2506/7022-family — 2502/2506 are
band codes, port with phase 5.

## §2 Relations (phase 4) — status pointers only

Engine per m3-types-relations-steps.md + checker-key §1. The 2XXX
relevance: the engine itself emits 2321 (excessive stack depth) and
2589-adjacent complexity errors, and CAPTURES the failure used by
every 2322/2345 site. Error-chain SHAPING beyond code+span is T2 —
capture heads only.

Types-crate addendum surfaced by the architecture audit — the TUPLE
model: tsc tuples are not a separate TypeData kind; a tuple is a
TypeREFERENCE whose target is a synthesized generic `TupleType`
carrying `elementFlags: ElementFlags[]` (Required/Optional/Rest/
Variadic), `readonly`, and labeled-element declarations, interned per
(flags, readonly, labels) shape. `createNormalizedTupleType`
(`_tsc.js` 61213) performs the variadic/spread normalization
(flattening spreads of tuples, collapsing trailing rests). Add both
to m3 stage 4.1's scope — tuple-heavy 2322/2345/2493/2556/2574
behavior all keys on this model, and retrofitting it under a naive
`Tuple(Vec<TypeId>)` shape is a rebuild-scale change.

## §3 [PORT TABLE] Names, symbol types, type nodes (phase 5)

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 0 | `initializeTypeChecker` — globals merging via `mergeSymbolTable`, globalThis, lazy `getGlobalType` environment (design: program-and-modules.md §3) | 88732 / 47818 | 2318, 2317, cross-file duplicate family |
| 1 | `resolveName` + error path | grep `function resolveNameHelper` | 2304, 2552 (suggestion variant), 2448/2454-adjacent gates, 2661, 2693 (type-only as value via meaning masks) |
| 2 | alias resolution (`resolveAlias`, import/export target chasing) + checker-side `resolveExternalModule` (the 2307-family reporter with suggestions and resolution-mode errors) | grep / 49504 | 2303, 2305, 2306, 2307, 2439, 2665, 2732, 2834/2835, 2846 |
| 3 | `getTypeOfSymbol` dispatch + workers | 56911 | 2502 via circularity; 7022+ (non-band, same sites) |
| 3a | **the DECLARED-type axis** — `getDeclaredTypeOfSymbol` dispatch + workers (class/interface, type alias, enum, enum member, type parameter, alias) | 57502 / 57375 | 2456 (circular alias), 2313/2502-family — this is a SEPARATE lazy family from #3 (`Type` vs `DeclaredType` resolution kinds, checker-foundations §1.2); a design that conflates them cannot express `class C` being mid-resolution as a value while resolving as a type |
| 4 | `getTypeFromTypeNode` dispatch | 63196 | — (dispatch) |
| 5 | `getTypeFromTypeReference` + `checkTypeReferenceNode` | 60557 / 81760 | 2314, 2315, 2344 (via #6), 2749, 2585, 2304-family for missing type names |
| 6 | `checkTypeArgumentConstraints` + `fillMissingTypeArguments` (defaults) | 81682 / 59545 | 2344, 2707 |
| 7 | type-node workers: array/tuple/union/intersection/typeof/keyof/predicate | region after 60557 | 2313 (circular constraint) |
| 7a | deferred type evaluation: `getIndexedAccessType` (AccessFlags), `getConditionalType`, `getTemplateLiteralType`, `getStringMappingType` | 62552 / 62646 / 62057 / 62119 | 2536, 2537, 2538-adjacent, 2589 via instantiation — NOT stubs: conditional/mapped/template evaluation is phase 5-6 work, the m4 5.1 "stub if rate allows" note applies only to first bring-up |
| 8 | enum member evaluation `createEvaluator`/`computeMemberValue` | 19382 / near 85595 | 2553, 2474, 2477/2478, 18033-family neighbor (non-band) — port the else-if chain EXACTLY (const → ambient → assignability) |

## §4 [PORT TABLE] Member access (phase 5)

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `getApparentType` chain | 59093 | — (feeds everything) |
| 2 | `resolveStructuredTypeMembers` | 58679 | — |
| 3 | `getPropertyOfType` + union/intersection property synthesis | 59100 | — |
| 4 | `checkPropertyAccessExpression` + `checkPropertyAccessibility` | 75069 | 2339, 2445, 2446, 2341, 2673/2674, 2571, 2531/2532/2533 (nullish receivers — full behavior lands with phase 8 narrowing; port the checks now against declared types) |
| 5 | `reportNonexistentProperty` | 75416 | 2339 + 2551 (suggestion variant), 2576 (static/instance mixup) |
| 6 | element access + index signatures | grep `checkIndexedAccess` | 2538, 2536, 2464, 7053-family (non-band, same site) |
| 7 | `checkIndexConstraints` | 84705 | 2411, 2374-adjacent (2412/2413 are 6.0-dead — see the full-band accounting in §10) |
| 8 | **class/interface base machinery** — `getBaseConstructorTypeOfClass`, `getBaseTypes`, base-type circularity | 57146 / 57218 | 2310, 2312, 2507, 2508, 2510 — lives in `declared.rs` with §3a; heritage CHECKING (§6#3) consumes what this resolves |

## §5 [PORT TABLE] Expressions + assignability sites (phase 6)

The single most important row is #1 — nearly every relation-family
2XXX flows through it.

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `checkTypeAssignableTo` / `checkTypeRelatedToAndOptionallyElaborate` | 63931 / 63947 | 2322 (assignment/return/initializer heads), elaborations (T2) |
| 2 | `checkObjectLiteral` + computed names + `getSpreadType` | 74135 / 62964 | 2353 excess via relation, 2418, 2464, 2698 |
| 3 | array literals + tuple contexts | grep `checkArrayLiteral` | 2322-family via #1, 2461-adjacent |
| 4 | assignment operators + destructuring assignment | grep `checkAssignmentOperator` / `checkDestructuringAssignment` | 2322, 2364, 2739/2740/2741 (property-missing forms), 2779 |
| 5 | `checkBinaryLikeExpression` + `checkArithmeticOperandType` | 80009 / 79214 | 2362, 2363, 2365, 2447, 2460 |
| 6 | equality/comparison via comparable relation | inside #5 | 2367 (+2839 neighbor) |
| 7 | `checkInExpression` / `checkInstanceOfExpression` | 79582 / 79558 | 2361, 2358/2359, 2638 |
| 8 | template expressions, tagged templates | grep | 2345-family via calls |
| 9 | `checkSuperExpression` | 72509 | 2335, 2336, 2337, 2660 |
| 10 | this expression | grep `checkThisExpression` | 2331, 2332, 2683 (non-band 7041 neighbor) |
| 11 | assertions/as/satisfies | grep `checkAssertionWorker` | 2352, 1360-adjacent (satisfies → 2322 heads via #1) |
| 12 | return/yield checking | grep `checkReturnStatement` / `checkYieldExpression` 80447 | 2322 heads, 2355, 2408, 2504-adjacent |
| 13 | **async/await machinery** — `checkAwaitExpression`, `checkAwaitGrammar`, `getAwaitedType` (+ unwrap/awaited-type recursion guards), `checkAsyncFunctionReturnType`, `createPromiseReturnType` | 79408 / 79352 / 82431 / 82498 / 78729 | 2570, 2697, 2547, 2705, 2711/2712, 2524, 2852-2854, 2589-adjacent depth guards; `getAwaitedType` also feeds for-await and async-iteration (§7#3) |
| 16 | **`checkIdentifier` expression-side family** — assignment-target mutability + TDZ helpers (`checkResolvedBlockScopedVariable` 48462) + nodeCheckFlags side (`checkIdentifierCalculateNodeCheckFlags` 72071) | 72158 | 2540 (readonly), 2588 (const assign), 2628-2631 (enum/class/namespace/import assign), 2539, 2448/2449/2450 (TDZ), 2454 (with phase 8 flow), 2496/2522/2815 (`arguments` family) |
| 14 | contextual signatures for function expressions — `getContextualSignature` | 73851 | — (drives parameter typing; 7006-family non-band + 2345 correctness) |
| 15 | remaining checkExpressionWorker arms (the dispatch at 81011 is the completeness checklist — diff your match against it): `checkDeleteExpression` 79303, `checkSpreadExpression` 73939, `checkExpressionWithTypeArguments` 77963 (instantiation expressions), `checkMetaProperty` 78061, `checkImportCallExpression` 77718, `checkPrivateIdentifierExpression` 75112 (`#x in obj`) | as listed | 2703, 2704, 2790 (delete); 2461-family (spread); 2558 (instantiation exprs); dynamic-import module errors + Promise typing; private-`in` narrowing feed |

### §5b [PORT TABLE] JSX (.tsx fixtures; phase 6 after §5, phase 7 for calls)

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `checkJsxElement`/`checkJsxSelfClosingElement` + children checks | 74320 | 2657, 2746, 2747 |
| 2 | intrinsic vs value tags: `getIntrinsicTagSymbol`, JSX.IntrinsicElements lookup | grep | 2339 (unknown intrinsic), 2604 |
| 3 | `resolveJsxOpeningLikeElement` (routes into resolveCall) | 77397 | 2604, 2605, 2769-family via calls |
| 4 | attributes: `createJsxAttributesTypeFromAttributesProperty` + spread + relation check vs props | grep `checkJsxAttributes` | 2322 heads (attr assignability), 2606, 2710 |
| 5 | JSX namespace/factory resolution (`getJsxNamespace`, React/factory entity lookups) | grep | 2602, 2503-adjacent, 2874-family (newer factory diagnostics — verify corpus presence before porting) |

## §6 [PORT TABLE] Declarations, classes, modules (phase 6)

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `checkVariableDeclaration` + `checkVarDeclaredNamesNotShadowed` | 83600 / 83371 | 2322 heads, 2403, 2451-adjacent checker half, 2481, 2488-adjacent for-of names |
| 2 | `checkFunctionOrConstructorSymbol` (overload consistency) | 82024 | 2391, 2392, 2393, 2394, 2389 |
| 3 | `checkClassLikeDeclaration` + heritage + member override checks | 84994 | 2415, 2417, 2420, 2422, 2507, 2510, 2340, 2425/2426, 2610, 2611, 2612, 2699, 2720 |
| 4 | property initialization (strictPropertyInitialization) | grep `checkPropertyInitialization` | 2564, 2565 (definite-assignment half needs phase 8) |
| 5 | interface checks | grep `checkInterfaceDeclaration` | 2320, 2310-adjacent, 2430, 2411 via §4#7 |
| 6 | enum + const enum rules | grep `checkEnumDeclaration` | 2432, 2567, 2474-family via §3#8 |
| 7 | module/namespace checks — `checkModuleDeclarationDiagnostics` | 85853 | 2433, 2434, 2435, 2436, 2437, 2668, 2669, 2670 |
| 8 | import/export checking | grep `checkImportDeclaration` / `checkExportSpecifier` + `checkExportsOnMergedDeclarations` (82215) | 2305, 2306, 2308, 2323, 2484, 2528, 2653, 2661 |
| 9 | declaration merging compat | inside the above + grep `checkTypeDuplicateChecks` equivalents | 2403 (identity relation), 2717, 2687 |
| 10 | destructuring declarations + binding patterns | inside #1 region | 2459, 2461, 2488, 2525, 2739/2741 |
| 11 | **declaration-shape checks the audit surfaced** (checkSourceElementWorker at 86557 also dispatches these — it is the statement-side completeness checklist): `checkSignatureDeclaration` 81289, `checkParameter` 81170, `checkConstructorDeclaration` 81556, `checkAccessorDeclaration` 81625, `checkClassStaticBlockDeclaration` 81552 | as listed | 2370 (rest param array), 2371 (initializer in ambient), 2372/2373 (param self-reference), 2333/2376/2377 (constructor/super rules; 2334 is 6.0-dead), 2378-family + 2808 (accessor pairs) |
| 12 | type-ANNOTATION node checking — checkSourceElementWorker dispatches type nodes as source elements too: `checkTypePredicate` 81206 plus the checkArrayType/checkTupleType/checkConditionalType/checkImportType/checkNamedTupleMember arms | 86557 region | 2677 (predicate type assignable to param), 1225-family grammar neighbors; checkTypeReferenceNode (§3#5) is one of these arms |
| 13 | `checkExportAssignment` + `checkWithStatement` | 86391 / 84589 | 2309, 2306-adjacent; 2410 (with-block symbol rule) |
| 14 | `checkForInStatement` (left-side and rhs rules) | 83864 | 2405, 2406, 2407, 2491, 2780 |
| 15 | **import attributes / assertions** — `checkImportAttributes` | 86193 | 2821, 2822, 2823, 2836, 2856, 2857, 2880 |
| 16 | `checkExternalEmitHelpers` (importHelpers-gated helper lookups) | 88922 | 2343, 2807 — gated on fixtures setting `importHelpers`; verify corpus presence before deep-porting |

## §7 [PORT TABLE] Iteration protocol (phase 6)

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `getIterationTypesOfIterable` + cached worker family | 84062 | 2488, 2489, 2504, 2548, 2549, 2568 |
| 2 | for-of / spread / destructuring / yield* consumers | callers of #1 | 2461, 2493, 2494, 2766 |
| 3 | async iteration variants | same family, async flavor | 2504, 2734-adjacent |

## §8 [PORT TABLE] Calls + inference (phase 7)

Structure per checker-key §3 (M4 stubs inference; phase 7 fills it —
see m4 5.7 / m6 docs).

| # | Function | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | `getEffectiveCallArguments` | 76295 | — |
| 2 | `reorderCandidates` | 75768 | — |
| 3 | `resolveCall` + arity/typearg error reporting | 76579 | 2554, 2555, 2556, 2557, 2558, 2575, 2769 (failure path), 2635-adjacent |
| 4 | `getSignatureApplicabilityError` | 76194 | 2345 (argument heads via §5#1 machinery), 2684 (this-arg) |
| 5 | `chooseOverload` + re-run | 76763 | — (selection; errors via #3/#4) |
| 6 | `resolveCallExpression` / `resolveNewExpression` / tagged/decorator resolvers | 76972 / 77055 | 2347, 2348, 2349, 2350, 2351, 2673674 family via §4#4, 2511 (abstract new), 2674 |
| 7 | `inferTypeArguments` + `inferTypes`/`inferFromTypes` + `getInferredType`/`getCovariantInference` | 75938 / 68637 / 69271 / 69263 | — directly; unlocks correct 2345/2769/2344 |
| 8 | union/intersection callable synthesis | grep `getUnionSignatures` | 2349 correctness on unions |

## §9 [PORT TABLE] Flow + operators completion (phase 8)

Engine per checker-key §4 (copy-level there). Band-relevant
consumers:

| # | Consumer | Anchor | 2XXX emitted |
|---|---|---|---|
| 1 | definite assignment / TDZ | grep `checkVariableDeclarationList` + flow `isDefinitelyAssigned` | 2454, 2448, 2729 (property use before init) |
| 2 | strictPropertyInitialization completion | §6#4 | 2564 full |
| 3 | exhaustiveness + comparability on narrowed types | switch checking | 2367 completion, 2678 |
| 4 | nullish receiver checks on NARROWED types | §4#4 sites | 2531/2532/2533/2571 completion |
| 5 | unreachable-adjacent band codes | reachability walk | 2365-family cleanup on dead branches |

## §10 THE 2XXX EMISSION MAP — the completeness inventory

Top-30 corpus codes (frequency priors from the first
implementation's mining) → owner function → phase. Phase 9's sweep
extends this table to EVERY band code the oracle emits on the corpus
(`xtask conformance --band 2xxx --codes` lists them); a code is DONE
when its row's function is ported, its pins are green, and the
corpus shows 0 FP / 0 FN for it.

| Code | Meaning (short) | Emitting function (anchor) | Phase |
|---|---|---|---|
| 2322 | type not assignable | checkTypeAssignableTo 63931 heads everywhere | 6 |
| 2339 | property does not exist | reportNonexistentProperty 75416 | 5 |
| 2304 | cannot find name | resolveName error path | 5 |
| 2345 | argument not assignable | getSignatureApplicabilityError 76194 | 7 |
| 2403 | redeclare mismatch | checkVarDeclaredNamesNotShadowed-adjacent redecl compat 83371 | 6 |
| 2554/2555 | expected N args | resolveCall arity 76579 | 7 |
| 2769 | no overload matches | resolveCall failure 76579 | 7 |
| 2300 | duplicate identifier | binder declareSymbol 42602 | 3 |
| 2411 | property incompatible with index | checkIndexConstraints 84705 | 5 |
| 2365 | operator cannot be applied | checkBinaryLikeExpression 80009 | 6 (full: 8) |
| 2367 | comparison unintentional | equality via comparable relation | 6 (full: 8) |
| 2454 | used before assigned | flow definite-assignment | 8 |
| 2571 | object is of type unknown | property access nullish/unknown checks | 5 (full: 8) |
| 2532/2533 | possibly undefined/null | same | 8 |
| 2349 | not callable | resolveCallExpression 76972 | 7 |
| 2314 | generic requires type args | getTypeFromTypeReference 60557 | 5 |
| 2344 | type arg constraint | checkTypeArgumentConstraints 81682 | 5 (full: 7) |
| 2415/2417 | class incorrectly extends / static side | checkClassLikeDeclaration 84994 | 6 |
| 2420 | incorrectly implements | same | 6 |
| 2488 | must have Symbol.iterator | getIterationTypesOfIterable 84062 | 6 |
| 2461 | not an array type (destructure) | destructuring checks | 6 |
| 2352 | conversion mistake (as) | checkAssertionWorker | 6 |
| 2564 | property not definitely assigned | property init + flow | 6 (full: 8) |
| 2739/2740/2741 | missing properties forms | relation error selection at assignment sites | 6 |
| 2551/2552 | did-you-mean variants | resolveName/reportNonexistentProperty suggestion paths | 5 (spelling: 9) |
| 2678 | switch case not comparable | switch checking via comparable | 8 |
| 2749 | value used as type | type-reference resolution 60557 | 5 |
| 2451 | block-scoped redeclare | binder | 3 |
| 2693 | type-only used as value | resolveName meaning masks | 5 |
| 2589 | instantiation too deep | instantiateType guards 63315 | 5 |
| 2507/2510 | not a constructor function type / base circularity | getBaseConstructorTypeOfClass 57146 | 6 |
| 2697/2547 | async return not Promise | checkAsyncFunctionReturnType 82498 | 6 |
| 2604/2605 | JSX element type invalid | resolveJsxOpeningLikeElement 77397 + checkJsxElement 74320 | 6-7 |
| 2536/2538 | index-type cannot be used | getIndexedAccessType 62552 + element-access checks | 5 |

Rules for the sweep (phase 9): work the table by corpus frequency;
one code = one workstream = pins + port + band gate; a code whose
function is already ported but still mismatching gets a probe-first
divergence hunt (EXECUTION-GUIDE discipline), never a local patch at
the emission site.

### Full-band accounting (the "ALL of 2XXX" answer, mechanically derived)

Measured against the 6.0.3 pin (re-run on re-vendor via
`xtask codegen band-inventory`, which regenerates exactly this
analysis: every `Diagnostics.<name>` use in `_tsc.js`, joined with
the 2000-2999 message table, region-classified by line):

- **539** codes exist in the 2XXX range of the message table.
- **472** are referenced somewhere in `_tsc.js` — the design-time
  universe. The other **67 can never fire in the batch compiler**
  (services/codefix titles like 2211/2212, and 6.0-retired variants)
  — out of scope BY CONSTRUCTION, verified empirically by the
  goldens (any oracle-emitted code outside the 472 would surface
  there and gets triaged, not assumed).
- **455** of the 472 have checker-region emission sites → covered by
  this doc's port tables plus the two completeness checklists
  (checkExpressionWorker @81011, checkSourceElementWorker @86557).
- The **13 codes never emitted from checker/binder** and their
  design homes:

| Codes | Emitting site (anchor) | Design home |
|---|---|---|
| 2302, 2562, 2467 | `resolveNameHelper` @19643 (a UTILITIES-region function the checker calls back into — §3#1's anchor) | symbols.rs, §3#1 |
| 2809 (`=` after block), 2754 (super type args), 2819 (namespace reserved name) | PARSER error paths (`parseBlock` @33077, `parseSuperExpression` @32323, `parseErrorForMissingSemicolonAfter` @29620) | impl-parser — parser-emitted 2XXX land in PHASE 2 and count toward band parity |
| 2458 (AMD pragma), 2688 (type reference directive), 2726/2727 (lib reference directive) | pragma/reference processing (`processPragmasIntoFields` @36275, program workers @124513/@125846) | program-and-modules §2b |
| 2578 (unused @ts-expect-error) | `getMergedBindAndCheckDiagnostics` @123752 | program-and-modules §2b — the comment-directive machinery ALSO SUPPRESSES arbitrary diagnostics; without it every directive-bearing fixture mismatches |
| 2209, 2210 | package-json export/import map resolution @41831 | out of scope by harness (no package.json in corpus programs) — record in the phase-9 ledger, confirm via goldens |

Corrections this accounting forced on earlier tables (codes that are
in the message table but DEAD in 6.0 — never referenced): 2412/2413
removed from §4#7 (the exactOptionalPropertyTypes variants are
retired; live behavior flows through 2322/2411 with chain
elaboration) and 2334 removed from §6#11 (2337-family covers the
live checks). Lesson standing: port tables cite codes only after the
band-inventory confirms the code is live.

### §11 The emitting-FUNCTION inventory (function-level completeness)

Complementing the code-level accounting: attributing every band
emission site to its enclosing tsc function yields **247 distinct
2XXX-emitting functions (623 emission sites)** at the pin. Coverage
model — a function is covered when it is REACHABLE from a port-table
row under the transcription rule ("unknown helpers encountered
mid-transcription go onto the port table as new rows",
2xxx-first-order.md): ~55 emitters are named directly in the design
docs; the rest are helpers inside named functions' regions
(`reportImplementationExpectedError` ports with
`checkFunctionOrConstructorSymbol`, `hasExcessProperties`/
`propertyRelatedTo`/`reportUnmatchedProperty`/`reportIncompatibleStack`
/`reportRelationError` @65096 with the relation engine's error half —
note: that error half carries band codes 2200-2205/2322/2326-2328
itself, so §2's "capture heads only" includes porting THESE call
sites). The canonical regenerable list is
`cargo xtask codegen band-inventory --by-function --band 2xxx`; the phase-9 closure
criterion at function level: **every function on that list has a
ledger entry or an explicit out-of-scope note.**

Tooling caveat baked into the band-inventory spec: usages of the
form `Diagnostics.X.code` are MEMBERSHIP TABLES, not emissions (the
program layer's `plainJSErrors` set is one; nearest-function
attribution mis-assigns them — two known artifacts at @122516 and
@47312). The tool must classify `.code` reads separately.

Real coverage gaps this pass found (rows added/updated below):
`checkIdentifier`'s assignment-mutability family, import attributes,
external emit helpers, for-in statement checks, checker-side
`resolveExternalModule`.

**The complete, line-ordered 247-function checklist lives in
[2xxx-emitter-inventory.md](2xxx-emitter-inventory.md)** (71 NAMED /
176 helper, 623 emission sites, membership tables split out) — that
file, regenerated per re-vendor, IS the function-level reproduction
contract this section defines.
