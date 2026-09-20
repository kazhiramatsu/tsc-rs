# W5 current census and historical provenance

The second official walk, r519, stopped with exit 1 after legitimate partial
mints. Its native 172-row census was freshly produced; all 67 selected rows
had complete path-joined write matches. The historical generator predicate
required at least one missing declaration write, so completed emission failed.
The whole walk remains unqualified. Its partial changes and failure logs are
preserved without manufacturing a successful receipt or convergence certificate.

The repair preserves the entire historical stratum through the original M1
projection guard. The 67 identities are still independently selected from the
frozen 172-row pool and checked against the signed list. Both original M1 and S2
hashes, all original case/observation bytes and the historical census hash stay
unchanged. The current native census is a separate readiness check. Its SHA is
reported in the log, not written into the signed historical section.

Current checks require no missing, differing, or unexpected writes; the exact
expected write count; the original emitSkipped and source-map count; and equal
diagnostic counts additionally bound to the qualification's expected count.
This is a bounded check of bytes/BOM by output path and counts. It does not
compare callback sequence or diagnostic contents and is not a full command
parity certificate. The adjacent declaration replay suites provide separate
checks; they also do not substitute for full command coverage.

Actual Opus round 212 proposed allowing the historical missing-declaration state
as well as complete output. The integrator rejected that relaxation because a
future missing declaration would pass. After reviewing the concrete candidate,
round 213 withdrew that proposal and agreed that a stricter current check does
not change or re-sign the historical selection. Round 214 found no correctness
or acceptance blocker in the applied generator and tests.

Freshness is mandatory on every actual check/write invocation, including a
TypeScript observation-receipt hit. The old census is removed before running
the existing native producer. A successful command that writes nothing fails;
the list is replaced atomically before execution. The former census environment
override is rejected. The measured cost was approximately 44 seconds for 172
native cases; this is accepted rather than adding a new cache protocol.

Tests load unchanged private function bodies with a separate child-process
stub module for each instance. No production test seam or process-wide builtin
mock was introduced. The synthetic controls deliberately test predicate shape,
not native conformance. The recorded fresh canonical probe uses the real Cargo
producer with no stub or injected census and does not mint an artifact.

Round 213's requested missing-artifact error change was not applied: both real
entrypoints already call the same projection verifier before preparing the
stratum, so a catch later in loadStratumCases would not affect that failure.
Round 214 confirmed this reasoning. Optional test suggestions require no new
acceptance behavior; the existing test comment already keeps the output target
private, and no further source change was made after the successful battery.

The failed walk left 59 harness manifest hash rows stale. This is the existing
official driver's recovery responsibility. The repair does not hand-mint or
bypass that recovery. A new sanctioned walk, complete unsplit final-head local
CI, hosted checks, and merge verification are still required.
