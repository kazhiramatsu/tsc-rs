//! tsgo's command-line parsing tests (tsoptions/commandlineparser_test.go):
//! `TestCommandLineParseResult` and `TestParseCommandLineVerifyNull` write
//! `commandLineParsing/parseCommandLine/<name>.js` and
//! `TestParseBuildCommandLine` writes
//! `commandLineParsing/parseBuildOptions/<name>.js`.
//!
//! A `parseCommandLine` baseline holds the arguments, the parser's raw
//! options in first-assignment order with tsgo's values (an enum's number,
//! a library's file name), the file names and the errors; a
//! `parseBuildOptions` baseline holds the build options and the compiler
//! options common with the build in tsgo's struct order, the projects and
//! the errors.

use tsc_diagnostics::{Diagnostic, JsStr};
use tsc_program::{
    parse_build_command_line, parse_command_line_with_declarations, CompilerOptionDeclaration,
    CompilerOptionValueKind, JsonValue, TYPESCRIPT_7_1_LIBRARIES,
};

use super::errors_baseline::flatten_with;
use super::tables::{
    ExtraOptionKind, PARSE_BUILD_OPTIONS, PARSE_COMMAND_LINE, PARSE_COMMAND_LINE_FALSE, VERIFY_NULL,
};
use tsc_program::go_json::{
    enum_number, FieldKind, GoJson, BUILD_OPTIONS_FIELDS, COMPILER_OPTIONS_FIELDS,
};

/// The tests' tsconfig-only `optionName` of each kind.
static STRING_OPTION: [CompilerOptionDeclaration; 1] = [CompilerOptionDeclaration::tsconfig_only(
    "optionName",
    CompilerOptionValueKind::String,
)];
static NUMBER_OPTION: [CompilerOptionDeclaration; 1] = [CompilerOptionDeclaration::tsconfig_only(
    "optionName",
    CompilerOptionValueKind::Number,
)];

/// One baseline to render: its name under `baselines/reference/tsoptions/`,
/// the arguments and how they are parsed.
pub(super) struct CommandLineCase {
    pub(super) baseline: String,
    pub(super) args: Vec<String>,
    pub(super) kind: CommandLineKind,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CommandLineKind {
    Compile(Option<ExtraOptionKind>),
    Build,
}

/// Every scenario of the three tests, in their order.
pub(super) fn cases() -> Vec<CommandLineCase> {
    let compile = |name: &str, args: &[&str], extra| CommandLineCase {
        baseline: format!("commandLineParsing/parseCommandLine/{name}.js"),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        kind: CommandLineKind::Compile(extra),
    };
    let mut cases = PARSE_COMMAND_LINE
        .iter()
        .map(|(name, args)| compile(name, args, None))
        .collect::<Vec<_>>();
    cases.push(compile(
        PARSE_COMMAND_LINE_FALSE.0,
        PARSE_COMMAND_LINE_FALSE.1,
        None,
    ));
    // TestParseCommandLineVerifyNull's loop.
    for verify in VERIFY_NULL {
        let option = format!("--{}", verify.option_name);
        let scenario = |suffix: &str, args: &[&str]| {
            compile(
                &format!("{} {suffix}", verify.sub_scenario),
                args,
                verify.extra_option,
            )
        };
        cases.push(scenario(
            "allows setting it to null",
            &[&option, "null", "0.ts"],
        ));
        if !verify.non_null_value.is_empty() {
            cases.push(scenario(
                "errors if non null value is passed",
                &[&option, verify.non_null_value, "0.ts"],
            ));
        }
        cases.push(scenario(
            "errors if its followed by another option",
            &["0.ts", "--strictNullChecks", &option],
        ));
        cases.push(scenario("errors if its last option", &["0.ts", &option]));
    }
    cases.extend(
        PARSE_BUILD_OPTIONS
            .iter()
            .map(|(name, args)| CommandLineCase {
                baseline: format!("commandLineParsing/parseBuildOptions/{name}.js"),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                kind: CommandLineKind::Build,
            }),
    );
    cases
}

/// The baseline text of one scenario (`formatNewBaseline` /
/// `formatNewBaselineBuild`).
pub(super) fn render(case: &CommandLineCase) -> String {
    let read_file = |_: JsStr<'_>| None;
    let current_directory = JsStr::from_str("/");
    let mut text = format!("Args::\n{}\n\n", format_args(&case.args));
    match case.kind {
        CommandLineKind::Compile(extra) => {
            let extra: &'static [CompilerOptionDeclaration] = match extra {
                None => &[],
                Some(ExtraOptionKind::String) => &STRING_OPTION,
                Some(ExtraOptionKind::Number) => &NUMBER_OPTION,
            };
            let parsed = parse_command_line_with_declarations(
                &case.args,
                current_directory,
                true,
                &read_file,
                extra,
            );
            let options = GoJson::Object(
                parsed
                    .options
                    .iter()
                    .map(|(name, value)| (name.clone(), raw_value(name, value)))
                    .collect(),
            );
            text.push_str("CompilerOptions::\n");
            text.push_str(&options.compact());
            text.push_str("\n\nFileNames::\n");
            text.push_str(&parsed.file_names.join(","));
            text.push_str("\n\nErrors::\n");
            text.push_str(&format_errors(&parsed.errors));
        }
        CommandLineKind::Build => {
            let parsed = parse_build_command_line(&case.args, current_directory, true, &read_file);
            text.push_str("buildOptions::\n");
            text.push_str(&struct_json(BUILD_OPTIONS_FIELDS, &parsed.build_options).compact());
            text.push_str("\n\ncompilerOptions::\n");
            // ParseBuildCommandLine parses the compiler options common with
            // the build (not the watch options) into core.CompilerOptions.
            let compiler_options = parsed
                .options
                .iter()
                .filter(|(name, _)| {
                    tsc_program::compiler_option_declaration(name.as_str()).is_some()
                })
                .cloned()
                .collect::<Vec<_>>();
            text.push_str(&struct_json(COMPILER_OPTIONS_FIELDS, &compiler_options).compact());
            text.push_str("\n\nProjects::\n");
            text.push_str(&parsed.projects.join(","));
            text.push_str("\n\nErrors::\n");
            text.push_str(&format_errors(&parsed.errors));
        }
    }
    text
}

/// The arguments as the tests print them: quoted, unescaped.
fn format_args(args: &[String]) -> String {
    let quoted = args
        .iter()
        .map(|arg| format!("\"{arg}\""))
        .collect::<Vec<_>>();
    format!("[{}]", quoted.join(", "))
}

/// A raw command-line value as tsgo's parser stores it: an enum option's
/// tsgo number, a library's file name.
fn raw_value(name: &str, value: &JsonValue) -> GoJson {
    if let Some(number) = value
        .as_js()
        .and_then(|spelling| spelling.as_str())
        .and_then(|spelling| enum_number(name, spelling))
    {
        return GoJson::number(number);
    }
    if name == "lib" {
        if let Some(elements) = value.as_array() {
            return GoJson::Array(
                elements
                    .iter()
                    .map(|element| {
                        let spelling = element.as_js().map(|spelling| spelling.to_string_lossy());
                        let file = spelling.as_deref().and_then(library_file_name);
                        match file {
                            Some(file) => GoJson::String(file.to_owned()),
                            None => GoJson::from_json(element),
                        }
                    })
                    .collect(),
            );
        }
    }
    GoJson::from_json(value)
}

/// `tsoptions.LibMap`'s file name for a library name.
fn library_file_name(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    TYPESCRIPT_7_1_LIBRARIES
        .iter()
        .find(|library| library.name() == name)
        .map(|library| library.value())
}

/// A struct of tsgo's (`fields` in declaration order) with the parsed
/// options' last values set: `omitzero` leaves out the unset fields and a
/// `null`, which sets nothing.
fn struct_json(fields: &[(&str, FieldKind)], options: &[(String, JsonValue)]) -> GoJson {
    let mut entries = Vec::new();
    for (field, kind) in fields {
        let Some((_, value)) = options.iter().find(|(name, _)| name == field) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let value = match kind {
            FieldKind::Enum => raw_value(field, value),
            FieldKind::String if value.as_js().is_some_and(|text| text.is_empty()) => continue,
            _ => raw_value(field, value),
        };
        entries.push(((*field).to_owned(), value));
    }
    GoJson::Object(entries)
}

/// `diagnosticwriter.WriteFormatDiagnostics` with `NewLine: "\n"`: each
/// diagnostic (the command line's have no file) on its own line.
fn format_errors(errors: &[Diagnostic]) -> String {
    let mut text = String::new();
    for diagnostic in errors {
        text.push_str(&format!(
            "{} TS{}: ",
            diagnostic.category().name(),
            diagnostic.code()
        ));
        flatten_with(&diagnostic.message, "\n", &mut text);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_names_follow_the_tests() {
        let cases = cases();
        assert_eq!(cases.len(), 80);
        assert!(cases.iter().any(|case| case.baseline
            == "commandLineParsing/parseCommandLine/option of type number errors if its last option.js"
            && case.args == ["0.ts", "--optionName"]));
        assert!(cases.iter().any(|case| case.baseline
            == "commandLineParsing/parseBuildOptions/reports error when --builders is 0.js"));
    }
}
