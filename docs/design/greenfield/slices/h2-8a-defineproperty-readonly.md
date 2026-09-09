# H2.8a A6-26: semantic readonly assignment descriptors

The initial readonly repair below required the
[diagnostic-cache prerequisite](h2-8a-nonexistent-property-cache.md) after its
first runtime replay exposed a speculative re-entry crash. Both initial evidence
and the amended scope remain frozen; the final measured result is at the end.

Runtime repair on b89c73fe675da10934c8be78580545c23c383fa2, in the H2.8 train
whose trusted base is 10748f6ee19ec083ce5748224930c5dcfbbd86df. The checker must
query a bindable Object.defineProperty descriptor's type before deciding whether
its symbol is readonly. The syntax-only implementation is partial-or-stale.
This activates no new option or public resolver method. Other A owners and B–E
remain open. The original function-expando cycle is an adjacent different owner.

Frozen before observations cover 224 main commands and 16 CommonJS exports
commands, with standard libraries, strict checking, declarations, both maps,
CRLF and outDir, across ES5/ES2015 and CommonJS/ESNext. Main: 164 exact and 60
owned failures, identically repeated in two jobs; 54 first declaration callback
differences and six reported diagnostic differences. Each failed supplemental
capture differs only in one declaration's readonly modifier. Exports: eight
exact and eight owned diagnostic differences, identically repeated; all writes
match. Four miss TS2540 and four add TS2540. Main diagnostics include four extra
TS2540 and two extra TS2704. ES5 TS5107 skips semantic reporting, so ES5 success
does not establish the semantic diagnostic branch.

The unchanged fresh comparison catches outside its repetition loop: positive
cases run twice per job and failed cases once. Main has 388 supplemental cloned
Program commands; exports has 24. These are artifact captures, not full actual
tuples. Two main jobs plus captures execute 1,164 commands; exports adds 72.
The original adjacent command executes twice with full tuples, for 1,238 total.
The original reports readonly name:any instead of readonly name:string; its
readonly decision already agrees. It remains outside the repair, guarded by
the exact old vector, unless the required earlier type query itself resolves it.
No type-inference algorithm expansion is authorized to repair that control.

The full current checker baseline executes 1,718 units: 1,717 pass and only the
new descriptor-cache query control fails. The three negative call guards pass.
The earlier test-setup compilation error executed zero tests and is preserved.
The main archive copy's SameFileError and successful path-map completion are
separate immutable records; neither the first record nor its failed script is
rewritten. All before observations remain immutable.

| Step | Local gap and upstream behavior | Concrete Rust action and completion proof |
| --- | --- | --- |
| A6-26-1 | isReadonlyAssignmentDeclaration is partial-or-stale: ObjectLiteral-only syntax scan | In structural.rs use &mut self -> CheckResult<bool>. CallExpression and existing bindable-call predicate guard first. Then check_expression_cached(descriptor, CheckMode::NORMAL), get_type_of_property_of_type(value), and test Option presence, including Some(undefined). With value, query writable symbol then its type; missing type or exact false_fresh/false_regular is readonly. Otherwise only a PropertyAssignment value_declaration causes raw check_expression(initializer, NORMAL); exact false identities are readonly. Without value, use named get_property_of_type_full(set) absence. Preserve every Result with ?. All 240 fresh complete commands and cache/guard controls prove it. |
| A6-26-2 | isReadonlySymbol is partial-or-stale: immutable bool cannot run semantic queries | Widen to &mut self -> CheckResult<bool>; preserve check-flags, property readonly modifier, const/using, get-only accessor, enum order. Clone declaration IDs only after these guards and iterate in order, returning on first true or error. Preserve no-query early exits. TS readonly field, accessor, const, enum, mapped, union/intersection and structural identity commands plus all checker units qualify the shared path. |
| A6-26-3 | All 25 calls in ten files are shared-prerequisite API consumers | Ordinary CheckResult callers add ?. In mapped.rs replace only the is_some_and bool closure with match Some(property) => query?, None => false, still behind include/exclude readonly short circuit. functions.rs returns the query Result directly. The six node-builder calls map abort through existing checker_abort_error with the same checker/context. Preserve left-to-right paired queries and every short circuit. The two new native test callers add expect("readonly descriptor query") only; keep all cache/boolean assertions and fixture inputs. Compile plus 1,718 units and the 261 adjacent complete commands close this API change. |

Allowed production paths are the ten caller files enumerated below. The only
test implementation adaptation is the two new Result callers just specified.
Do not edit binder/parser, inference algorithms, factory, printer, sink/host,
comparison helpers, frozen expectations, public EmitResolver API or CheckAbort.
The structural.rs tsc-port/span/hash entries retain their exact whole owners;
replace stale subset comments. Import CheckMode from tsc_types.

| Semantic state | Producer / owner / updater | Consumer, lifetime, invalidation and identity |
| --- | --- | --- |
| Descriptor expression | Binder-owned NodeId and CallExpression.arguments | The bindable predicate proves exactly three args; borrow IDs, never mutate parsed AST; source_of_node owns source identity |
| Descriptor type cache | CheckerState::check_expression_cached and Links::set_node_resolved_type | Same checker session and speculation log; existing flow-loop/cache save-reset-restore; preserve reentrant cache policy; no new memoization |
| value/writable/set lookup | get_type_of_property_of_type, get_property_of_type_full, get_type_of_symbol | Existing apparent/union/intersection property resolution; named lookup has no index-signature fallback; Option absence differs from undefined TypeId |
| Boolean false identity | TypeTables::intrinsics.false_fresh and false_regular allocated at checker construction | Compare exact TypeId, not broad BooleanLiteral flags; session lifetime, unchanged allocation |
| writable valueDeclaration | Binder SymbolId -> optional NodeId | Only PropertyAssignment uses raw initializer check_expression; shorthand does not manufacture an initializer |
| Symbol declaration list | Binder or checker-owned Symbol.declarations | Copy ordered NodeIds before mutable queries; short circuit in original order; no declaration changes |
| Query failure | CheckResult<bool> / CheckAbort | ? preserves first failure; node builder maps to existing EmitResolverError::CheckerAborted with context identity; no bool fallback |

The native cache test calls the real private assignment helper with an uncached
nonliteral descriptor. Its existing cache/readonly assertions must both pass.
Negative controls use Reflect.defineProperty, a four-argument Object call, and
a non-call descriptor node; none may query its descriptor. There is no existing
expression-query fault injector: CheckAbort::BoundaryProbe is only a speculation
transaction control. Do not invent a production fault or claim an executed
query-abort witness. Source propagation review and the existing full checker
transaction units qualify that unchanged failure channel. No sink, host or
cancellation operation is added. The original cycle control guards reentrancy.

Architecture is freshly read against this base, with source hashes in readiness.
E-RESOLVER-BASE is modified-requalify for the internal query schedule and typed
error propagation: active-unqualified until exact focused qualification. The
public resolver contract remains the same. E-ENTRY, E-PROTOCOL, E-ARENA,
E-CONTEXT, E-METADATA-BASE and E-POSITIONS are premise-unchanged: ProgramSession
run still constructs no emitter component; checking and serialization borrow
the same session; nodes, context, metadata and map domains are unchanged. The
architecture rows' predecessor validation is 0653e10d84351c33ebd34d9442198ffff754722b
(2026-08-17); this fresh audit does not inherit a missing semantic branch from
that historical profile. No planned/dormant row is activated and no architecture
gap is needed. Current symbols/visibility are frozen in the source receipts.

After qualification selects 502 complete commands: main224 + exports16 + all
261 A6-25 commands + the original cycle1. Require all 501 owned/positive commands
exact twice. The cycle must either retain exactly its two before tuples or
become exact twice solely through the specified change. Preserve any different
failure before revising scope. Fresh and original comparators stay unconditional;
an unchanged outside cycle therefore gives Cargo exit101, otherwise require0.
Run all 1,718 checker units on the newly built after binary, with binary/source
hash, size, timestamp and Cargo build provenance; never reuse the before binary.
Also run the old 22 declaration-blocking cases (44 emit-only executions, separate
from complete commands). No supplemental artifact captures after repair; the
original failure capture stores existing full command tuples without re-execution.

Readiness: python3 scripts/check-defineproperty-readonly-readiness.py. The
manifest freezes every witness row, 24 whole TS functions, the 25 caller sites,
three steps, seven architecture rows and baseline/source authority receipts.
Unresolved=0 and undispositioned=0 are required before production edits. Use two
Cargo jobs and background priority, logs plus real exits, and fresh launch paths.
Keep source frozen during canonical jobs. The schedule's lightweight workflow
applies; historical certificate walk and full developer CI are not claimed.
This focused close changes no historical global count and does not close H2.8.

Pinned upstream owners (vendor/typescript-6.0.3/lib/_tsc.js):

| Owner | Whole body span | SHA256 |
| --- | --- | --- |
| isReadonlyAssignmentDeclaration | 79226–79252 | `b24314da6ed2383d7effcd36e7298f7bfc7a28b6ac97beaecf56c41759ef0c4c` |
| isReadonlySymbol | 79253–79255 | `f4bb3512724bb23e8f837910378f78347824481f39847034aec8d8fdf8cf6f3b` |
| isBindableObjectDefinePropertyCall | 15059–15065 | `11b59e7edd8f66683d81b05c7488aa75a2404cf5328472fffb75d1a94f2cb2a9` |
| getTypeOfPropertyOfType | 55803–55806 | `ddd47344f8b1b3d0de20c2241560a370790f21248f978ea95d10914e91566057` |
| getPropertyOfType | 59348–59389 | `39a7221f835629e1b6b6c3d3e53d7aec1032999299e682d96846922fa299498a` |
| getTypeOfSymbol | 56945–56975 | `36123c37428ab9dcfb6c89ba1c42dbf1a5461becdfdade097ed545e21b50bfd7` |
| checkExpressionCached | 80580–80595 | `d53b8def69286cea6beb2fdadf985dcd3c6d0dec3ef171f10eda495f50485178` |
| checkExpression | 80960–80974 | `b56997759c77785af8c96e94324267893636cd83bfdc656d059ef139e4cd71ac` |
| getCheckFlags | 17433–17435 | `a83648d039471a61a67a19bbf1a35c6a4bee49544839d279d7194fcf8a6ad7a4` |
| getDeclarationModifierFlagsFromSymbol | 17436–17452 | `640b7c9d80362bc126b54abbed85737561ceff6a8dec412cfaf111c794a4ef8d` |
| getDeclarationNodeFlagsFromSymbol | 74859–74861 | `a19a53639783b499d7b4b2741669b1b24e90e46a22e849eb4edf43b60c79042b` |
| isAssignmentToReadonlyEntity | 79256–79290 | `2381c469899908deddba8fbd4bbc7ce0898d86865a1e6f19062ef7f8166720de` |
| resolveReverseMappedTypeMembers | 58423–58455 | `9c2b1ea3f2113f2c77230deba94d6bda93f3a13f722929468e52407b4525146c` |
| getSpreadSymbol | 63044–63056 | `081b8438c5ab6cb37771a452a7f336cc67ee020d921994fcd1e6cc09100e97f9` |
| addPropertyToElementList | 52241–52397 | `6c6b916aa8ce07acd5ac0ba435bbb827fea9ee957fdaf3e2843643414cba7580` |
| isConstantReference | 70374–70393 | `63298ed7776bb8e07259b2d8bb0051c1ce8c9e2f1e4be5517b1c15c6eca65e81` |
| propertyRelatedTo | 66663–66707 | `19d5c6a584328bf185f97857552fb6e2d018cee9a73a975a40b8decf71d676da` |
| compareProperties | 67536–67558 | `42f04303574ccb64448bdeda07e716852dbca284333cf3f172159a566c991bb9` |
| createUnionOrIntersectionProperty | 59101–59245 | `21791f74b0558b599db3de8950d26bd93152bbb62c3e250727503334167bf713` |
| isPropertyIdenticalTo | 67533–67535 | `e5bc0670b1b176c446db71d2ecbd36ba6cdc4903be20e3bb2ac807ec25c89652` |
| addMemberForKeyTypeWorker | 58539–58575 | `cffc0b4ecb403dd48564a65c1fdcfaa1388a7fd5b8a78ef6b82daeb1c648a6ac` |
| checkIdentifier | 72126–72213 | `9fe9fcc0b033367436dd525613db28f5705a42f8428a97215ce75bef6e985875` |
| checkDeleteExpression | 79303–79323 | `098796e01bc1bebf86af1a1f3845547ec8ed79951a6b573798b6fbaae6ac96e9` |
| makeSerializePropertySymbol | 55099–55229 | `fab03df95bee26e871b1bc817932eb99b58a9c262f37009266cc1a5291f817a7` |

Current caller inventory:

| Path | Before line | Expression |
| --- | --- | --- |
| crates/checker/src/access.rs | 3101 | `self.is_readonly_symbol(symbol)` |
| crates/checker/src/annotate.rs | 4171 | `state.is_readonly_symbol(property)` |
| crates/checker/src/check.rs | 9055 | `self.is_readonly_symbol(property)` |
| crates/checker/src/check.rs | 9119 | `self.is_readonly_symbol(property)` |
| crates/checker/src/expr.rs | 1029 | `self.is_readonly_symbol(local_or_export_symbol)` |
| crates/checker/src/expr.rs | 3321 | `self.is_readonly_symbol(symbol)` |
| crates/checker/src/functions.rs | 1259 | `self.is_readonly_symbol(resolved)` |
| crates/checker/src/literals.rs | 1951 | `self.is_readonly_symbol(prop)` |
| crates/checker/src/mapped.rs | 589 | `self.is_readonly_symbol(property)` |
| crates/checker/src/node_builder/statements.rs | 4653 | `self.checker.is_readonly_symbol(base_property)` |
| crates/checker/src/node_builder/statements.rs | 4654 | `self.checker.is_readonly_symbol(property)` |
| crates/checker/src/node_builder/statements.rs | 4892 | `self.checker.is_readonly_symbol(property)` |
| crates/checker/src/node_builder/statements.rs | 4990 | `self.checker.is_readonly_symbol(property)` |
| crates/checker/src/node_builder/type_nodes.rs | 4467 | `checker.is_readonly_symbol(property)` |
| crates/checker/src/node_builder/type_nodes.rs | 4540 | `checker.is_readonly_symbol(property)` |
| crates/checker/src/structural.rs | 2200 | `self.st.is_readonly_symbol(source_prop)` |
| crates/checker/src/structural.rs | 2201 | `self.st.is_readonly_symbol(target_prop)` |
| crates/checker/src/structural.rs | 2945 | `self.st.is_readonly_symbol(source_prop)` |
| crates/checker/src/structural.rs | 2945 | `self.st.is_readonly_symbol(target_prop)` |
| crates/checker/src/structural.rs | 5263 | `self.is_readonly_symbol(prop)` |
| crates/checker/src/structural.rs | 5265 | `self.is_readonly_symbol(prop)` |
| crates/checker/src/structural.rs | 5539 | `self.is_readonly_symbol(source_prop)` |
| crates/checker/src/structural.rs | 5539 | `self.is_readonly_symbol(target_prop)` |
| crates/checker/src/structural.rs | 6642 | `self.is_readonly_symbol(source_prop)` |
| crates/checker/src/structural.rs | 6642 | `self.is_readonly_symbol(target_prop)` |

First-after build amendment: the first Cargo invocation exits101 before any test
or command executes. Three existing public-helper assertions in inference/tests.rs
(one) and mapped/tests.rs (two) were omitted from the production-only call census.
The complete failed launch, all original readiness bytes and both test files are
preserved in h2-8a-defineproperty-readonly-first-after-build.v1.json and its archive.
That record's phrase "Production compiled" is an inference from the lib-test-only
errors, not a claim of a successful complete Cargo build or runtime validation.

A6-26-3 additionally allows only these three assertions to call
is_readonly_symbol(...).expect("readonly symbol query"). Preserve each original
assert!/negation and all other test logic and inputs. The existing mapped and
reverse-mapped semantics remain asserted. This extends test API adaptation to
five callers in three files, while production remains the same 25 calls/10 files.
No expected observation, implementation algorithm, test count or acceptance
threshold changes. Readiness is checked again before this mechanical amendment.

Final measured result with the diagnostic-cache prerequisite: all 502 complete
commands match TypeScript twice. All 68 fresh readonly differences, the eight
readonly differences retained by A6-25, and the original function-expando
diagnostic are repaired. The original now reports name:string and remains
non-crashing. All other setter, binding-clone, JSDoc and original adjacent
commands remain exact. The cache repair is required for safe semantic readonly
queries; no assignment type-inference algorithm or recursion fallback was added.

The final combined job exits0 and executes 1,004 complete native commands without
supplemental artifact executions; test-target durations are [0.03, 0.0, 501.67, 51.86] seconds.
The 22 declaration-blocking controls pass with 44 separate emit-only executions.
Four selected query/cache units pass, then all 1,720 current checker units pass
with zero failures, ignored or filtered tests in 1.8 seconds on the
newly built binary. The exact identity set is the old 1,718 plus the two new cache
controls. Binary hash/size/timestamp, source and Cargo build provenance are frozen.
The prior focused cache probe separately executed the original command twice
and both new native controls successfully; those executions are not added to
the final 1,004-command count.

Initial failures remain immutable: the first after-build ran zero tests because
three existing test API callers were missed; the first runtime after completed
984 fresh commands and 44 separate emits, then aborted the first original
attempt with stack overflow before tuple capture. Its original second repetition
and nine other original cases did not run. That candidate's 1,718 checker units
passed. The two new cache controls then failed before the prerequisite, with
all nested and transaction-boundary scenarios observed. These records are not
rewritten as successful runs or counted as a complete 502-command replay.

The final ratchet is h2-8a-defineproperty-readonly-after.v1.json. Production is
bounded to the initial ten checker files plus links.rs. Existing test assertions
retain their meaning while accepting the typed Result; two additional native
tests cover the new journal's nested visibility and all exit boundaries. Public
resolver API, parser/binder, factory/printer, comparison contracts and frozen
TypeScript observations are unchanged. No new emitter-suite run, global total,
full developer CI or historical certificate walk is claimed. Other H2.8a owners,
B–E and final hosted acceptance remain open.
