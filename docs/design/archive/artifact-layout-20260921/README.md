# Artifact relocation: preserved draft inputs

These are byte-for-byte copies of the M9 draft manifests at commit
`869bf21d15fe3a89be7475a846a799eab88f798e`, before the repository-root artifact
relocation. They preserve the original source paths and hashes.

The current files remain in `ratchets/`. Their input paths and the hashes of
the tooling changed by this relocation are updated together. All statuses,
checks, blockers and qualification claims remain unchanged: these are blocked
drafts, not fresh performance or compatibility qualification. No historical
evidence is regenerated. See [the location guide](../../../../ratchets/README.md).

- [Original domain draft](fuzz-domain.v1.toml)
- [Original preflight draft](fuzz-preflight.v1.json)

The [original hosted policy](qualification-policy.v2.json) is also preserved.
The current policy updates only the source hash for `crates/xtask/src/main.rs`
after the reviewed path changes. Its workflows, selected coverage, worker
limits and time budgets are unchanged. This is current input-pin maintenance,
not a refresh of historical profile qualifications.
