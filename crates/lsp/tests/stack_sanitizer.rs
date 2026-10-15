//! tsgo `stack_sanitizer_test.go` for Rust's backtraces: tsc-rs's frames
//! kept with their repository paths, every other frame redacted, and VS
//! Code's generic secret pattern defeated.

use tsc_lsp::stack_sanitizer::{defeat_generic_secret_regex, sanitize_stack_trace};

/// tsgo `TestSanitizedDebugStackTraceCompletionsRequest`: a debug build's
/// frames, with the user's checkout path.
#[test]
fn a_debug_stack_keeps_only_tsc_rs_frames_and_paths() {
    let input = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/17067e9ac6d7e98f18d4e7f6b0fc4b5ae8aa7d7e/library/std/src/../../backtrace/src/backtrace/libunwind.rs:116:5
   1: std::backtrace::Backtrace::force_capture
             at /rustc/17067e9ac6d7e98f18d4e7f6b0fc4b5ae8aa7d7e/library/std/src/backtrace.rs:312:9
   2: tsc_lsp::stack_sanitizer::capture_panic_stacks::{{closure}}::{{closure}}
             at /Users/someone/work/tsc-rs/crates/lsp/src/stack_sanitizer.rs:24:43
   3: tsc_ls::completions::<impl tsc_ls::LanguageService>::get_completion_data
             at ./crates/ls/src/completions.rs:1303:9
   4: tsc_lsp::server::Server::handle_completion
             at /home/me/crates/tsc-rs/crates/lsp/src/server.rs:1102
   5: core::ops::function::FnOnce::call_once
             at /rustc/17067e9ac6d7e98f18d4e7f6b0fc4b5ae8aa7d7e/library/core/src/ops/function.rs:250:5
   6: <alloc::boxed::Box<F,A> as core::ops::function::FnOnce<Args>>::call_once
   7: <tsc_lsp::queue::DynamicQueue<T>>::get
             at /Users/someone/work/tsc-rs/crates/lsp/src/queue.rs:40:13";
    assert_eq!(
        sanitize_stack_trace(input),
        "(REDACTED FRAME)
\t(REDACTED FRAME)
(REDACTED FRAME)
\t(REDACTED FRAME)
tsc_lsp::stack_sanitizer::capture_panic_stacks::{{closure}}::{{closure}}
\tcrates|>lsp|>src|>stack_sanitizer.rs:24
tsc_ls::completions::<impl tsc_ls::LanguageService>::get_completion_data
\tcrates|>ls|>src|>completions.rs:1303
tsc_lsp::server::Server::handle_completion
\tcrates|>lsp|>src|>server.rs:1102
(REDACTED FRAME)
\t(REDACTED FRAME)
(REDACTED FRAME)
<tsc_lsp::queue::DynamicQueue<T>>::get
\tcrates|>lsp|>src|>queue.rs:40"
    );
}

/// tsgo `TestSanitizedReleaseStackTraceCompletionsRequest`: a release build
/// has no source lines; anything that is not a frame is redacted too.
#[test]
fn a_release_stack_has_functions_only() {
    let input = "   0: std::backtrace::Backtrace::force_capture
   1: tsc_ls::completions::<impl tsc_ls::LanguageService>::get_completion_data
   2: tsc_lsp::server::Server::handle_completion
   3: __pthread_deallocate
note: Some details are omitted";
    assert_eq!(
        sanitize_stack_trace(input),
        "(REDACTED FRAME)
tsc_ls::completions::<impl tsc_ls::LanguageService>::get_completion_data
tsc_lsp::server::Server::handle_completion
(REDACTED FRAME)
(REDACTED FRAME)"
    );
}

/// tsgo `TestSanitizedStackTraceDefeatsVSCodeGenericSecretRegex`: each
/// trigger keyword before `(`, `[`, `.` or `|` takes `X_X`, in any case.
#[test]
fn the_generic_secret_pattern_is_defeated() {
    let input = "TypeScript|>tsc|>internal|>ls.(*LanguageService).getSignatureHelp()
\tTypeScript|>tsc|>internal|>ls|>signature.go:42
TypeScript|>tsc|>internal|>ls.LookupKey()
\tTypeScript|>tsc|>internal|>ls|>keys.go:7
TypeScript|>tsc|>internal|>ls.validateToken()
\tTypeScript|>tsc|>internal|>ls|>token.go:9
TypeScript|>tsc|>internal|>ls.signRequest()
\tTypeScript|>tsc|>internal|>ls|>sig.go:11
TypeScript|>tsc|>internal|>ls.setPwd()
\tTypeScript|>tsc|>internal|>ls|>pwd.go:13";
    assert_eq!(
        defeat_generic_secret_regex(input),
        "TypeScript|>tsc|>internal|>ls.(*LanguageService).getSignatureHelp()
\tTypeScript|>tsc|>internal|>ls|>signatureX_X.go:42
TypeScript|>tsc|>internal|>ls.LookupKeyX_X()
\tTypeScript|>tsc|>internal|>ls|>keys.go:7
TypeScript|>tsc|>internal|>ls.validateTokenX_X()
\tTypeScript|>tsc|>internal|>ls|>tokenX_X.go:9
TypeScript|>tsc|>internal|>ls.signRequest()
\tTypeScript|>tsc|>internal|>ls|>sigX_X.go:11
TypeScript|>tsc|>internal|>ls.setPwdX_X()
\tTypeScript|>tsc|>internal|>ls|>pwdX_X.go:13"
    );
    assert_eq!(
        sanitize_stack_trace("   1: tsc_ls::signature_help::get_signature_help\n             at ./crates/ls/src/signature.rs:3:1"),
        "tsc_ls::signature_help::get_signature_help\n\tcrates|>ls|>src|>signatureX_X.rs:3"
    );
}
