# Final-gate maintenance discovered at r95

Recovery-next c1e699ab5 passed formatting; the read-only walk preflight reported24 surfaces. Its native test build and frozen census are unchanged. Gate-prep isolates maintenance from active measurements.

Eight Rust files contain D/E path/hash pins that require owner review on observation re-mint. They now use a verify-only class with explicit expected pair counts, covering adjacent literals, `workspace.join` literals, and locally defined named path constants. A mismatch cannot be repaired by `--fix`; an extractor/count failure exits2. Existing automatic repair of current-tracking pins is unchanged. File-local library/case fingerprints and the collector's env-selected inputs/cross-module qualification constant stay enforced by their existing runtime readers; this preflight does not claim those as covered.

Opus97 recommended verify-only checking. Its claim that h2_6c_de_promotions.rs contains only content hashes was incorrect: lines2008–2019 include three artifact pairs. All eight files are therefore checked, including that file. The three D/E artifacts were refreshed in c370e0241, so these are review-held pins, not a claim that the current artifact path can never be regenerated.

Four focused tests pass: drift/missing artifact rejection without rewrite, changed extractor shape rejection, joined/named-constant coverage, and rejection before current-pin fixups. The live preflight checks6 current-tracking and8 verify-only files successfully. Three earlier tests and receipts are also retained.

Pending: H2.8a candidates, candidate-inputs, and observations schema contracts/registration; policy execution-source and fuzz references; final full walk/gate. No artifact/source_commit/count/STAGE/qualification promotion was changed here. Opus97 confirmed the missing ORDER-rung schemas were an omission. Schemas must retain candidate/reference-only dispositions and zero runtime admission, and use hash patterns rather than embedded hash constants.


r98 adds the three missing schemas and registry entries. They validate all809 rows, the233-file shared project mount, original source identity, candidate/reference-only state, both observed repetitions, and the complete write/diagnostic/result/status/exit tuple. Raw compiler options/project descriptors stay producer-owned JSON objects. The local schema validator forbids recursive references; the reference artifact's related-information leaves therefore require null nested related-information, matching all809 stored commands, instead of introducing a validator feature change. Schema checks do not validate cross-artifact hashes or replace the existing producer/consumer verification.

All three original artifacts validate unchanged. Six selected Node tests pass, including mutation checks for false qualification, runtime admission, missing rows/provenance/tuple fields, wrong route, wrong generator, and fewer repetitions. The pin index passes with65 consumers/258 classified sites. Existing artifacts and generator files are byte-unchanged. Policy/fuzz pin refresh, the final walk and the full gate remain pending.
