//! The `.trace.json` baseline: the `--traceResolution` lines of the Program
//! as tsgo's harness records them (`TracerForBaselining`,
//! testutil/harnessutil/harnessutil.go:536-605, and
//! `DoModuleResolutionBaseline`, tsbaseline/module_resolution_baseline.go).
//! Every line is sanitized (`sanitizeTrace` with `usePackageJsonCache`):
//! the compiler version becomes `FakeTSVersion`, and the lines about a
//! file's existence take the plain form the first time the file is
//! mentioned and the "according to earlier cached lookups" form afterwards,
//! whichever form the resolver emitted. The lines end with LF.

use std::collections::HashSet;

/// tsgo `harnessutil.FakeTSVersion`.
const FAKE_VERSION: &str = "FakeTSVersion";

/// Renders the record of `lines`; empty when there is no line (tsgo writes
/// no baseline then).
pub(super) fn render(
    lines: &[String],
    current_directory: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let mut sanitizer = Sanitizer {
        current_directory,
        use_case_sensitive_file_names,
        package_json_cache: HashSet::new(),
    };
    let mut text = String::new();
    for line in lines {
        text.push_str(&sanitizer.sanitize(line));
        text.push('\n');
    }
    text
}

struct Sanitizer<'a> {
    current_directory: &'a str,
    use_case_sensitive_file_names: bool,
    /// tsgo `packageJsonCache`: the files whose existence a line reported.
    package_json_cache: HashSet<String>,
}

impl Sanitizer<'_> {
    /// tsgo `tspath.ToPath`: the key of a file name.
    fn path_key(&self, file: &str) -> String {
        let absolute = super::absolute(self.current_directory, file);
        if self.use_case_sensitive_file_names {
            absolute
        } else {
            absolute.to_lowercase()
        }
    }

    fn sanitize(&mut self, message: &str) -> String {
        let version = format!("'{}'", tsc_types::TYPESCRIPT_VERSION);
        if message.contains(&version) {
            return message.replacen(&version, &format!("'{FAKE_VERSION}'"), 1);
        }
        if let Some(rest) =
            message.strip_suffix("' does not exist according to earlier cached lookups.")
        {
            let file = rest.strip_prefix("File '").unwrap_or(rest);
            if !self.package_json_cache.insert(self.path_key(file)) {
                return message.to_owned();
            }
            return format!("File '{file}' does not exist.");
        }
        if let Some(rest) = message.strip_suffix("' exists according to earlier cached lookups.") {
            let file = rest.strip_prefix("File '").unwrap_or(rest);
            if !self.package_json_cache.insert(self.path_key(file)) {
                return message.to_owned();
            }
            return format!("Found 'package.json' at '{file}'.");
        }
        if let Some(rest) = message.strip_suffix("' does not exist.") {
            let file = rest.strip_prefix("File '").unwrap_or(rest);
            if self.package_json_cache.insert(self.path_key(file)) {
                return message.to_owned();
            }
            return format!("File '{file}' does not exist according to earlier cached lookups.");
        }
        if let Some(rest) = message.strip_prefix("Found 'package.json' at '") {
            let file = rest.strip_suffix("'.").unwrap_or(rest);
            if self.package_json_cache.insert(self.path_key(file)) {
                return message.to_owned();
            }
            return format!("File '{file}' exists according to earlier cached lookups.");
        }
        message.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_mentions_are_plain_and_later_ones_cached() {
        let lines = [
            "File '/p/node_modules/a/package.json' does not exist.".to_owned(),
            "File '/p/node_modules/a/package.json' does not exist.".to_owned(),
            "Found 'package.json' at '/p/package.json'.".to_owned(),
            "File '/p/package.json' exists according to earlier cached lookups.".to_owned(),
            "File '/p/a.ts' does not exist according to earlier cached lookups.".to_owned(),
            "File '/p/a.ts' does not exist.".to_owned(),
        ];
        assert_eq!(
            render(&lines, "/p", true),
            "File '/p/node_modules/a/package.json' does not exist.\n\
             File '/p/node_modules/a/package.json' does not exist according to earlier cached lookups.\n\
             Found 'package.json' at '/p/package.json'.\n\
             File '/p/package.json' exists according to earlier cached lookups.\n\
             File '/p/a.ts' does not exist.\n\
             File '/p/a.ts' does not exist according to earlier cached lookups.\n"
        );
    }

    #[test]
    fn the_version_becomes_the_fake_one_once() {
        let line = format!(
            "'package.json' has a 'typesVersions' entry '>=3.1' that matches compiler version '{}', looking for a pattern to match module name 'x'.",
            tsc_types::TYPESCRIPT_VERSION
        );
        assert_eq!(
            render(std::slice::from_ref(&line), "/p", true),
            "'package.json' has a 'typesVersions' entry '>=3.1' that matches compiler version 'FakeTSVersion', looking for a pattern to match module name 'x'.\n"
        );
    }

    #[test]
    fn case_insensitive_keys_share_the_cache() {
        let lines = [
            "File '/p/A.ts' does not exist.".to_owned(),
            "File '/p/a.ts' does not exist.".to_owned(),
        ];
        assert_eq!(
            render(&lines, "/p", false),
            "File '/p/A.ts' does not exist.\nFile '/p/a.ts' does not exist according to earlier cached lookups.\n"
        );
        assert_eq!(render(&[], "/p", true), "");
    }
}
