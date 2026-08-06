use super::compiler_version_satisfies;

#[test]
fn versioned_types_conditions_use_the_pinned_compiler_semver() {
    for range in [">=1", ">=6.0.3", "^6.0", "~6.0", "5 - 6", "<4 || >=6", ""] {
        assert_eq!(compiler_version_satisfies(range), Some(true), "{range}");
    }
    for range in [">=10000", "<4", "^5", "6.0.4 - 7", ">6.0.3"] {
        assert_eq!(compiler_version_satisfies(range), Some(false), "{range}");
    }
    for range in [">=", "not-a-version", "6..0"] {
        assert_eq!(compiler_version_satisfies(range), None, "{range}");
    }
}
