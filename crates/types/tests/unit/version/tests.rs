use super::compiler_version_satisfies;

// tsgo core.Version() is "7.1.0-dev": TypeScript's semver compares the
// version numbers, then the prerelease identifiers (a prerelease sorts below
// its release), without npm's same-tuple rule for prereleases.
#[test]
fn versioned_types_conditions_use_the_pinned_compiler_semver() {
    for range in [
        ">=1",
        ">=6.0.3",
        ">6.0.3",
        "^7.0",
        // a partial `>=` operand gets the `-0` prerelease (parseComparator)
        ">=7.1",
        ">=7.1.0-0",
        ">=7.1.0-dev",
        "6.0.4 - 7",
        "<4 || >=6",
        "",
    ] {
        assert_eq!(compiler_version_satisfies(range), Some(true), "{range}");
    }
    for range in [
        ">=10000",
        "<4",
        "^5",
        "^6.0",
        "~6.0",
        "5 - 6",
        ">7.1.0-dev",
        ">=7.2.0-0",
    ] {
        assert_eq!(compiler_version_satisfies(range), Some(false), "{range}");
    }
    for range in [">=", "not-a-version", "6..0"] {
        assert_eq!(compiler_version_satisfies(range), None, "{range}");
    }
}
