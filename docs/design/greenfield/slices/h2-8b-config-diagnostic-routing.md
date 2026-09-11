# Source-owned config diagnostic routing follow-up

Hosted acceptance at `15b6a6038` passed conformance 49024/49024 and H2.5g, then found three new H2.5h diagnostic divergences. The baseline is retained in `ratchets/h2-8b-config-diagnostic-routing-baseline.v1.json`; no qualification or known-divergence manifest changes.

`verifyCompilerOptions` creates TS1148/TS6131 with a source file (`_tsc.js:124874–124898`). The collection name `addConfigDiagnostic` does not determine the public getter: `getOptionsDiagnostics` selects global/config-file diagnostics (`124024–124036`), and source-owned entries join the semantic getter. `emitFilesAndReportErrors` (`129412–129442`) skips semantic diagnostics after option/global errors. The loader put these source-owned rows in `PreparedProgram.diagnostics().options()`, making them visible alongside deprecation errors.

The repair moves the same diagnostics, with unchanged code/span/text, into the existing source-owned Program diagnostic route. Existing compiler routing already supplies semantic and conformance streams from that collection. Eight new complete TypeScript commands observed twice cover the three failing source forms, config/deprecation controls for both TS1148 and TS6131, and noEmitOnError (which must still report source errors collected by emit). Tests also assert that the loader retains the source constraints outside the option bucket.

The original `h2-8b-config-completion-final.v1.json` remains evidence for its measured `ec509858e` input tree; this follow-up supersedes the loader routing at that head. The old receipt is not rewritten or relabelled. Current integrated validation and hosted acceptance are recorded in PR #515.
