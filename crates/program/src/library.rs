use std::path::{Path, PathBuf};

use tsc_host::to_file_name_lower_case;
use tsc_types::CompilerOptions;

use crate::config_options::{typescript_6_0_3_libraries, typescript_6_0_3_library_value};

/// An injected, version-pinned TypeScript standard-library catalog.
///
/// The catalog owns metadata only. Library bytes still come from the caller's
/// [`tsc_host::CompilerHost`], so memory and filesystem hosts observe the same
/// read, decode, lexical-identity, and failure contracts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryCatalog {
    directory: PathBuf,
}

impl LibraryCatalog {
    /// Construct the exact catalog shipped by the vendored TypeScript 6.0.3.
    ///
    /// `directory` is injected by the embedding application instead of being
    /// inferred from the process executable or a global installation.
    pub fn typescript_6_0_3(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub const fn logical_entry_count(&self) -> usize {
        typescript_6_0_3_libraries().len()
    }

    pub const fn distinct_file_count(&self) -> usize {
        95
    }

    /// Resolve one raw `compilerOptions.lib` key.
    ///
    /// The production [`CompilerOptions`] contract retains lowercased logical
    /// keys such as `es5` and `dom`; other spellings fail closed instead of
    /// loading bytes that the checker would interpret under different options.
    pub fn option_file_name(&self, value: &str) -> Option<&'static str> {
        if value != to_file_name_lower_case(value) {
            return None;
        }
        typescript_6_0_3_library_value(value)
    }

    /// Resolve the exact spelling admitted by `/// <reference lib="...">`.
    pub fn reference_file_name(&self, value: &str) -> Option<&'static str> {
        let normalized = to_file_name_lower_case(value);
        typescript_6_0_3_library_value(&normalized)
    }

    /// TypeScript's target-selected default library, including the ES2015
    /// `lib.es6.d.ts` compatibility quirk.
    ///
    /// tsc-port: targetToLibMap/getDefaultLibFileName @6.0.3
    /// tsc-hash: 7bb778cf3aca481496de2c0e1a073621a04f7f9cdcadd4ba837c16bd94544422
    /// tsc-span: _tsc.js:11240-11274
    pub fn default_file_name(&self, options: &CompilerOptions) -> &'static str {
        match options.emit_script_target().bits() {
            99 => "lib.esnext.full.d.ts",
            12 => "lib.es2025.full.d.ts",
            11 => "lib.es2024.full.d.ts",
            10 => "lib.es2023.full.d.ts",
            9 => "lib.es2022.full.d.ts",
            8 => "lib.es2021.full.d.ts",
            7 => "lib.es2020.full.d.ts",
            6 => "lib.es2019.full.d.ts",
            5 => "lib.es2018.full.d.ts",
            4 => "lib.es2017.full.d.ts",
            3 => "lib.es2016.full.d.ts",
            2 => "lib.es6.d.ts",
            _ => "lib.d.ts",
        }
    }

    /// tsc-port: getDefaultLibFilePriority @6.0.3
    /// tsc-hash: 76ba34e95562034f7cf2bde179f09ddac57adf36e403e26a589c8575d3759ae5
    /// tsc-span: _tsc.js:123124-123138
    pub(crate) fn priority(&self, directory: &Path, file_name: &Path) -> usize {
        if !file_name.starts_with(directory) {
            return typescript_6_0_3_libraries().len() + 2;
        }
        let basename = file_name
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if matches!(basename, "lib.d.ts" | "lib.es6.d.ts") {
            return 0;
        }
        let Some(name) = basename
            .strip_prefix("lib.")
            .and_then(|name| name.strip_suffix(".d.ts"))
        else {
            return typescript_6_0_3_libraries().len() + 2;
        };
        typescript_6_0_3_libraries()
            .iter()
            .position(|entry| entry.name() == name)
            .map_or(typescript_6_0_3_libraries().len() + 2, |index| index + 1)
    }

    pub(crate) fn spelling_suggestion(&self, value: &str) -> Option<&'static str> {
        let normalized = to_file_name_lower_case(value);
        let unqualified = normalized
            .strip_prefix("lib.")
            .unwrap_or(&normalized)
            .strip_suffix(".d.ts")
            .unwrap_or_else(|| normalized.strip_prefix("lib.").unwrap_or(&normalized));
        spelling_suggestion(
            unqualified,
            typescript_6_0_3_libraries()
                .iter()
                .map(|entry| entry.name()),
        )
    }
}

/// tsc-port: getSpellingSuggestion @6.0.3
/// tsc-hash: 37b9cd417fd83af45f9fa8584ae1a3aa05e3f7ac3764438bb0627a7d61591ab6
/// tsc-span: _tsc.js:951-975
fn spelling_suggestion(
    name: &str,
    candidates: impl IntoIterator<Item = &'static str>,
) -> Option<&'static str> {
    let name_text = name;
    let name_units = name_text.encode_utf16().collect::<Vec<_>>();
    let maximum_length_difference = 2usize.max(name_units.len() * 34 / 100);
    let mut best_distance = (name_units.len() * 40 / 100 + 1) as f64;
    let mut best_candidate = None;
    for candidate in candidates {
        let candidate_units = candidate.encode_utf16().collect::<Vec<_>>();
        if name_units.len().abs_diff(candidate_units.len()) > maximum_length_difference
            || candidate == name_text
            || (candidate_units.len() < 3 && candidate.to_lowercase() != name_text.to_lowercase())
        {
            continue;
        }
        let Some(distance) =
            levenshtein_with_max(&name_units, &candidate_units, best_distance - 0.1)
        else {
            continue;
        };
        best_distance = distance;
        best_candidate = Some(candidate);
    }
    best_candidate
}

fn lowercase_unit(unit: u16) -> Vec<u16> {
    match char::from_u32(u32::from(unit)) {
        Some(scalar) => scalar
            .to_lowercase()
            .flat_map(|lowered| {
                let mut buffer = [0u16; 2];
                lowered.encode_utf16(&mut buffer).to_vec()
            })
            .collect(),
        None => vec![unit],
    }
}

/// tsc-port: levenshteinWithMax @6.0.3
/// tsc-hash: eb79af510122ea8b6f14324bdd579dcdfeba2df1e60995fb199e415688f583df
/// tsc-span: _tsc.js:976-1016
fn levenshtein_with_max(left: &[u16], right: &[u16], maximum: f64) -> Option<f64> {
    let mut previous = (0..=right.len())
        .map(|value| value as f64)
        .collect::<Vec<_>>();
    let mut current = vec![0.0; right.len() + 1];
    let big = maximum + 0.01;
    for left_index in 1..=left.len() {
        let minimum_right = if left_index as f64 > maximum {
            ((left_index as f64 - maximum).ceil() as usize).max(1)
        } else {
            1
        };
        let maximum_right = if right.len() as f64 > maximum + left_index as f64 {
            (maximum + left_index as f64).floor() as usize
        } else {
            right.len()
        };
        current[0] = left_index as f64;
        let mut column_minimum = left_index as f64;
        current
            .iter_mut()
            .take(minimum_right)
            .skip(1)
            .for_each(|value| *value = big);
        for right_index in minimum_right..=maximum_right {
            let substitution =
                if lowercase_unit(left[left_index - 1]) == lowercase_unit(right[right_index - 1]) {
                    previous[right_index - 1] + 0.1
                } else {
                    previous[right_index - 1] + 2.0
                };
            let distance = if left[left_index - 1] == right[right_index - 1] {
                previous[right_index - 1]
            } else {
                (previous[right_index] + 1.0)
                    .min(current[right_index - 1] + 1.0)
                    .min(substitution)
            };
            current[right_index] = distance;
            column_minimum = column_minimum.min(distance);
        }
        current
            .iter_mut()
            .skip(maximum_right.saturating_add(1))
            .for_each(|value| *value = big);
        if column_minimum > maximum {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    (previous[right.len()] <= maximum).then_some(previous[right.len()])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LibraryCatalog;
    use tsc_types::CompilerOptions;

    #[test]
    fn typescript_6_0_3_catalog_pins_aliases_counts_and_target_defaults() {
        let catalog = LibraryCatalog::typescript_6_0_3("/vendor/lib");
        assert_eq!(catalog.logical_entry_count(), 107);
        assert_eq!(catalog.distinct_file_count(), 95);
        assert_eq!(catalog.option_file_name("es6"), Some("lib.es2015.d.ts"));
        assert_eq!(
            catalog.option_file_name("esnext.object"),
            Some("lib.es2024.object.d.ts")
        );
        assert_eq!(catalog.option_file_name("DOM"), None);
        assert_eq!(catalog.option_file_name("lib.dom.d.ts"), None);
        assert_eq!(catalog.reference_file_name("lib.dom.d.ts"), None);
        assert_eq!(
            catalog.default_file_name(&CompilerOptions::default()),
            "lib.es2025.full.d.ts"
        );
        assert_eq!(
            catalog.default_file_name(&CompilerOptions {
                target: Some(2),
                ..CompilerOptions::default()
            }),
            "lib.es6.d.ts"
        );
    }

    #[test]
    fn priorities_and_spelling_suggestions_match_the_pinned_order() {
        let catalog = LibraryCatalog::typescript_6_0_3("/vendor/lib");
        let directory = Path::new("/vendor/lib");
        assert_eq!(
            catalog.priority(directory, Path::new("/vendor/lib/lib.es6.d.ts")),
            0
        );
        assert!(
            catalog.priority(directory, Path::new("/vendor/lib/lib.es5.d.ts"))
                < catalog.priority(directory, Path::new("/vendor/lib/lib.dom.d.ts"))
        );
        assert_eq!(
            catalog.priority(directory, Path::new("/outside/lib.es5.d.ts")),
            catalog.logical_entry_count() + 2
        );
        assert_eq!(catalog.spelling_suggestion("es2050"), Some("es2015"));
        assert_eq!(catalog.spelling_suggestion("not-a-library"), None);
    }
}
