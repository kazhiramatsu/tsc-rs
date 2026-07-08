#![forbid(unsafe_code)]

pub use tsrs2_checker::{check_program, CheckResult, CompilerOptions, InputFile};

pub fn check_empty_program() -> CheckResult {
    check_program(&[], &CompilerOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_reaches_checker_api() {
        assert!(check_empty_program().diagnostics.is_empty());
    }
}
