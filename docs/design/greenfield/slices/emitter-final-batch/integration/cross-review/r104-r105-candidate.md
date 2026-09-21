# r104–r105 candidate, qualification pending

The r101 boundary run at 663da1938 passed all 1,526 pre-existing cases. The 394 new cases produced 355 exact and 39 failed complete commands: six for-await comment cases, nine map-only callee/access cases, and 24 object-rest await cases. Emitter lib and checker lib passed on that head. These results do not qualify the successor below.

Actual Claude Opus103–105 reviewed the source causes and implementation. Source-ranged virtual grammar parentheses now record their own raw-range boundaries around the child pipeline. Ancestor suppression remains in the existing recorder; child overrides do not become parent overrides. Brace comments require same-kind parsed provenance. Variable-initializer token comments honor active declaration/list end ownership. Uninitialized retained names run their trailing phase before declaration syntax, including the erased-type raw end under the same name flags and ancestor suppression. The latter covers the ES5 generator control identified in Opus105; it is limited to the existing no-initializer correction.

Await loops prepare an object-rest head before child visitation, creating the TS local temporary and raw binding statement once. The parsed loop is retained for parent-based layout; transformed range/original propagation uses the prepared loop. No block-scope interface was added: the es2018 pass does not produce such declarations.

The ten new complete-command controls exercise computed binding and assignment keys, assignment, member iterators, labels, nesting, typed uninitialized name comments at ES5/ES2017, a generator's erased-type boundary, and declaration-name JSDoc order. Each oracle command was observed twice; native successor results are pending. The initial six/eight-control observations were preparation stages; the final ten-case fixture is authoritative for qualification.

Review against broad impact: rerun all 1,930 boundary cases, emitter contracts, checker lib and program tests on fixed committed bytes. Keep existing exact assertions and complete callback/diagnostic/map comparisons. The later full gate and parse-census replay proof remain required.

The user's post-integration architecture concern is recorded separately in `post-integration-dependency-boundaries.md`; this candidate does not introduce that refactoring.
