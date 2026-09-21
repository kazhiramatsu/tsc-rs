# H2.5g writer reuse repair — actual Opus rounds 218–220

The third actual walk, r526, performed a cold 9,027-case check with two
TypeScript observations per case. Its sharded comparison failed. Precisely
four cases differ from the stored artifact in seventeen raw diagnostic file
paths for the vendored lib.es5.d.ts; generated writes, diagnostic code/text/
position, summary, execution contract, and the other 9,023 cases are unchanged.
The official writer then reused all 9,027 stale records and wrote identical
bytes, proving that this invocation could not converge without a repair.
The integrator stopped the owned process tree with SIGTERM, after preserving
the cause, rather than let the same failure recur. Its recorded exit is -15
(launcher exit 241), not success. No certificate is issued for r526.

Opus 218 proposed virtualizing the library host. The integrator challenged
that proposal using gate-tax-3 §2 and §4, which deliberately preserve raw
workspace-scoped library paths, and the existing Rust diagnostic comparator,
which recognizes the vendor suffix. Opus 219 withdrew the portability claims
and the virtual-host proposal. Both agreed on a narrow reuse invalidation
predicate. Opus 220 reviewed the concrete implementation and found no blocker.
Its optional absent-array hardening was applied and covered by a sixth test.
Its suggested as-is legacy-record assertion is an external pre-write proof;
it cannot be a permanent assertion that the current, subsequently re-minted
artifact must remain stale.

The predicate rejects external vendored-library diagnostic paths from another
workspace. Exemptions are derived from fresh input files, VFS links, and config
paths, after all existing input/hash checks pass. It inspects both serialized
diagnostic arrays, does not rewrite paths, and leaves the TypeScript host,
Rust source, floor, case roster, receipt keys and enforcement unchanged.
Stored-artifact, receipt and journal reuse all pass through the same guard.
Shard adoption remains the existing same-run child observation path and is
still checked by the whole-artifact comparison.

Actual registered qualification tests passed 61/61. The official --write then
freshly observed four cases and reused 9,023, taking 19.969 seconds. Complete
comparison proves that only seventeen raw library-file leaves and their
fingerprints changed in those four cases, plus generator/envelope hashes.
Agreement with r526's failed-run journals is comparison evidence only; those
journals are not proof under the repaired generator key. The normalized logic
hash changed and a full independent 9,027-case double check is required.

The artifact remains path-scoped by design. The new predicate only covers
vendored-library diagnostic file paths; writes[].source_files, emitted_files
and other path-bearing fields retain the existing full-observation backstop.
This is not a general portability claim, a new diagnostic normalization rule,
or completion of final unsplit CI, hosted checks, or merge.
