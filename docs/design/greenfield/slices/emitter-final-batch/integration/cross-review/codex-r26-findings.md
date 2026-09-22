# r26 independent checks and corrections

The hosted conformance and H2.5h failures are repaired: all 49,024 diagnostics
match T0/T1/T2/T3 and H2.5h is 888 exact / 44 deferred / zero known.
The next failures expose historical consumer contracts.

H2.1a stops immediately on the private-name comment row (index 21 of the 49
source-deferred rows). Claude's statement that all 49 were executed and still
refuse is unsupported and incorrect: the loop propagates that error. The optional
chain assertion-comment row is index 37 and was not reached. Both use deleted
text guards and are routed through the existing complete original-observation
promotion comparator. Neither is considered exact before that comparison runs.
The four candidate promotions' frozen observations total four writes and ten
diagnostics. The historical H2.1a qualification stays immutable.

H2.6c's loader defines absence as zero known rows. Its writer is corrected to
remove an empty manifest, including an already-absent file, instead of producing
a state its own reader rejects. Fixed EF2/EF3 rows still execute in full even
when a live known manifest is absent.

retired-known.v1.json retained the original hashes but accidentally mutated all
three embedded before objects to empty cases when it emptied the live manifests.
retired-known.v2.json recovers the 12/1/8 original rows from base 3b1f5fe87 and
verifies each original SHA-256. Historical v1 is preserved with an explicit
correction. The separate checker archives contain 25+2 complete rows, resolution
four, and helper collision one; none of those archives is empty.

For bundle-declarations, replacing only its recorded h2-7de-observations input
hash produces bd63c9... exactly, the fresh hosted oracle's full retained payload
hash. The old frozen payload is b51176...; all case inputs/main/forced payloads
are unchanged. Refresh the official writer, then bundle-maps' downstream input
identity, with before snapshots and strict equality of all observation fields.
