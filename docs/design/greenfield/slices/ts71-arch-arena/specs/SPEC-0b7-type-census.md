# SPEC-0b.7 — TypeData-kind census (observation build, not merged)

Track: [ts71-arch-arena](../README.md), phase 0b. The gen2 REPORT's per-Type-kind byte table has no populations because the B-3
census counted types only in aggregate (2,575,521 on VS Code). Lane rules as [SPEC-arena](SPEC-arena.md) (no commits; patches under
`target/lane-census/patches/`; `STATUS.md`; offline; `nice -n 20 cargo … -j 2`; one cargo command at a time; time box one working
day). Builds allowed: `cargo build -p tsc-rs-cli` in dev profile only (the integrator builds release and runs the census in a quiet
window); `cargo test -p tsc-rs-types`.

## Deliverable

A patch on main `aefd66ce6` that, under the existing `TSRS_MEMORY_REPORT` output (`crates/types/src/tables.rs` `memory_usage` and
the CLI's report printing), adds one JSON object per checker at check end:

```json
{"type_kinds": {"Object": n, "Intrinsic": n, "Literal": n, "Enum": n, "UniqueESSymbol": n, "Union": n, "Intersection": n,
 "EvolvingArray": n, "Reference": n, "TupleTarget": n, "TypeParameter": n, "TemplateLiteral": n, "StringMapping": n, "Index": n,
 "IndexedAccess": n, "Mapped": n, "ReverseMapped": n, "Conditional": n, "Substitution": n, "GenericType": n},
 "mapped_cold_materialized": n, "conditional_cold_materialized": n, "total": n}
```

- `type_kinds` counts by `TypeData` variant over every type the checker created (the sum equals the Types row).
- `mapped_cold_materialized` = Mapped types with any of `mapped_modifiers_type`, `mapped_contains_error`, `mapped_apparent_type`
  set in `TypeLinksCold`; `conditional_cold_materialized` = Conditional types with any of the four conditional cold fields set
  (names in the gen2 REPORT "Mapped cold fields / Conditional cold fields").
- The shape must match `crates/records-preview/tools/README.md` on branch `arch/gen2-preview` (`--type-populations`).
- Zero cost when `TSRS_MEMORY_REPORT` is unset (the count is a walk over the type store at report time, not a counter in
  `create_type`).
- A unit test on a small program (a few types of several kinds) that the counts sum to the Types row.

## Acceptance (integrator)

Release build in a quiet window; VS Code and TypeScript compiler 1-thread runs; the JSON fed to `memory_report.py
--type-populations` reproduces the aggregate; the resulting Type byte totals recorded in the design §0.5.
