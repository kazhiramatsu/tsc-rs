# H2.8a: unchanged H2.6c output inputs

The complete old 643-input acceptance passes at the A1–A4 implementation with
explicit output promotions: 631 exact, 8 known divergences, 4 deferred,
two executions per admitted input. The current divergence manifest shrinks
from 158 to 8; all retained entries are value-identical to the checkpoint
`4b0f4d75b5f79edcf93baca34c44b250a0d68710`. This is an old-band migration,
not a new H2.8a corpus/profile qualification or an H2.8a close.

| Evidence | Result and immutable identity |
| --- | --- |
| `target/h2-8a-legacy16-measurement.json` | 16 exact twice; SHA256 `3e1186d4b7b9712c77ba619042a7b992997df4ec3059b020d20ee8d9319a92a3`; 40 declaration members, 16 ordinary D requests, zero C/E requests |
| `target/h2-8a-output134-measurement.json` | 134 exact twice; SHA256 `88ae0f0db6329e2f124b2e8244cc166f52533760e06b583f6c40f429c2a947b5`; 294 declaration members, zero C/D/E requests |
| `target/h2-8a-output150-acceptance.log` | Full 643-input acceptance passed; 631 exact / 8 known / 4 deferred; 886 total declaration members |

The first 16 are original H2.6c D/output overlap inputs selected from the
completed old 177-input record (`56721751cdf2368c3af395a789de1aa1fc36203089030468748ab5e728e4ea3e`).
Their original case/input/expected-tuple identities remain frozen. They use the
existing D/E promotion reader, with H2.8a included in the original new-census
owner join. They replace 16 current outDir refusal migrations; the old
case-insensitive bundle remains the sole current refusal migration.

The other 134 are exactly the 130 old outDir and four old rootDir refusals in
the checkpoint's known-divergence manifest (SHA256
`03883d7a5abcba746b09907624315dbd872087e4ca2ecbe9e0659110747c0876`).
Their collector selects by those original identities and uses the existing
old preparation, host, scoped library reuse, command projection and full
mismatch comparator. It never substitutes newer D/E input tuples. The separate
`h2_6c_output_promotions.rs` registry pins case/input/expected-tuple hashes and
measured declaration counts; the ordinary runtime guard still requires zero
C/D/E activity for them. Missing, duplicate, deferred or divergent rows fail,
including in manifest-write mode.

The first full attempt after the 16-row registration stopped at
`commonSourceDirectory.ts#default`: its declaration count had become one,
while the old guard expected zero. The 134-row collector then measured every
remaining historical output refusal twice. Only after those complete results
were exact was their registry added. A second full 643-input run verified every
row before the existing writer persisted the 150-entry shrink. No diagnostic,
map, output byte, callback facet or retained mismatch vector was overwritten.

Suite totals from that completed run: compiler 199 = 189 exact + 8 known + 2
deferred; conformance 32 exact; project 410 exact; transpile 2 deferred. Current
refusals are one `isolatedModules` and one case-insensitive bundle. The six
other known rows keep their prior mismatch vectors. The H2.7c self-name package
input-remapping residual is outside this old 643-input success and remains the
next A5 implementation task.
