# r116 native control qualification

Source aea09cbaf2015a5eef7869fc4888042f9784bf3c: 74 complete commands, 71 exact twice and 3 divergent. Four Rust tests: three pass, the r113 type-comment test fails; exit 101. The 1930-boundary and 4088-original suites were not launched because the bounded set failed.

All five previously divergent r109 cases are now exact: all three declaration controls and both non-block do-body comment controls. All four converted do-body cases are exact. The remaining three failures are the System initialized variable's erased-type comment, and d.ts name/type comment phases on class and parameter properties. All are output/map differences, not new diagnostics. The r113 namespace and generator clone controls are exact.

`recovery-new-controls-r116-capture-summary.json` pins the immutable executable and all 145 full tuple captures. `recovery-new-controls-r116-differences.json` retains the three complete actual/expected tuples; the text-diffs companion shows readable JS/d.ts changes. No KNOWN was added or retired. The census executable/inode was checked before and after a build-only pause and resumed; neither overlapping functional run qualifies performance.
