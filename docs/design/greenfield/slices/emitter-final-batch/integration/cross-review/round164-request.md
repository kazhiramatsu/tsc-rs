# Round 164: final retirement and gate ordering review

Continue actual Opus review. READ ONLY. No source edits, no Cargo/Node/build/test or generator execution. Read/Grep/Glob and read-only Bash only. Another serial runtime pipeline is running on frozen HEAD.

Canonical tree: /Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep
Current frozen HEAD 4dfc0fb4d3b3ca2364957c85d9ad84bd0a677dc2.
Runtime-qualified source 0336c56663ff243503a18987a2cf8021109e8295; same production Rust after current retirement. All r211 1600 controls exact twice; r212 all48commands (46emit+2noEmit),108projects exact twice; r213 all774 older commands exact twice,1015emitter and1739checker tests pass. Transpile289exact2known, raw101 solely6 stale-known retire assertions. Full127-artifact archive: docs/design/greenfield/slices/emitter-final-batch/integration/records/layout-controls-r211-r213-complete/manifest.json.

Current retirement commit 4dfc0fb4d:36liveparseKNOWNS ->0 and8transpileKNOWN ->2; historical observations retained in comparator mutation guards and explicit witness inputs. Diff parent and records/known-retirement-r222-r223. r223planner84PASS. r215 post-retirement runtime pipeline underway (36historicalIDs exact separately+3guards+9transpiletests+workspacealltargetsclippy).

Please independently examine:
1. Any weakening or incomplete retirement in these7livefiles, especially historical guards, live fixture counts, witness selection?
2. Last two transpile differences remain text/unicode (invalid escape recovery outside admitted profiles) and numeric-target/transform-100 (tsc ScriptTarget.JSON internal exception versus Rust typed refusal). Is either safely repairable inside the bounded qualified emitter changes, or is previous finite-scope disposition still justified? Check actual prior evidence and code; no speculative redesign.
3. Final pin/walk ordering: witness-coverage/inventory.py defaults v35, README linksv29. Need mint fresh immutablev36 at finalsource. Its READ set includes Rust test sources, manifests and replay/witness scripts, but no fixture bytes/genericratchetread. Could canonical chain-walk mutate any READ inputs (e.g. pin-audit --fix Rust test literals), making prewalk snapshot stale? Recommend correct ordering (default/README prewalk; immutable snapshot afterwalk if no profile dependency; verify exact generator inputs). Search actual source for input references. No masking hashes, no overwrite historical snapshots, no manual oracle loops, full unsplit CI remains mandatory.
4. Operational sequence now gate/walk in already-built Repair, then fast-forward publication Integration branch to exact final V and push PR561; full unsplit localgate on sameV. Postmerge D only architecture-validation.md 18 delegatedrows as agreed143. Any remaining contradiction? Main architecture no timeless pending wording; integrationREADME being updated as dated bounded observations beforewalk.

Give concrete file/line evidence and minimal corrections only; identify hard blockers separately. Do not treat historical pending records as current failures. Never claim total H2.9,build/watch,public re-emit,TS7 completion.
