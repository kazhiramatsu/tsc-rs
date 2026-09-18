Please independently re-check the r37 diagnosis against the actual code, read-only. Do not make edits. Fable remains preferred; user authorizes Opus only on an actual rate limit.

We cannot accept the proposed identical typed comment range yet:
- CI repair builtins.rs create_module_export_assignment already set_text_range(access, target).
- printer.rs comment_range_for_node explicitly falls back to raw pos/end, not original. Access has no evident explicit range. Setting an identical typed range would do nothing.
- vendor _tsc.js cloneIdentifier/cloneNode does not itself copy pos/end either. Please correct the mistaken premise and show actual provenance.
- Rust parsed Identifier for full original someMethod has raw pos392/end437 (includes JSDoc), BindingElement same. /tmp/emitter-r37-syntax-probe confirms.
- Full source/output /tmp/emitter-jsdoc-r37; native cached CLI built from audit B reproduces missing comment5, d.ts exact. This is clean syntax and not the empty-block comment change.
- Code to inspect /Users/hiramatsu/dev/tsc-rs-emitter-final-ci-repair. All216 TS oracle controls complete now. No production fix applied.

Trace actual transformation producer and printer phases/emit flags/suppression including deferred expression scopes; find the real reason leading phase does not emit. Suggest smallest structural fix, plus re-export wrapper implications. Validate claims against actual TS transformation inspection if necessary. We will implement/test locally.
