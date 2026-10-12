//! tsgo `DoContentMapperBaseline` (testutil/tsbaseline/contentmapper_baseline.go):
//! each content-mapped file's original and transformed text, then the
//! diagnostics of those files written by the diagnostic writer, each
//! against the text it belongs to, without ANSI styling and test path
//! prefixes. A program without content-mapped files has no baseline.

use tsc_compiler::ProgramDiagnosticWriter;
use tsc_diagnostics::Diagnostic;
use tsc_program::PreparedProgram;

use super::errors_baseline::remove_test_path_prefixes;

/// A content-mapped file of the program (tsgo `GetContentMapper` finds its
/// mapper): a canonical output, not a supplemental one.
pub(super) struct MappedFile {
    name: String,
    script_kind: &'static str,
    /// Go's `%v` of the mapper's extensions.
    extensions: String,
    original: String,
    transformed: String,
}

/// The content-mapped files in program order.
pub(super) fn mapped_files(prepared: &PreparedProgram) -> Vec<MappedFile> {
    let Some(spec) = prepared.program_options().content_mapper_spec() else {
        return Vec::new();
    };
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    prepared
        .source_files()
        .iter()
        .filter_map(|source| {
            let info = source.content_mapped()?;
            let name = source.path().display();
            let mapper =
                tsc_program::content_mapper_for_file_name(&spec.mappers, name, case_sensitive)?;
            Some(MappedFile {
                name: name.to_string_lossy().into_owned(),
                script_kind: script_kind_name(&info.virtual_file_name.to_string_lossy()),
                extensions: format!(
                    "[{}]",
                    mapper
                        .definition
                        .extensions
                        .iter()
                        .flatten()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                original: info.original_text.to_string(),
                transformed: source.snapshot().text().to_owned(),
            })
        })
        .collect()
}

/// tsgo's `ScriptKind` name of a virtual file (by its extension).
fn script_kind_name(virtual_file_name: &str) -> &'static str {
    let extension = |suffixes: &[&str]| {
        suffixes
            .iter()
            .any(|suffix| virtual_file_name.ends_with(suffix))
    };
    if extension(&[".tsx"]) {
        "ScriptKindTSX"
    } else if extension(&[".ts", ".mts", ".cts"]) {
        "ScriptKindTS"
    } else if extension(&[".jsx"]) {
        "ScriptKindJSX"
    } else if extension(&[".js", ".mjs", ".cjs"]) {
        "ScriptKindJS"
    } else if extension(&[".json"]) {
        "ScriptKindJSON"
    } else {
        "ScriptKindUnknown"
    }
}

fn ensure_trailing_newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}\n")
    }
}

/// tsgo's `ansiEscape` (`\x1b\[[0-9;]*m`) removed.
fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\x1b[") {
        let after = &rest[start + 2..];
        let parameters = after
            .find(|character: char| !(character.is_ascii_digit() || character == ';'))
            .unwrap_or(after.len());
        if after[parameters..].starts_with('m') {
            plain.push_str(&rest[..start]);
            rest = &after[parameters + 1..];
        } else {
            plain.push_str(&rest[..start + 2]);
            rest = after;
        }
    }
    plain.push_str(rest);
    plain
}

/// The baseline, or `None` without content-mapped files. `diagnostics` are
/// the compilation's, in its order; those of the mapped files are written.
pub(super) fn render(
    files: &[MappedFile],
    diagnostics: &[Diagnostic],
    writer: &ProgramDiagnosticWriter,
) -> Result<Option<String>, String> {
    if files.is_empty() {
        return Ok(None);
    }
    let mut baseline = String::new();
    for file in files {
        baseline.push_str(&format!(
            "//// [{}] (ScriptKind: {}, ContentMapper: {})\n",
            remove_test_path_prefixes(&file.name),
            file.script_kind,
            file.extensions
        ));
        baseline.push_str("--- Original ---\n");
        baseline.push_str(&ensure_trailing_newline(&file.original));
        baseline.push_str("--- Transformed ---\n");
        baseline.push_str(&ensure_trailing_newline(&file.transformed));
        baseline.push('\n');
    }
    let file_diagnostics: Vec<Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.file_name.as_ref().is_some_and(|name| {
                let name = name.to_string_lossy();
                files.iter().any(|file| file.name == name)
            })
        })
        .cloned()
        .collect();
    baseline.push_str("=== Diagnostics ===\n\n");
    if file_diagnostics.is_empty() {
        baseline.push_str("<no content>\n");
        return Ok(Some(baseline));
    }
    let rendered = writer.format_with_color_and_context(&file_diagnostics, "\n")?;
    baseline.push_str(&remove_test_path_prefixes(&strip_ansi(&rendered)));
    Ok(Some(baseline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_styles_are_removed_as_tsgo_removes_them() {
        assert_eq!(strip_ansi("\x1b[91merror\x1b[0m x"), "error x");
        assert_eq!(strip_ansi("\x1b[1;31ma\x1b[K"), "a\x1b[K");
    }

    #[test]
    fn script_kinds_follow_the_virtual_name() {
        assert_eq!(script_kind_name("/a.box.ts"), "ScriptKindTS");
        assert_eq!(script_kind_name("/a.box.tsx"), "ScriptKindTSX");
        assert_eq!(script_kind_name("/a.box.mjs"), "ScriptKindJS");
    }
}
