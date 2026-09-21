# Inherited lint repair candidate

Prepared in the isolated Lint-prep worktree from663da193844047b951bb9035787e914d7372831f while Recovery-next native qualification runs. No program/checker source is changed in the measured tree. No fresh clippy or native result is claimed yet.

The producer's r11 checker log contains143 diagnostics plus two compile-failure summaries, rather than145 independent lint sites. The r9a program log contains144 diagnostics, including91 result_large_err reports. These historical logs are a repair guide; fresh compiler/lint results are still required.

187 identity conversions were removed using the exact diagnostic expressions and matching source lines, including five multiline/map cases reviewed separately. The mapping report records each source edit. Other mechanical repairs use is_empty, direct filter_map functions, rfind, nth, and remove one needless borrow. The original diagnostic contents, strings, order and recovery predicates are unchanged by these operations.

Codex and actual Claude Opus102 selected narrow boxing rather than broad lint suppression:

- The internal checker ExcessPropertyOutcome stores Option<Box<Diagnostic>>, allocating only when a diagnostic exists, then returns the same owned Diagnostic at its sole consumer.
- PreparationError privately stores Option<Box<ResolutionError>>. Its public resolution() return type and typed error cause are preserved.
- ProgramLoadError's public Host.source and Resolution.source fields now use Box. This is a source-level type change for callers directly constructing/destructuring those variants. Its private constructors, kind/operation/path accessors, Display, and underlying Error::source dynamic types remain equivalent. The repository's nine affected loader test patterns still make the same assertions after explicit dereference.

No lint allows were added. Boxing HostError's private payload alone would not shrink the public ResolutionError canonicalization variant enough, so it would require wider changes. Keeping the two field changes localized avoids that redesign.

A focused regression checks that Error::source still downcasts to HostError, ResolutionError and PreparationError through the cause chain, and that the existing resolution() accessor returns the same resolution value. Merely casting &Box<Error> would compile while breaking this contract; both affected Error implementations explicitly expose the inner error.

Pending: fresh program/checker all-target clippy, program tests including loader/prepared contracts, checker library tests, and eventual workspace gate. Any newly reported style errors must be fixed before integration. The source/ratchet pin refresh remains at the final freeze.
