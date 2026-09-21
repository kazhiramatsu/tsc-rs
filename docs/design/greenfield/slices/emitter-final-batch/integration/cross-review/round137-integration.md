# Round137: ES5 variable producer correction

Actual Opus136 proposed an original link and one-sided for-of comment ranges. Root independently identified that set_original_node merges emit metadata, unlike the TS direct original assignment, and that the failing rename source has an erased annotation. The replacement owns its name comments but not that annotation; only emitInitializer reads the pre-substitution name's type end. Opus137 checked these distinctions and withdrew the earlier type and end-minus-one hypotheses.

The candidate now links only semantic provenance to the current replaced name (preserving intermediate metadata donors), and the VariableDeclaration initializer follows only print-order-marked replacements to the first ordinary name for its type cursor. Actual replacement trailing comments still read only replacement metadata. The raw cursor rule and shared comment/map collectors remain unchanged. A cycle yields no donor, consistent with bounded original traversal.

For-of non-pattern declarations give the list EndOnly and the statement StartOnly comment ranges through the existing typed helper; assignment heads get StartOnly on the statement. Pattern branches retain their old ranges.

These are candidate repairs, not qualification. Existing268 commands are preserved, with136 new ES5 commands covering both CommonJS/System, checked/noCheck, removeComments false/true, captured names, direct/aliased exports, typed/untyped comments, destructuring, converted/nonconverted loops, assignment and pattern heads, and Unicode/escaped names. Original380 intermediate TS observations fromr154 are also retained.

The read-only request preceded the local for-of patch and evidence fast-forward; Opus observed that ongoing source state. No source was edited by Claude. Native qualification is still pending.
