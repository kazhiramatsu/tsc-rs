# Compatibility

tsc-rs follows TypeScript 7.1's `tsc` (the native compiler, `tsgo`): the same options, diagnostics and output files. This
page records how that is checked, and the differences that remain.

## Status

`main` follows TypeScript 7.1 at commit `aa814927`, vendored under
`vendor/typescript-native/`. This work is in progress; so far:

- **Conformance:** in TypeScript 7.1's own test suite, every compared
  configuration (13,577 of 15,339; `tsgo`'s runner skips the others)
  matches `tsgo`'s baselines: diagnostics, emitted files, source maps,
  module resolution traces, `.types` and `.symbols`, and content mapper
  output.
- **Real projects:** `tsc-rs --noEmit` printed the same output as `tsgo` on
  all 9,067 DefinitelyTyped packages, all 2,756 azure-sdk-for-js
  configurations and all 38 material-ui projects (October 2026, one
  checker). On the 157 tsconfigs of astro, remix, nuxt, SolidJS and svelte,
  the diagnostics and the emitted JavaScript and declaration files are the
  same as `tsgo`'s (October 17, 2026).
- **Language server:** in 4,583 of the 4,584 sessions of TypeScript 7.1's
  language server tests, tsc-rs answers exactly as `tsgo` does; `tsgo`'s
  own answer to the last one varies from run to run (October 15, 2026). In
  generated sessions over eight real projects (hono, zod, effect, the
  TypeScript compiler, playwright-core, vue, next.js and VS Code's
  `src/vs/base`; 653,464 requests), 69 answers differ (hono 18, vue 1,
  playwright-core 50): 68 completions whose auto-import module specifier
  `tsgo` itself picks differently from session to session (tsc-rs gives its
  most frequent answer, and where one of `tsgo`'s answers names a module
  that does not export the symbol, the one that does) and one hover whose
  display depends on which files `tsgo`'s checker saw first (October 17,
  2026). In a session over svelte's JavaScript sources (368 files, 61,659
  requests) 49 answers differ: 47 completions whose auto-import module
  specifier depends on the order in
  which `tsgo` fills its specifier cache in a file's first completion (it
  varies between runs), one hover that depends on which files `tsgo`'s
  checker saw first, and one definition where `tsgo` reports an empty range
  for a `@this` parameter; a session over SolidJS's sources (23 files, 3,824
  requests) answers identically; sessions over astro's `packages/astro/src`
  (680 files, 113,109 requests), nuxt's `packages/*/src` (507 files, 85,414
  requests) and remix's `packages/*/src` (796 files, 118,095 requests)
  differ in 13 answers, all nuxt completions whose auto-import module
  specifier `tsgo` itself picks differently from run to run; a session over
  webpack's `lib/**/*.js` (790 JavaScript files, 134,174 requests) differs in
  55 answers after eight rounds of fixes: 43 hovers whose
  `Array<T>.length` qualifier depends on the session's order, 3
  signature-help labels whose union members `tsgo` orders by first use,
  one file's diagnostics whose inferred type argument differs, 5
  references of an `@import` default name `tsgo` misses and 3 `@this`
  definitions `tsgo` answers at an empty range; sessions over
  preact (33 files, 5,307 requests), SvelteKit's `packages/kit/src` (382
  files, 50,333 requests), ESLint's `lib` (389 files, 68,905 requests) and
  prettier's `src` (541 files, 79,819 requests) differ in 3, 1, 1 and 3
  answers; sessions over NestJS's `packages` (1,000 files, 144,763
  requests), RxJS's `packages/rxjs/src` (231 files, 41,628 requests),
  Excalidraw's `packages/excalidraw` (501 files, 85,695 requests) and
  puppeteer-core's `src` (214 files, 36,954 requests) differ in 1, 0, 77
  and 0 answers after the ninth round of fixes, the 77 Excalidraw
  completions whose auto-import source `tsgo` lists only once its session
  has seen the package's own index (October 18, 2026).
- **TypeScript 7 rules:** the default `target` is ES2026, and the options
  TypeScript 7 removed (`baseUrl`, `outFile`, `target: "es5"`, the `amd`,
  `umd` and `system` modules, …) are errors.

tsc 6.0.3 compatibility is complete in release
[`v0.1.0`](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0)
(branch `release/6.0.3`), with the limitations listed in
[its README](https://github.com/kazhiramatsu/tsc-rs/blob/v0.1.0/README.md#current-limitations)
(for example, no watch mode or `--build`).

## Limitations

- The [language server](usage.md#language-server-experimental) is incomplete and
  untested in editors.
- `-b` builds projects one at a time.
- `--diagnostics` prints fewer statistics than `tsgo`.
- Memory grows with the number of checkers.
- `tsc-rs --api --pipe` does not support Windows named pipes.
- `--pprofDir` (and the language server's profile commands) writes CPU
  profiles only, on Unix; heap and allocation profiles are not supported.
