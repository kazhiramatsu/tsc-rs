// The command line sets process-wide checker modes (tsc reports no
// suggestion diagnostics, JSDoc is parsed for type errors only), so its
// contract runs in a test process of its own.
#[path = "integration/system_contract.rs"]
mod system_contract;
