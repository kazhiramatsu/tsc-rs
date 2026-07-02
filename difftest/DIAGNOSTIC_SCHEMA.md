# tsrs diagnostic JSON schema (`--diag-json`)

This is the stable, machine-consumable diagnostic format emitted by
`tsrs --diag-json`, intended as a backend for downstream tooling (linters,
LSP servers, editors, CI gates). It is byte-for-byte shape-compatible with the
tsc oracle in `difftest/diag_oracle.js`, so a tool can be validated against real
`tsc` and then switched to `tsrs`.

## Envelope

```jsonc
{
  "emittedFiles": ["main.js", "main.d.ts"], // files emit WOULD produce (names only)
  "emitSkipped": false,                     // whether emit was skipped
  "diagnostics": [ Diagnostic, ... ]        // sorted by (start, code, category)
}
```

`tsrs` does not emit yet, so `emittedFiles` is `[]` and `emitSkipped` is `false`
until the `noEmit:false` work lands. (The oracle populates them from real tsc.)

## `Diagnostic`

```jsonc
{
  "code": 2322,            // TSxxxx number — stable identity for rule config / suppression
  "category": 1,           // 0=warning, 1=error, 2=suggestion, 3=message (tsc DiagnosticCategory)
  "source": null,          // diagnostic source tag (null for core; set by plugins)
  "reportsUnnecessary": false, // true ⇒ render greyed-out (unused/unreachable)
  "reportsDeprecated": false,  // true ⇒ render struck-through (deprecated API)

  // ── position (see "Position conventions" below) ──
  "file": "main.ts",       // basename, or null for global/option diagnostics
  "start": 16,             // UTF-16 code-unit offset  (tsc / LSP convention)
  "length": 2,             // UTF-16 code-unit length
  "byteStart": 18,         // UTF-8 byte offset        (native, indexes the raw buffer)
  "byteLength": 2,         // UTF-8 byte length
  "startLine": 1, "startCol": 17,  // 1-based line, 1-based UTF-16 column (LSP-style)
  "endLine": 1,   "endCol": 19,    // span end — drives the ~~~~ underline

  "message": MessageChain,        // the (possibly nested) message
  "related": [ RelatedInfo, ... ] // secondary locations (DiagnosticRelatedInformation)
}
```

`reportsUnnecessary` / `reportsDeprecated` / `source` are currently always
`false`/`null` in `tsrs` (placeholders) and fully populated in the oracle; they
are part of the schema so consumers can rely on the keys today and get real
values once the compat work lands.

## `MessageChain`

A leaf message is `{ "text": "…" }`. A chain carries its own code/category and
children, matching tsc's `DiagnosticMessageChain` (rendered as indented
sub-lines under the head message):

```jsonc
{
  "text": "Argument of type 'X' is not assignable to parameter of type 'Y'.",
  "code": 2345,
  "category": 1,
  "next": [
    { "text": "Types of parameters 's' and 'n' are incompatible.", "code": 2328, "category": 1,
      "next": [ { "text": "Type 'number' is not assignable to type 'string'.", "code": 2322, "category": 1, "next": [] } ] }
  ]
}
```

## `RelatedInfo`

```jsonc
{ "code": 6500, "category": 3,
  "file": "main.ts", "start": …, "length": …, "byteStart": …, "byteLength": …,
  "startLine": …, "startCol": …, "endLine": …, "endCol": …,
  "message": MessageChain }
```

Used for "the expected type comes from property 'y' declared here", overload
candidate locations, the first declaration in a redeclaration, etc. A linter
surfaces these as jump-to-related-location targets.

## Position conventions (important for tooling)

Every span is reported in **two encodings**, because tooling needs both:

| field                | unit                | use                                            |
|----------------------|---------------------|------------------------------------------------|
| `start` / `length`   | UTF-16 code units   | tsc parity; LSP `Position.character` math       |
| `byteStart` / `byteLength` | UTF-8 bytes  | indexing the raw source buffer (Rust/native)    |
| `startCol` / `endCol`| UTF-16, 1-based     | display columns (match tsc's `(line,col)`)      |
| `startLine`/`endLine`| 1-based             | line numbers                                    |

The two diverge on any line containing non-BMP or multi-byte characters
(e.g. `𠮷` is 1 UTF-16-pair worth of 2 code units but 4 UTF-8 bytes). A consumer
that maps to VS Code / LSP positions should use the UTF-16 `start`/columns; a
consumer that slices the byte buffer should use `byteStart`/`byteLength`.

## Comparing against tsc

`difftest/diag_cmp.py <file>` runs both engines and reports `MISSING` /
`EXTRA` / `FIELDDIFF` (message chain and `related` compared by default;
`--strict` adds span end, category, source, and the hint flags). This is the
conformance loop for the diagnostic layer.
