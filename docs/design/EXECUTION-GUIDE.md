# Execution guide for implementing agents (READ FIRST, FOLLOW EXACTLY)

This guide assumes you are an automated agent implementing one of the
designs in this directory. It is written so that you never need to make
a judgment call that isn't spelled out. When you hit a situation not
covered here or in the workstream doc, STOP (see "Stop conditions").

## The loop (never deviate)

Work in steps. A "step" is defined by the workstream doc. For EVERY step:

1. Make the change described in that step ONLY. Do not refactor
   neighboring code. Do not rename things. Do not fix unrelated issues
   you notice (write them into `docs/design/NOTES-<date>.md` instead).
2. Build: `cargo build --release 2>&1 | grep -E "^error" -A5`
   → must print nothing. If it prints errors, fix ONLY the compile
   errors. If you cannot fix them in 3 attempts, revert the step
   (`git checkout -- <files>`) and record a stop-note.
3. Test: `cargo test --release 2>&1 | grep "test result:"`
   → all suites `ok`, first suite `89 passed` (or higher if a step
   says to add tests). ANY failure: the step is wrong. Do not edit the
   failing test to make it pass — tests here pin tsc behavior. Revert
   and record a stop-note.
4. Run the step's OWN verification command (each step lists one) and
   compare against the step's "expect:" line.
5. `git add -A && git commit` with the message the step specifies.

After the LAST step of a stage: run the full gate:

```
./verify.sh golden-check        # minutes; run in background if you can
```

- `NEW FALSE POSITIVES: 0` → proceed (commit, then `./verify.sh golden-save`).
- `NEW FALSE POSITIVES: N>0` → do the FP triage procedure below.
- `NEW FALSE NEGATIVES: N>0` → triage the same way; a NEW_FN is
  acceptable ONLY if the workstream doc explicitly predicted it.

## FP triage procedure (mechanical)

For each file in "NEW FP detail":

1. Get the exact new positions (run from repo root):

```python
python3 - <<'EOF'
import json
def load(path):
    out={}
    for line in open(path):
        if '\x01' not in line: continue
        k,js=line.split('\x01',1)
        try: d=json.loads(js)
        except: continue
        out[k]={(x['code'],x['startLine'],x['startCol']) for x in d.get('diagnostics',[]) if x.get('file')=='main.ts'}
    return out
new=load('/tmp/golden_now.txt'); old=load('/tmp/golden_diag.txt')
NAME='FIXTURE_BASENAME.ts'   # <-- edit
for k in new:
    if k.endswith('/'+NAME):
        print(k)
        print('ADD', sorted(new[k]-old.get(k,set())))
        print('RM ', sorted(old.get(k,set())-new[k]))
EOF
```

   CAUTION: match `endswith('/'+NAME)` exactly — several fixtures are
   name-prefixes of others.
2. `python3 scripts/probe.py <full fixture path>` and find the ADDed
   position in the output. Above the `--- tsc:` divider = what tsrs
   emits; below = the oracle. A `*` marks one-sided lines.
3. Reduce to a micro-fixture: copy the minimal declarations + the one
   failing statement into `/tmp/scratch/mX.ts`, keeping the fixture's
   `// @...` directives on top. Probe the micro. If the micro does NOT
   reproduce, add surrounding statements from the fixture one at a time
   until it does (order matters in this codebase).
4. Diagnose against the workstream doc's "expected failure modes"
   table. If your diagnosis matches a listed mode, apply its listed
   fix. If not → stop-note.

## Stop conditions (produce a note and halt the workstream)

Write `docs/design/NOTES-<UTCdate>-<workstream>.md` containing: the
step you were on, the exact command outputs (build/test/gate), your
micro-fixture, and your one-paragraph diagnosis. Then stop. Conditions:

- A test in `cargo test` fails and the step didn't predict it.
- The gate shows NEW_FP in a file the workstream doc doesn't mention,
  and the micro-fixture diagnosis doesn't match any listed failure mode.
- You need to change a file the workstream doc doesn't list.
- Two consecutive gate rounds do not reduce the NEW_FP count.
- Anything requires "interpreting" tsc behavior without an oracle probe.

## Hard prohibitions

- NEVER edit files under `ts-tests/`, `oracle/`, or `/tmp/golden_diag.txt`.
- NEVER change `scripts/parallel_classify.py` / `full_conformance_compare.py`
  or set `TSRS_CLASSIFY_JOBS` / `TSRS_JOBS`.
- NEVER weaken an existing diagnostic to silence a NEW_FP; the fix is
  always in the specific mechanism the triage identifies.
- NEVER commit when the gate has NEW_FP > 0 (exception: intra-branch
  commits on a feature branch when the workstream doc says the SERIES
  is gated at the end).
- NEVER answer a semantics question from memory of TypeScript. Write a
  micro-fixture and probe it. The oracle is the truth.

## Micro-fixture cheat sheet

```
// @target: es2015          <- default used across the corpus
// @strict: true            <- add explicitly when testing strict behavior
// (batch base options are strict unless the fixture overrides)
```

Probe: `python3 scripts/probe.py /tmp/scratch/mX.ts`
Direct run (options may differ from batch — prefer probe):
`./target/release/tsrs --check /tmp/scratch/mX.ts`
Full diagnostic JSON incl. message chains:

```
echo /tmp/scratch/mX.ts > /tmp/scratch/list.txt
./target/release/tsrs --check-batch /tmp/scratch/list.txt | python3 -c "
import sys,json
line=sys.stdin.readline(); p,js=line.split('\x01',1); d=json.loads(js)
for x in d['diagnostics']: print(json.dumps(x,ensure_ascii=False)[:400])"
```

## If /tmp was wiped (fresh machine/reboot)

```
bash scripts/bootstrap.sh          # provisions /tmp fixtures + chunk lists
./verify.sh golden-save            # golden MUST correspond to current HEAD
```

Do this BEFORE any gate run; a stale or missing golden makes every
classification meaningless.
