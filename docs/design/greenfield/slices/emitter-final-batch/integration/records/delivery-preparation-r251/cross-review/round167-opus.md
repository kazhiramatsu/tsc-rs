The `h1-emit-profile.v1.json` artifact has no `crates/oracle/h1-emit-profile.mjs` generator, and nothing in ORDER re-checks it. So its internal runtime hashes are historical, unlike the H2 profile ladder. `h2-profile-transition.v1.json` is re-validated by `h2-transition` (ORDER position 9).

# Round 167: review of the post-merge D entry

This is read-only (HEAD `628562c31`, the r227 plan, the r248 owner extraction, and ORDER/generator/test sources). I qualify nothing. **Verdict: D stays isolated and the plan is sound, with three corrections**:
- replace the four absent owner names;
- resolve the ambiguous same-name owners explicitly;
- don't describe the 23 H2 profiles' runtime hashes as merely "historical", but do describe `h1-emit-profile` that way.

## 1. Owner identities for the 18 rows

**Source check:** r248's `source_sha256 = 1c59e77a54b186ec…` equals the SHA-256 of the vendored `vendor/typescript-6.0.3/lib/_tsc.js`.

**Label conventions:**
- `Paths` below abbreviates r248's lexical paths: `P/` = `<top>/createPrinter@116912:1/`, `C/` = `<top>/createProgram@122625:1/`, `K/` = `<top>/createTypeChecker@46438:1/`, `F/` = `<top>/transformClassFields@95852:1/`, `R/` = `<top>/<anonymous>@28914:2/` (the parser IIFE).
- **"†"** marks names r248 lacks. Each † name is confirmed as `function <name>(` at the listed `_tsc.js` line, but its body/declaration SHA-256 has **not** been computed. Compute it with the r248 script at final V before D cites a digest; don't use invented values.

| Row | Primary owners (lexical path from r248, or † line) | Ambiguity resolution |
|---|---|---|
| E-ENTRY | `C/emit@123568`, `C/emitWorker@123595`, `handleNoEmitOptions@125636`, `emitFiles@116530` | `emit` has 6 declarations: **not** `createPrinter/emit` (printer entry) and **not** the builder/build `emit`s (build/watch is out of scope). `emitWorker`: not `transformGenerators/emitWorker@109671`. |
| E-PLAN-SCRIPT | `getSourceFilesToEmit@16600`, `sourceFileMayBeEmitted@16617`, `getOutputPathsFor@116373`, `forEachEmittedFile@116312` | none |
| E-RESOLVER-IDENTITY-G | `getParseTreeNode@11426`, `K/getEmitResolver@47561`, `K/createResolver@88545` | `getEmitResolver`: the checker declaration, **not** the `transformNodes@116001` context accessor. |
| E-METADATA-BASE | `getOriginalNode@11400`, `setOriginalNode@25208`, `mergeEmitNode@25218`, `createNodeFactory/cloneNode@24436`, `propagateChildFlags@25110` | none |
| E-METADATA-G-CLASS | `transformTypeScript/getClassFacts@94410`, `transformNamedEvaluation@93950`, `getAssignedName@11566`, `F/visitClassDeclarationInNewClassLexicalEnvironment@96971`; add † `getFirstConstructorWithBody@16674` and † `transformTypeScript/transformClassMembers@94564` (the parameter-property selector correction) | `getClassFacts`: this row cites the **TypeScript** transform's; the class-fields one belongs to E-NAMES-CLASS-G. `transformClassMembers` also exists at 97143 (class-fields); cite 94564. |
| E-CAPTURE-BASE | `transformES2017@100810` | **Drop `transformNodes`**: it is the generic transformation driver, not a capture owner. |
| E-CHECKER-FACTS-BASE | `K/createResolver@88545`, `K/getEmitResolver@47561` | Record `K/getReturnTypeOfSignature@59810` as the **adjacent upstream semantic correction**, not a new owner of this row (the r150 decision). |
| E-NAMES-BASE | `P/generateName@120624`, `P/generateNameCached@120633`; replace absent `generateUniqueName` with † `makeUniqueName@120741`, † `makeTempVariableName@120703`, † `generateNameForNode@120876` | none |
| E-NAMES-CLASS-G | `F/getClassFacts@96844`, `F/visitClassExpressionInNewClassLexicalEnvironment@97049`, `…/createClassTempVar` (nested in the latter) | `getClassFacts`: the **class-fields** one. |
| E-HELPERS-IMPORT-STATE | `getExternalHelpersModuleName@27603`, `createExternalHelpersImportDeclarationIfNeeded@27613`, `P/emitHelpers@117719`, `transformNodes/requestEmitHelper@116243` | none |
| E-PRINTER-G | `P/emitParametersForArrow@119983`, `P/emitNodeListItems@120068`; add † `canEmitSimpleArrowHead@119979`, † `shouldEmitBlockFunctionBodyOnSingleLine@118999`, † `emitCaseOrDefaultClauseRest@119486`, † `emitIdentifierName@117149` (this train's function-name, block-layout and clause changes) | Move `pipelineEmitWithComments` to the comment rows. |
| E-COMMENT-SCOPE-H | `P/pipelineEmitWithComments@120978`; replace absent `emitNodeWithComments` with † `emitCommentsBeforeNode@120987` and † `emitCommentsAfterNode@120995` (the container-triple save/restore) | none |
| E-MAPS | `createTextWriter@16365`, `createSourceMapGenerator@92365`; replace absent `emitNodeWithSourceMap` with † `pipelineEmitWithSourceMaps@121277`, † `emitSourceMapsBeforeNode@121283`, † `emitSourceMapsAfterNode@121294` (the clause map-end ordering) | none |
| E-OUTPUT-FUTURE | `emitFiles@116530`, `writeFile@16644`, `writeFileEnsuringDirectories@16663` | `writeFile`: the top-level emit utility, **not** `createProgramHost/writeFile@129622`. |
| E-RECOVERY-FACTS | `R/nextToken@29502`, `R/parseList@30169`, `R/parseDelimitedList@30428`, `R/parseSemicolon@29770`; add † `parseErrorAtPosition@29467` (same-start dedup that grounds the retained-only fact) and † `abortParsingListOrMoveToNextToken@30356` (ListAbort skip in the class-member gap) | none |
| E-COMMENTS-G | `P/emitLeadingComments@121123`, `P/emitTrailingComments@121176`; add † `iterateCommentRanges@8491` (the CR/LF-versus-Unicode collector contract), † `forEachLeadingCommentToEmit@121219`, † `emitTokenWithComment@118731` (token progress), † `emitDetachedComments@16817`, † `emitComments@16794` (detached separators) | none |
| E-COMMENT-PHASES-A36 | `P/emitSpreadElement@118528`, `P/emitSourceFileWorker@119753`; replace absent `emitModifiers` with † `emitDecoratorsAndModifiers@119846` (the DecoratorPolicy source) and † `emitModifierList@119903` | none |
| E-COMMENT-ELLIPSIS-A37 | `P/emitParameter@117855`, `P/emitSpreadAssignment@119536`, `P/emitJsxExpression@119448` (JSX raw-position layout) | none |

**Keep the scope wording:**
- These are routing owners with branch proofs, **not** closures.
- The evidence column should cite scoped runtime evidence at `0336c566`:
  - r211 (1600 controls);
  - r212 (48 commands, 108 projects);
  - r213 (774 commands, emitter 1015, checker 1739);
  - the r185 parser proof (16994), for E-RECOVERY-FACTS;
  - r215/r224 retirement.
- Qualification itself rests only on the final-V unsplit CI plus hosted checks, and on V and M having identical trees.

## 2. The profile manifest: a 24-row table is sufficient, with the scope stated correctly

A **24-row table** (path and SHA-256 via `git show V:path`), plus `merge-base --is-ancestor V M` and `V^{tree} == M^{tree}`, is sufficient. It needs no ~1,000-row copy and no new JSON in D. Each artifact's `runtime_inputs` array is bound by the artifact's own SHA-256, so linking the artifacts is enough.

**Wording correction:**
- **The 23 H2 ladder profiles** (`h2-1a…h2-5g`) and `h2-profile-transition` are all rungs in ORDER, and their generators compute `pathHash` from **current** bytes. For example, `h2-5f-profile.mjs:63-64` and `:163` do this, and `--check` byte-compares.
  - So after the one converged walk at V, their `runtime_inputs` hashes **do** equal V's bytes. Saying "historical hashes" would understate them.
  - Any rung whose inputs changed is re-minted by the walk, and downstream historical-authority pins are repinned.
  - D should say: "each artifact as re-validated by the converged walk at V", and make no claim about pre-walk hashes.
- **`ratchets/h1-emit-profile.v1.json`:** no generator named `h1-emit-profile.mjs` exists and it isn't in ORDER, so its internal runtime hashes are **historical lineage**. List its SHA-256 at V, but label it historical, not current.
- **`h2-5g-profile`:** must be the freshly minted 921-path closure. The 921 count (and the H2.5g closure pin) must match V.

## 3. D stays isolated from every generator

- No code under `crates/`, `scripts/`, `.github/` or `ratchets/` refers to `architecture-validation.md` (`git grep`: none).
- **Glob and walk readers:** the only recursive ones walk `crates/` or `.cargo/` (the `run-*-design-experiment.py` scripts) or read specific capture directories or `.patch` files. None enumerates `docs/**/*.md`.
- **Repository digests:** `qualification.mjs` tree digests cover `vendor/`, `baselines/`, `tests/`, `ratchets/`, `crates/oracle/`, the crate test directories and `.github/ci/`, but not `docs/`.
- **The H2.5h trio** (owner graph, gap matrix, dispositions) parses row IDs and hashes **only** `emitter-architecture.md` (r141/r150).
- **Readiness envelopes** don't name this record, and the slice-readiness checker digests only `ready` packet paths.
- **The only integration-directory generator input** is `cross-review/recovery-inputs-r20.json` (`scripts/foundation_witnesses.py:22`, `scripts/observe-emitter-recovery.mjs:11`). That is a separate file.

So there is **no risk that needs a fix before the walk**, provided D changes only that one Markdown file (with `git diff --check`, link checks, and the README `STATUS` unchanged).

**Hard blockers:** none. Every "†" owner needs its body/declaration digest computed from the pinned source at final V before D is written.
