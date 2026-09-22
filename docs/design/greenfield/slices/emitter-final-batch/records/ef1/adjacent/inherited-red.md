# Inherited red at the start SHA (not an EF1 regression)

`cargo test -p tsc-rs-emitter --test contracts` after the EF1 patch: 451 passed / 1 failed
(`emitter-contracts.log`): `active_transform_contract::compact_private_function_body_emits_inter_statement_comment_once`
(`/* PRIVATE_BODY_BETWEEN */` printed twice inside `_A_method = function _A_method() { first(); … second(); }`).

Bisect: with the PropertyAccess arm reverted (ElementAccess arm still patched) the test still fails
(`inherited-compact-private-body-pa-reverted.log`); with `crates/emitter/src/printer.rs` checked out to
the start SHA `c35e00ccb` it fails identically (`inherited-compact-private-body-start-bytes.log`,
`test result: FAILED. 0 passed; 1 failed`). The failure predates this batch; it is an ordinary-emit
comment-ownership defect (private method body hoisted by class-fields lowering at ES2015) and is carried
in the EF7 ledger as a reproduced divergence with owner printer/class-fields, not counted against EF1.
