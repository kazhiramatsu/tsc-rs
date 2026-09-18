# r30: review concrete empty-block and registry retirement changes

Read-only review of /Users/hiramatsu/dev/tsc-rs-emitter-final-ci-repair working
diff. No edits, Cargo or subagents. Fable unless rate limited.

The printer change implements r29's agreed trailing-comments -> list space/line
-> leading-comments order. 72 adjacent TS commands (12 shapes x three targets
x removeComments on/off, maps on) repeat exactly; Rust comparisons are pending.

Registry changes are now concrete. Host case moved to HOST_CASE_SENSITIVITY in
h2_6c_de_promotions with owners [H2.7d,H2.8b], declaration_members=0 and C/D/E
requests=0/1/0 (these counters still require actual runtime validation).
Important correction: current H2.6c oracle correction changed BOTH the case
fingerprint and observation_input_sha256, because input.use_case_sensitive_file_names
now reflects the original directive. We pin corrected e60e5e... / 48e558...;
the old f860... / 2ab06... plus old refusal are archived separately. D/E
a7b13d... remains independently pinned. Do not change those historical artifacts.

Also projected totals still had isolatedModules:1, whose original EF3 command
is already exact x2. Added that original to output_promotions (no D/E request,
declaration_members=0), pinning old case/input/complete tuple. Both former
refusals now subtract their original buckets via exact promotions.

We did NOT conditionally skip the now-empty live refusal registry tests.
de_registry_contracts still has five unconditional tests. Records asserts zero
live migrations and both retired IDs are exact promotions. Identity mutations,
missing/duplicate/unknown originals, restored refused vectors on each retired
row, all existing promotion guards, zero current refusal totals and an absent
manifest round-trip are exercised. The old runtime-error fingerprint path has
no active row; its prior observed record remains archived, rather than creating
a fake live migration solely to keep that old test populated.

Please review actual diff for mistakes (including an overlooked caller expecting
nonzero migrations or a changed counter/tuple pin). Confirm the retirement guards
are nonvacuous for the current state, and identify only necessary corrections.
No native acceptance success is claimed yet.
