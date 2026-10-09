//! The tsc and tsc -b tests (tsgo `internal/execute/tsctests`): each
//! scenario runs the command line over an in-memory system, then applies its
//! edits one at a time and runs again, and its baseline records the inputs,
//! every run's output and exit status, the file system's changes, and the
//! incremental programs' state (runner.go `tscInput.run`).
//!
//! The scenarios are the Go tests' own values and edit closures, recorded by
//! scripts/tsctests_scenarios.py into the profile's
//! `tsctests-scenarios.json`: the files, arguments, environment and
//! terminal, and each edit's file operations in order.

use std::path::Path;

use serde::Deserialize;

mod build_info;
mod patience;
mod system;

use system::TestSystem;

/// One recorded scenario.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Scenario {
    /// The baseline's path below `baselines/reference`:
    /// `<tsc|tsbuild>/<scenario>/<name>.js`.
    pub(super) baseline: String,
    #[serde(default)]
    cwd: String,
    args: Vec<String>,
    #[serde(default)]
    env: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    output_is_tty: Option<bool>,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default)]
    windows_style_root: String,
    files: Vec<SeedFile>,
    edits: Vec<Edit>,
}

#[derive(Clone, Debug, Deserialize)]
struct SeedFile {
    path: String,
    #[serde(default)]
    symlink: Option<String>,
    #[serde(flatten)]
    contents: Contents,
}

/// File text: UTF-8, or other bytes in base64.
#[derive(Clone, Debug, Default, Deserialize)]
struct Contents {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    base64: Option<String>,
}

impl Contents {
    fn bytes(&self) -> Result<Vec<u8>, String> {
        match (&self.text, &self.base64) {
            (Some(text), _) => Ok(text.as_bytes().to_vec()),
            (None, Some(encoded)) => decode_base64(encoded),
            (None, None) => Ok(Vec::new()),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Edit {
    caption: String,
    /// The command line of this run; the scenario's when absent.
    args: Option<Vec<String>>,
    #[serde(default)]
    expected_diff: String,
    ops: Vec<Op>,
    /// The operations on the clean system of the incremental correctness
    /// check, when they differ from `ops`.
    #[serde(default)]
    non_incremental_ops: Option<Vec<Op>>,
}

/// One TestSys file operation of an edit.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Op {
    Write {
        path: String,
        #[serde(flatten)]
        contents: Contents,
    },
    Append {
        path: String,
        #[serde(flatten)]
        contents: Contents,
    },
    Prepend {
        path: String,
        #[serde(flatten)]
        contents: Contents,
    },
    Replace {
        path: String,
        old: String,
        new: String,
    },
    ReplaceAll {
        path: String,
        old: String,
        new: String,
    },
    Remove {
        path: String,
    },
    Rename {
        path: String,
        to: String,
    },
    Touch {
        path: String,
    },
}

#[derive(Deserialize)]
struct Recording {
    scenarios: Vec<Scenario>,
}

/// The recorded scenarios of the profile.
pub(super) fn cases(workspace: &Path, profile: &str) -> Result<Vec<Scenario>, String> {
    let path = workspace
        .join("vendor/typescript-native")
        .join(profile)
        .join("tsctests-scenarios.json");
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let recording: Recording =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(recording.scenarios)
}

/// The suite (`tsc` or `tsbuild`) and the baseline's name within it.
pub(super) fn suite_and_name(scenario: &Scenario) -> (&str, &str) {
    scenario
        .baseline
        .split_once('/')
        .unwrap_or(("tsc", scenario.baseline.as_str()))
}

/// tsgo `tscInput.run`: the baseline of one scenario.
pub(super) fn render(scenario: &Scenario, libraries: &[String]) -> Result<String, String> {
    let mut baseline = String::new();
    let system = TestSystem::new(scenario, libraries, false)?;
    baseline.push_str(&format!(
        "currentDirectory::{}\nuseCaseSensitiveFileNames::{}\nInput::\n",
        system.current_directory_text(),
        !scenario.ignore_case
    ));
    system.baseline_fs_with_diff(&mut baseline);
    let mut watcher = execute(&system, &mut baseline, &scenario.args);
    system.serialize_state(&mut baseline);
    if watcher.is_some() && system.has_watches() {
        baseline.push_str(&system.watch_state());
    }
    let mut unexpected = system.baseline_programs(&mut baseline, "Initial build");
    for (index, edit) in scenario.edits.iter().enumerate() {
        system.clear_output();
        let args = edit.args.as_ref().unwrap_or(&scenario.args);
        baseline.push_str(&format!("\n\nEdit [{index}]:: {}\n", edit.caption));
        system.apply(&edit.ops)?;
        let changed = system.changed_paths();
        system.baseline_fs_with_diff(&mut baseline);
        // A watch run sees the changes through its watches and runs a
        // cycle; any other run runs the command again.
        match &mut watcher {
            Some(watcher) => {
                system.send_changed_paths(&changed);
                watcher.do_cycle();
            }
            None => {
                execute(&system, &mut baseline, args);
            }
        }
        system.serialize_state(&mut baseline);
        if watcher.is_some() && system.has_watches() {
            baseline.push_str(&system.watch_state());
        }
        unexpected.push_str(&system.baseline_programs(
            &mut baseline,
            &format!("Edit [{index}]:: {}\n", edit.caption),
        ));

        // The incremental correctness check: a clean system with every
        // edit so far, built once.
        let clean = TestSystem::new(scenario, libraries, true)?;
        for edit in &scenario.edits[..=index] {
            clean.apply(edit.non_incremental_ops.as_ref().unwrap_or(&edit.ops))?;
        }
        let _ = tsc_compiler::execute_command_line(&clean, args, Some(&clean)).status;
        let diff = system::diff_for_incremental(&system, &clean);
        if !diff.is_empty() {
            let explanation = if edit.expected_diff.is_empty() {
                "!!! Unexpected diff, please review and either fix or write explanation as expectedDiff !!!"
            } else {
                edit.expected_diff.as_str()
            };
            baseline.push_str(&format!("\n\nDiff:: {explanation}\n"));
            baseline.push_str(&diff);
        } else if !edit.expected_diff.is_empty() {
            baseline.push_str(&format!(
                "\n\nDiff:: {} !!! Diff not found but explanation present, please review and remove the explanation !!!\n",
                edit.expected_diff
            ));
        }
    }
    let _ = unexpected;
    Ok(baseline)
}

/// tsgo `executeCommand`: the command line, its run and its exit status;
/// the watch a `--watch` command started.
fn execute<'a>(
    system: &'a TestSystem,
    baseline: &mut String,
    args: &[String],
) -> Option<tsc_compiler::watch::Watcher<'a>> {
    baseline.push_str("tsgo ");
    baseline.push_str(&args.join(" "));
    baseline.push('\n');
    let result = tsc_compiler::execute_command_line(system, args, Some(system));
    baseline.push_str(match result.status {
        0 => "ExitStatus:: Success",
        1 => "ExitStatus:: DiagnosticsPresent_OutputsSkipped",
        2 => "ExitStatus:: DiagnosticsPresent_OutputsGenerated",
        3 => "ExitStatus:: InvalidProject_OutputsSkipped",
        4 => "ExitStatus:: ProjectReferenceCycle_OutputsSkipped",
        5 => "ExitStatus:: NotImplemented",
        _ => "ExitStatus:: Unknown",
    });
    result.watcher
}

fn decode_base64(encoded: &str) -> Result<Vec<u8>, String> {
    let value = |byte: u8| -> Result<u32, String> {
        Ok(match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("invalid base64 byte {byte:#x}")),
        })
    };
    let mut bytes = Vec::new();
    for chunk in encoded.trim_end_matches('=').as_bytes().chunks(4) {
        let mut accumulator = 0u32;
        for &byte in chunk {
            accumulator = accumulator << 6 | value(byte)?;
        }
        let bits = chunk.len() * 6;
        accumulator <<= 24 - bits;
        for index in 0..(bits / 8) {
            bytes.push((accumulator >> (16 - 8 * index)) as u8);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_every_tail_length() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("YQ==").unwrap(), b"a");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
        assert_eq!(decode_base64("//79").unwrap(), [0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn recorded_operations_deserialize() {
        let ops: Vec<Op> = serde_json::from_str(
            r#"[{"op":"write","path":"/a","text":"x"},{"op":"replace","path":"/a","old":"x","new":"y"},{"op":"touch","path":"/a"},{"op":"rename","path":"/a","to":"/b"}]"#,
        )
        .unwrap();
        assert_eq!(ops.len(), 4);
        assert!(
            matches!(&ops[0], Op::Write { contents, .. } if contents.text.as_deref() == Some("x"))
        );
        assert!(matches!(&ops[3], Op::Rename { to, .. } if to == "/b"));
    }
}
