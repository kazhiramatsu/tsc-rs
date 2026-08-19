# gate-tax 2 — witness observation adoption + resume divergence printing

CI slice (slice A/B/B' lineage: PR #448/#450 precedent), branch
`ci/gate-tax-2`. Ratified 2026-08-19: emitter-first with a 1–2 train
CI intervention. Goal: cut the per-train chain-walk/converge tax for
the H2.5h-a packet ladder (CS-4..B-act) from 40–80 min to minutes,
without touching the soundness keystones. Every mechanism here banks
into the post-H2.9 evidence-DAG design.

Design provenance: five review passes over the working draft
(adversarial, independent re-verification, code-verification,
resume/re-run lens, holistic consolidation). The consolidated scope
below is the authoritative outcome; superseded alternatives
(check-side adoption, NodeRuntimeOracle ratchet narrowing) were
adjudicated and dropped — do not resurrect them without a new review.

## 1. Scope

1. **Write-side observation adoption** in the three H2.5h-a
   observation generators (`h2-5h-a-foundation.mjs`,
   `h2-5h-a-comment-scope-witnesses.mjs`,
   `h2-5h-a-es2015-generators-witnesses.mjs`).
   - Adoption key, all-or-nothing: stored `generator` sha +
     `typescript` record (now including the vendored **lib inventory**
     digest — the fresh-process type check reads `lib.*.d.ts` from
     disk, uncovered by the bundle/implementation hashes) byte-match
     current, and every stored fingerprint validates. Gated on
     `--write`; `--check` and the internal fresh-process child never
     consult the store.
   - Only the fresh-process oracle **observations** are adopted.
     Assembly and every `requireCondition` — marker expectations,
     foundation-control cross-checks against the CURRENT foundation,
     layer-evidence guards, census guards, lineage pins — re-execute
     on every write. The stored observation fingerprints stand for the
     repetitions=2 determinism proof (5g precedent).
   - Atomic same-directory temp+rename writes in all four
     observation-carrying generators (the three above plus
     `h2-5g-qualification.mjs`): a kill mid-write can never truncate
     the artifact, which doubles as the adoption/reuse store.
   - The three contract schemas gain the required
     `typescript.lib` record (`path`, `default_libraries`, `sha256`).
2. **Convergence loop** (§3): the four observation-carrying scripts
   converge through adoption-enabled `--write` + pre/post byte diff;
   plain `--check` for them runs only at the slice boundary (packet
   checker) and, for 5g, inside the gate's freshness proof.
3. **Resume divergence printing** (`local_ci_resume.rs` only; journal
   schema 2): the journal stores per-component records — tool
   version-output hashes, CI environment key/value hashes, and the
   per-file workspace snapshot — and a declined receipt prints exactly
   which environment key, tool, scoped file, or recorded output
   diverged. Schema-1 journals are replaced silently. The
   `TSRS_H2_5G_CHECK_SHARDS` set-vs-unset incident is replayed as a
   unit test.
4. **This document's own doc-bug fix**: `h2-5h-a.md` no longer claims
   the six artifact `--check`s run inside the local gate (they never
   did; the gate validates through the qualification registry's
   contract table), with the envelope re-pinned.

## 2. Key asymmetry (do not "fix" either side toward the other)

| | adoption key | full re-observation backstop |
|---|---|---|
| `h2-5g-qualification` | loose (no own-generator term; pin-carrying inputs projected) | EVERY full local gate (freshness proof) |
| H2.5h-a witnesses/foundation | strict (own generator sha + typescript record + lib digest) | once per slice (packet checker) |

Both are deliberate: the backstop frequency licenses the key
looseness. The witnesses' all-or-nothing trigger (no per-case
fallback) is also deliberate — less machinery inside evidence
generators; a generator edit pays one full re-observation on its own
train, which doubles as the fresh-observe acceptance run.

## 3. Canonical convergence loop

Session tooling — never check this into `scripts/` (that directory
sits inside the WorkspaceAudit AND NodeRuntimeOracle input scopes, so
every tweak would re-trigger the 20–40 min 5g freshness receipt).
Re-derive the driver from this block verbatim; pin-const sync between
rounds (`sync_consts`) stays the walk driver's job.

```bash
#!/bin/bash
# gate-tax 2 convergence. The four observation-carrying scripts
# converge via adoption-enabled --write + byte diff; everything else
# keeps plain --check per round. Ends with exit "$rc": never let a
# trailing echo eat the real status (2026-08-19 wrapper incident).
set -u
cd /Users/hiramatsu/dev/tsc-rs
Q="taskpolicy -c maintenance nice -n 20"
# Topological order; OBS entries are the four observation carriers.
ALL=(h1-rust-omission-inventory h1-emit-qualification h2-transition
     h2-1a-qualification h2-1a-profile h2-1b-qualification
     h2-1c-qualification h2-1e-qualification h2-2c-qualification
     h2-2d-qualification h2-3a-qualification h2-3c-qualification
     h2-3d-qualification h2-5d-qualification h2-5e-qualification
     h2-5f-qualification h2-5g-profile h2-5g-qualification
     h2-5h-a-foundation h2-5h-a-comment-scope-witnesses
     h2-5h-a-owner-graph h2-5h-a-gap-matrix h2-5h-a-dispositions
     h2-5h-a-es2015-generators-witnesses)
obs() { case "$1" in h2-5g-qualification|h2-5h-a-foundation|h2-5h-a-comment-scope-witnesses|h2-5h-a-es2015-generators-witnesses) return 0;; *) return 1;; esac; }
# art() serves only the four OBS names, whose artifacts are all regular.
art() { echo "ratchets/$1.v1.json"; }
rc=0
for round in 1 2 3 4 5; do
  stale=()
  for n in "${ALL[@]}"; do
    if obs "$n"; then
      cp "$(art "$n")" "/tmp/cv-$n.before" 2>/dev/null || :
      if ! $Q node "crates/oracle/$n.mjs" --write > "/tmp/cw-$n.log" 2>&1; then
        echo "re-mint $n FAILED :: $(grep -m1 Error "/tmp/cw-$n.log" | cut -c1-90)"
        rc=1; break 2
      fi
      cmp -s "$(art "$n")" "/tmp/cv-$n.before" || stale+=("$n=rewrote")
    else
      $Q node "crates/oracle/$n.mjs" --check > "/tmp/cv-$n.log" 2>&1 || {
        stale+=("$n")
        $Q node "crates/oracle/$n.mjs" --write > "/tmp/cw-$n.log" 2>&1 || {
          echo "re-mint $n FAILED :: $(grep -m1 Error "/tmp/cw-$n.log" | cut -c1-90)"
          rc=1; break 2
        }
      }
    fi
  done
  echo "round $round stale: ${stale[*]:-none}"
  [ ${#stale[@]} -eq 0 ] && break
done
[ ${#stale[@]} -ne 0 ] && rc=1
$Q node .github/ci/qualification.mjs check > /tmp/cv-qual.log 2>&1 \
  && echo "qualification ok" || { echo "FAIL qualification"; rc=1; }
echo "converge rc=$rc"
exit "$rc"
```

Never run this loop while `cargo xtask ci` is running: artifact
writes trip the gate's stability marker and abort the running phase.
The read-only packet checker is the only sanctioned gate overlap.

## 4. Accepted, documented costs

- The gate's 5g freshness proof (20–40 min) re-runs whenever anything
  in its NodeRuntimeOracle scope changes — including every `ratchets/`
  pin diff, i.e. once per CS train — and is phase-granular: a crash
  mid-proof re-pays it entirely. Check-side adoption would delete the
  keystone that makes write-side adoption sound; the proper fix is the
  post-H2.9 evidence-DAG sub-node receipts.
- Fresh observation (generator/typescript/lib-key change) has no
  mid-run checkpoint; bounded at ~10–20 min per witness set, ~57 min
  for 5g, and rare. Deferred to the kill-safe-checkpoint requirement
  of the new-CI design.
- Honest per-train arithmetic: converge rounds drop to ~1–2 min, but
  a CS train still pays the 5g proof plus the packet checker
  (~30–50 min wall with the read-only checker overlapped on the gate).

## 5. Acceptance

- Pin-only rebind of each adopted artifact in seconds; write log
  reports `adopted_cases`/`adopted_controls` and `oracle_runs_saved`
  (foundation validated end-to-end during development: fresh mint,
  then adoption at 1.3 s with `adopted_controls=6`, then a full
  re-observation `--check` byte-identical against the adopted output).
- Plain `--check` still re-observes fully (byte-compared run).
- SIGKILL during a write leaves the prior artifact bytes intact;
  an immediate re-run needs no manual cleanup.
- Interrupted-walk re-run converges in seconds, byte-identical.
- The divergence printer names the divergent component on the
  replayed shard-env incident (unit test).
- Full local gate green at the final head; no policy pins moved
  (`crates/xtask/src/main.rs` untouched).
