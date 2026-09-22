# Round67: implementation review for eight native failure causes

Read-only, no builds or edits. Recovery-next /Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next currently uncommitted r67 emitter changes over 18c0e70a4. Source frozen during review and cargo check. Inspect git diff crates/emitter. All eight round66 recommendations are implemented; please independently check precise equivalence and adjacent regressions, especially:
1. factory apply_parenthesizer_rules adds statement, six unary operands, yield. Statement call callee parenthesization restores PartiallyEmitted ancestors. Existing prefix helper uses precedence; new postfix threshold LHS. Confirm equivalence with upstream isUnary/isLeftHandSide for As/TypeAssertion/NonNull/PartiallyEmitted and any special kinds; avoid broader changes without observable requirement.
2. System initial reservation now parsed identifiers + untyped synthetics + generated module bases. Typed temps reserved when wrapper-hoisted. Check call order (late module hoists / custom prologue), and generated name collision identity.
3. Replacement variable assignment only copies text range, not original/emit flags, per upstream; inspect affected comment ownership.
4. CJS exported import branch return immediately even if no exports, like getExports.
5. ES2017 names unchanged for method/get/set; nullish return and block ranges.
6. optional questionDot gap uses trivia-skipped token start only.

TS observer is being expanded 722->764 with seven shapes x six (static await decorator, computed getter/setter await, computed async IIFE method/get/set, module optional earlier temp). Prior722 unchanged assertion will be checked; regenerated two-complete-command observations may still be running. Native tests not yet rerun. New static-decorator case deliberately checks your remaining postassigned warning, not yet changed speculatively.

Supplemental r66 full captures finished: 648exact74failed, artifact records/captures/system-r66. Census work remains separate; not scope of this review. Please list concrete blockers, low-risk amendments, and necessary regression suites. Do not start builds.
