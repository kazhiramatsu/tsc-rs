# Verification artifacts

Repository-owned verification settings, accepted-state data, inventories and
evidence live here. Cargo files, toolchain pins and standard repository entry
files remain at the root. Prose design lives under
[docs/design](../docs/design/README.md).

| Location | Contents |
| --- | --- |
| [ratchet.toml](ratchet.toml) | Accepted diagnostic counts and reviewed verification ceilings |
| [STAGE](STAGE) | Historical milestone marker; relocation does not advance it |
| [escapes.toml](escapes.toml), [fn-dispositions.toml](fn-dispositions.toml), [nodes-missing-fields.txt](nodes-missing-fields.txt) | Reviewed implementation-debt and schema inventories |
| [diag-families.json](diag-families.json) | Diagnostic family definitions |
| [m8/](m8/) | M8 scope, emitter inventory/dispositions, evidence configuration and frozen owner plans |
| Other existing files | Subsystem observations, accepted state, pins and historical qualification records |

These root artifacts moved without changing their bytes. An old root filename
beginning with `m8-` now lives at `ratchets/m8/<filename>`; the other listed
filenames have the `ratchets/` prefix. Historical records retain their original
paths and hashes. Current tooling resolves the exact relocated names;
historical Git reads support the former root and older `tsrs2/` layouts and
reject ambiguous copies. Relocation does not refresh or requalify evidence.

The old M5/M6 test selections are preserved under
[docs/design/archive/milestone-canaries](../docs/design/archive/milestone-canaries/README.md).

Generated runs and scratch output belong under ignored `target/`. Commit
durable verification artifacts here or with their owning slice packet; keep
new project-specific artifacts out of the root. The
[current verification policy](../CLAUDE.md#current-verification-policy) applies;
a routine legacy chain walk or full local CI replay is not required.
