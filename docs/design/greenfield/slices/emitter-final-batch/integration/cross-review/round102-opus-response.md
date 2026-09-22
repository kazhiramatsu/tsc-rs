**Recommendation:** make all three changes as small boxing edits, and no lint allows are needed. That means your two proposals as written, plus option (A) for `ProgramLoadError`. The one real risk is a silent `Error::source()` downcast break, which I explain below. All sizes here are hand estimates for a 64-bit target; the later clippy run is the real check. I made no edits and ran no builds.

## Size estimates

Field sizes used: `PathBuf`, `String` and `JsString` (a `Vec<u8>`) are 24 bytes each, and wrapping them in `Option` adds nothing.

| Type | Now | After the change |
|---|---|---|
| `HostError` (private fields) | ≈80 | unchanged |
| `ResolutionError` (public enum, largest variant is `Host`) | ≈80 | unchanged |
| `PreparationError` | 160, matching the r9a log | ≈88 |
| `ProgramLoadError` `Host` variant | ≈136 | ≈64 |
| `ProgramLoadError` `Resolution` variant | ≈160, whole enum 168 as in the log | ≈88 |
| `ProgramLoadError` overall | 168 | ≈104, set by `Unsupported` (four 24-byte fields); `LimitExceeded` ≈72, `Decode` ≈64, `Preparation` ≈96 |

The ≈104 result leaves about 24 bytes of headroom below 128. `ConfigProgramLoadError::Program(..)` shrinks with it.

## 1. Checker `ExcessPropertyOutcome`: approve

- The lint here is `large_enum_variant`.
- `Option<Box<Diagnostic>>` is 8 bytes, and `None` doesn't allocate.
- There are three sites:
  - the definition at `engine.rs:1040`;
  - the only construction at `engine.rs:2442`, where you wrap the existing `diagnostic` binding with `.map(Box::new)`;
  - the only consumer at `check.rs:4759`, which becomes `diagnostic.map(|d| *d)`.
- The derived Clone/Debug/Eq still work, and `Box`'s Debug output is transparent.
- Nothing compares an outcome against a literal. It's `pub(crate)`, so no public API changes.
- The allocation only happens on the error-reporting path.

## 2. `PreparationError`: approve, with one trap

- Store `Option<Box<ResolutionError>>`, built with `Some(Box::new(error))` in `from_resolution_js`.
- `resolution()` becomes `self.resolution.as_deref()`, keeping the same public signature. The two `prepared_program_contract.rs` users (1517, 1540) stay unchanged.
- **Trap:** the current `Error::source` body (`.as_ref().map(|e| e as &(dyn Error + 'static))`) would **still compile** after the change. But it would then hand out `&Box<ResolutionError>`, so `downcast_ref::<ResolutionError>()` would quietly return `None`. It must be `.as_deref().map(|e| e as &(dyn Error + 'static))`.
- This alone should clear roughly 40 `result_large_err` sites (about 35 in `prepared.rs` plus `path.rs`, all at 160 bytes).

## 3. `ProgramLoadError`: choose (A)

**(C) can't reach the threshold without changing public types.**
- Boxing `HostError`'s private internals would bring `Host` down to about 64.
- But `ResolutionError` is a public enum whose `Canonicalization` variant is about 72 bytes. That keeps `Resolution` at about 152.
- Getting under 128 would mean changing public `ResolutionError`, which is a wider API change than (A).

**(B) is the broad suppression you asked to avoid, and it keeps the cost.**
- It needs allows on about 51 sites (about 41 in `loader.rs`, 9 in `config.rs`, 1 in `module_resolution.rs`) or module-level allows in 3 modules.
- It keeps a 168-byte `Result` flowing through about 50 loader functions.
- The repo's existing `large_enum_variant` allows are single-type allows with a stated reason, not a precedent for this.

**Why (A) is low-impact.**
- `tsc-rs-program` has no consumer of these variants outside its own crate.
- Kind, accessors, `Display` (Box forwards it) and the typed causes all stay the same.
- It only changes the two public `source` field types.

**What (A) touches:**
- **Construction:** only the private constructors `host` (`loader.rs:362`) and `resolution` (`:374`) build these two variants, with `Box::new(source)`. `host_js` and `resolution_js` delegate to them. The compiler will flag any construction site I missed.
- **`Error::source` (`loader.rs:548-550`):** the same trap as above. `Some(source)` compiles against a Box and breaks downcasting. It must be `Some(&**source)` or `Some(source.as_ref())` for both `Host` and `Resolution`.
- **Integration tests: 9 destructuring sites in 3 files.** The assertions stay as strong as before; only the code shape changes.
  - `assert_eq!(source, x)` becomes `assert_eq!(*source, x)` at:
    - `automatic_type_directive_loader_contract.rs:488, 529`;
    - `library_program_loader_contract.rs:904`;
    - `no_lib_program_loader_contract.rs:1371, 4888, 4916`.
  - `no_lib…:3620` needs `matches!(*source, …)`.
  - `no_lib…:269` can't destructure through a Box on stable. Split it into two `let … else` bindings with the same panic message.
  - `automatic…:596` also binds `source`; check its later use the same way.

## Checks to run

1. `cargo clippy -p tsc-rs-checker -p tsc-rs-program --all-targets -- -D warnings`, then the workspace clippy. Confirm zero `result_large_err` and `large_enum_variant` remain for these types, rather than trusting my estimates.
2. `cargo test -p tsc-rs-program`, including every `*_loader_contract` and `prepared_program_contract`, plus the checker library tests.
3. **Add one small focused test for the downcast.** I found no existing test that calls `downcast_ref` on `ProgramLoadError::source()` or `PreparationError::source()`, so the trap above would go unnoticed today. Assert that `downcast_ref::<HostError>()` and `downcast_ref::<ResolutionError>()` return `Some` for `Host`, `Resolution` and a resolution-carrying `PreparationError`. This covers the actual risk of this change, not a hypothetical one.
4. **Pin impact:** `loader.rs`, `error.rs` and `engine.rs` are each referenced by about 90–120 ratchet or oracle files. Any lint edit re-stales those, so they belong with the final-freeze pin refresh you already have queued. Nothing here affects diagnostics or recovery behaviour.
