I found no behaviour bug. Boxing `Host.source` and `Resolution.source` is the narrowest honest fix under `-D warnings` with no new lint allowances. It is a real source-compatibility break for those two public variants and must be stated as one. This is from source and diff reading at b451489e4 (the deb2f2ebf changes plus follow-ups); I made no edits, builds or tests.

## 1. Remaining behaviour bugs: none found

- **Error source chain and downcasting.**
  - `ProgramLoadError::source` (`loader.rs:546-550`) returns `source.as_ref()` for `Host` and `Resolution`. `Box<T>`'s only `AsRef` is `AsRef<T>`, so this returns `&HostError` / `&ResolutionError`, not `&Box<…>`.
  - `PreparationError::source` and `resolution()` use `as_deref()`, so the public return type `Option<&ResolutionError>` is unchanged.
  - The test at `loader.rs:~4600-4660` checks all of these:
    - `ProgramLoadError::Host` downcasts to `HostError`;
    - `ProgramLoadError::Resolution` downcasts to `ResolutionError`;
    - the next level down from a resolution is `HostError`;
    - a resolution-carrying `PreparationError` downcasts to `ResolutionError`;
    - `ProgramLoadError::Preparation` downcasts to `PreparationError`.
  - `prepared_program_contract.rs:1517, 1540` still use `resolution()` unchanged.
- **Equality, Clone and Debug.** The derives are `Clone, Debug, Eq, PartialEq` (`loader.rs:215`). `Box` compares and clones by content, and `Box`'s `Debug` is transparent, so `==`, clone and debug text are unchanged.
- **Display.** `write!(formatter, ": {source}")` (`:530`, and the `Resolution` arm) forwards `Display` through `Box`, so message text is unchanged. `compiler/src/cli.rs:885` consumes `ConfigProgramLoadError::Program(error)` through `Display`/accessors, so it is unaffected.
- **Allocation.**
  - `Box::new` happens only in the private constructors `host` (`:362`) and `resolution` (`:374`); `host_js` and `resolution_js` delegate to them.
  - It also happens in `PreparationError::from_resolution_js`.
  - The checker boxes only when an excess-property diagnostic is actually produced (`diagnostic.map(Box::new)`), and unboxes at the single consumer (`check.rs`).
  - `None` allocates nothing. All of these are failure or reporting paths, not success paths.
- **Checker `ExcessPropertyOutcome`.** It is `pub(crate)`, with one producer and one consumer, so its semantics are unchanged.

## 2. Is there a smaller fix? No

- **Lint allows** on the ~51 `Result<_, ProgramLoadError>` / `ConfigProgramLoadError` functions, or at module level, are excluded by your no-new-allowances rule. They would also keep the 168-byte `Err` type.
- **Keeping the public field types and shrinking internals** isn't enough:
  - Boxing `HostError`'s private internals leaves `ResolutionError` (a public enum; `Canonicalization` is about 72 bytes).
  - That keeps the `Resolution` variant around 152 bytes, above the 128-byte limit.
  - Going further means changing `ResolutionError`'s public shape, which is a wider API change.
- **`PreparationError` and the checker type** are already done without API impact (private field, `pub(crate)` type).
- So boxing exactly the two `source` fields is the minimal public change. `ProgramLoadError` is now about 104 bytes, set by `Unsupported`; this is a hand estimate, and the final Clippy gate is the real check.

## 3. Compatibility limitation to state in the final PR

- **`tsc-rs-program` public API change:**
  - `ProgramLoadError::Host { source }` changes from `HostError` to `Box<HostError>`.
  - `ProgramLoadError::Resolution { source }` changes from `ResolutionError` to `Box<ResolutionError>`.
- **Code that breaks:**
  - constructing either variant with a struct literal (now needs `Box::new(...)`);
  - binding `source` and using it as the unboxed type, e.g. `assert_eq!(source, host_error)`, which now needs `*source`;
  - nested patterns through the field, e.g. `Resolution { source: ResolutionError::Host(e), .. }`, which must now become two steps (`let … = *source`).
- **Unchanged:**
  - variant names;
  - `kind()`, `operation()`, `path()` and `js_path()`;
  - Display and Debug text;
  - equality;
  - `Error::source()`, which still returns `&HostError` / `&ResolutionError`, so downcasting keeps working.
- **Non-breaking internal changes:**
  - `PreparationError`'s private field is now `Option<Box<ResolutionError>>`, and the public `resolution()` signature is unchanged.
  - The checker's `pub(crate)` `ExcessPropertyOutcome` is internal.
- **Known in-workspace breakage:** nine destructuring sites in `tsc-rs-program`'s own integration tests were updated. No other crate matches on these fields.

Don't describe it as source-compatible. The accurate description is behaviour- and message-preserving, with a source-level break in those two variant field types.
