# H2.8a A6-7: preserve source-literal resolution context

Kind: runtime at checkpoint aff01a2a68ca884ae0a2299d9e423c431e6a40db. The complete native before
contains 48 commands: 36 exact twice and twelve failures (56.19 seconds,
exit 101). Readiness must pass before the production change.

The original ReexportedCjsAlias ES2015 command became red after A6-5 exposed
fresh alias-target queries. Its ES5 sibling remains exact. Source targets are
not the cause: omitted module computes CommonJS at ES5 and ES2015 at ES2015,
and TS 6.0.3 computes Bundler resolution for both. The explicit-module controls
isolate the difference: all JS/ES2015-module commands lose the second private
import, whereas JS/CommonJS and every TS named-import control match.

A6-7-1 changes the two checker fallback calls in compute_module_specifiers
from resolve_external_module_name(importing_file, literal, true) to the same
query with literal as both location and module-reference expression. The
literal's actual parent carries require/import/type-only mode information.
Using the source root discards that information: the first import's specifier
synthesis retries a require lookup with the file's ESNext mode, records an
exact-host missing request, and causes the later alias target query to return
no module. The existing require mode remains CommonJS when the original
literal is retained. No alias cache is substituted for the upstream fresh
getTargetOfAliasDeclaration query.

Two temporary trace runs confirm distinct local/export symbol identities,
both private aliases queued, the first BindingElement target found, and the
second module-symbol lookup returning None only in the ES2015 command. The
ES5 command resolves both targets. Both runs execute both complete original
commands twice and exit 101 with one divergent original. Trace logs are
in target/h2-8a-reexported-alias-trace.log (23.93 seconds) and
 target/h2-8a-reexported-alias-member-trace.log (17.79 seconds).
All temporary instrumentation has been restored to exact checkpoint bytes.

Allowed runtime path: crates/checker/src/node_builder/specifier.rs, only the
location argument at its two semantic fallback calls and the explanatory
comment. The existing direct prepared-provider attempt, host mode filters,
module-name ordering, cache keys, path construction, symbol identities,
serializer, printer, and authoritative failure propagation are unchanged.
This is the source-literal fallback of the existing native representation of
upstream cached module facts. It is not qualification of every optional host
capability or arbitrary mode override. Those broader H2.8b/API boundaries
retain their existing owners.

E-PROTOCOL, E-RESOLVER-BASE and E-METADATA-BASE are premise-unchanged but
rechecked. The existing borrowed checker and provider remain in one request;
location and literal are the same existing parse NodeId, and no new state,
callback, arena mutation, public resolver method or ownership transfer occurs.
The query uses the existing resolution_mode_for_usage and authoritative
provider; its failure handling is retained, rather than cleared or suppressed.

The observer crosses JS/TS, ES5/ES2015, explicit CommonJS, omitted module and
explicit ES2015, with class/function targets, reversed bindings and two renamed
imports of one target. All 48 TS commands are observed twice with full writes,
callback metadata, diagnostics, result presence, status and exit. Native exact
rows run twice; failed fresh rows report their first complete-comparison
mismatch. No input or expected result is edited after comparison. The immutable
before is ratchets/h2-8a-repeated-alias-before.v1.json; source/log copies are
outside the checkout at target/h2-8a-repeated-alias-before-location.txt.

The original pair remains a full-tuple mandatory projection of the immutable
809 original observations. Its previously recorded before is in
ratchets/h2-8a-class-original-before.v1.json, with the same divergence retained
in A6-6's current 21/22 after and both trace runs. Required adjacent checks are
all original require aliases (22), local/ambient alias witnesses (40), class
ordering witnesses (28), and existing checker statement/chain/specifier tests.
Whole769 replay and the rest of H2.8a–e remain open. No full qualification or
hosted acceptance is claimed by this bounded repair.

Pinned TypeScript 6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- computeModuleSpecifiers: 45493–45561, SHA256 `3b65f2d4af82611bfbf4a32c2099a5fe215cbfff1b4daa729393650ac0a7238d`.
- getResolvedModuleFromModuleSpecifier: 123014–123018, SHA256 `458cd49cbb91fb02ab389ace0e6b75feeee2df8db7cbb8a07acff388a05a26aa`.
- getModeForUsageLocationWorker: 122270–122289, SHA256 `3c08e885df01130696de78aeec7153c2eb24117ba6da00f7bfeebe8d91d0c52c`.
- getEmitSyntaxForUsageLocationWorker: 122290–122308, SHA256 `4922201818d439fdf20dd97dfe55d99061c8844612f5d2b50b92ef4de1a92545`.
- resolveExternalModule: 49473–49663, SHA256 `c60386d4343175ebc820e0dbc23c0c2fde8196c47265af66b3023e05432cc820`.
- getTargetOfImportSpecifier: 48959–48983, SHA256 `b70d0ba3fc6fdabec84fe6e33db05f4876baf99eea95b1091ec57298e4508601`.
- serializeAsAlias: 54707–54946, SHA256 `60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277`.
- getSpecifierForModuleSymbol: 53060–53109, SHA256 `cc081ccc9162d99c71cfb5013a0786210de8d66472567a9ee1d6eab90f686463`.

The after run passes all 48 repeated-target witnesses, all 40 existing
local/ambient alias witnesses and all 28 class ordering witnesses twice:
five tests, 126.25 seconds. The original binary passes three tests in 98.92
seconds: all 22 unique require-alias commands match twice, and the explicit
reexported pair plus ExportForms pair are also repeated in their separate
projections. Both binaries exit 0. Full writes and callback metadata remain
in the unchanged comparisons; expected results and membership are unchanged.
The combined log is target/h2-8a-repeated-alias-after.log. Source/log copies
are outside the checkout at the location recorded by
target/h2-8a-repeated-alias-after-location.txt. All 25 adjacent checker
statement/chain/specifier tests pass (0.02 seconds, exit 0;
target/h2-8a-repeated-alias-adjacent.log). These focused results do not replace
the full769 replay.
