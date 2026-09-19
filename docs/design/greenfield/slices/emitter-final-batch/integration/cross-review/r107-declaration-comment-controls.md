# Bundle versus per-file declaration name comments

These three new fixtures are controls for the r104/r105 no-initializer name phase. They do not change production source. TypeScript 6.0.3 observations repeat identically; native runs are pending.

Each command contains typed initialized and uninitialized variables with JSDoc comments after the name and type. The controls include an AMD ES5 module bundle, an ES2017 script bundle, and CommonJS ES5 per-file output. JavaScript, declarations, both maps and the full command tuple are retained.

Actual Opus107 withdrew the proposed blanket `!declaration_syntax` guard. Bundle JS disposal deliberately retains parsed `type_node` metadata for declaration emission. The d.ts declaration derived from an initialized source has a later container end, so TypeScript prints the type-side JSDoc before the colon as well as at the retained declaration end. For an uninitialized source, the container end suppresses the extra occurrence. Per-file disposal clears this metadata. Keep the existing guarded range ownership; the controls verify both assumptions in native output.

Reference: `round107-opus-response.md`. Preserve Recovery e80dfc18f throughout its active native battery. Register and run these controls only after that battery finishes; no current result is reclassified as a test of these three rows.
