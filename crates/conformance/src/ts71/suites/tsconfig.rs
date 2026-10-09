//! tsgo's tsconfig parsing tests (tsoptions/tsconfigparsing_test.go), which
//! write `tsconfigParsing/<title> jsonParse.js` (`TestParseConfigFileTextToJson`)
//! and `<title> with json api.js` / `<title> with jsonSourceFile api.js`
//! (`TestParseJsonConfigFileContent`, `TestParseJsonSourceFileConfigFileContent`,
//! `TestParseTypeAcquisition`).
//!
//! tsc-rs parses a config file from its text only, as the jsonSourceFile
//! api does. The json api parses the object converted from the same text and
//! reports the same errors without a location (the 40 vendored pairs agree
//! apart from that), so its baseline is rendered from the same parse with the
//! errors' locations removed and the text's own parse errors left out.

use std::collections::BTreeMap;

use tsc_diagnostics::{Diagnostic, FormatDiagnosticsHost, JsString};
use tsc_host::MemoryCompilerHost;
use tsc_program::go_json::{compiler_options_json, struct_json, GoJson, TYPE_ACQUISITION_FIELDS};
use tsc_program::{
    parse_config_file_text_to_json, parse_config_root_plan, CompilerConfigHost, ConfigRootPlan,
    ConfigRootPlanRequest,
};

use super::tables::{TestConfig, JSON_PARSE, PARSE_JSON_CONFIG, TYPE_ACQUISITION};

/// Which of tsgo's two entry points a baseline exercises.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Api {
    Json,
    JsonSourceFile,
}

/// One baseline to render: its name under `baselines/reference/config/` and
/// its input.
pub(super) struct TsconfigCase {
    pub(super) baseline: String,
    pub(super) input: TsconfigInput,
}

pub(super) enum TsconfigInput {
    JsonParse(&'static [&'static str]),
    Parse {
        api: Api,
        include_compiler_options: bool,
        configs: Vec<TestConfig>,
    },
}

/// Every baseline of the three tables, in their order.
pub(super) fn cases() -> Vec<TsconfigCase> {
    let mut cases = JSON_PARSE
        .iter()
        .map(|(title, inputs)| TsconfigCase {
            baseline: format!("tsconfigParsing/{title} jsonParse.js"),
            input: TsconfigInput::JsonParse(inputs),
        })
        .collect::<Vec<_>>();
    let parse = |title: &str, api, include_compiler_options, configs: Vec<TestConfig>| {
        let suffix = match api {
            Api::Json => "with json api",
            Api::JsonSourceFile => "with jsonSourceFile api",
        };
        TsconfigCase {
            baseline: format!("tsconfigParsing/{title} {suffix}.js"),
            input: TsconfigInput::Parse {
                api,
                include_compiler_options,
                configs,
            },
        }
    };
    for api in [Api::Json, Api::JsonSourceFile] {
        for case in PARSE_JSON_CONFIG {
            cases.push(parse(
                case.title,
                api,
                case.include_compiler_options,
                case.input.to_vec(),
            ));
        }
    }
    // TestParseTypeAcquisition's input.
    const TYPE_ACQUISITION_FILES: &[(&str, &str)] = &[("/apath/a.ts", ""), ("/apath/b.ts", "")];
    for (title, config_name, config) in TYPE_ACQUISITION {
        for api in [Api::Json, Api::JsonSourceFile] {
            cases.push(parse(
                title,
                api,
                true,
                vec![TestConfig {
                    json_text: config,
                    config_file_name: config_name,
                    base_path: "/apath",
                    all_file_list: TYPE_ACQUISITION_FILES,
                }],
            ));
        }
    }
    cases
}

/// The baseline text of one case.
pub(super) fn render(case: &TsconfigCase) -> Result<String, String> {
    match &case.input {
        TsconfigInput::JsonParse(inputs) => render_json_parse(inputs),
        TsconfigInput::Parse {
            api,
            include_compiler_options,
            configs,
        } => {
            let mut text = String::new();
            for (index, config) in configs.iter().enumerate() {
                text.push_str(&render_config(config, *api, *include_compiler_options)?);
                if index + 1 != configs.len() {
                    text.push('\n');
                }
            }
            Ok(text)
        }
    }
}

/// `TestParseConfigFileTextToJson`.
fn render_json_parse(inputs: &[&str]) -> Result<String, String> {
    let mut text = String::new();
    for (index, input) in inputs.iter().enumerate() {
        text.push_str("Input::\n");
        text.push_str(input);
        text.push('\n');
        let (config, errors) =
            parse_config_file_text_to_json(JsString::from("/apath/tsconfig.json"), *input)
                .map_err(|error| error.to_string())?;
        text.push_str("Config::\n");
        text.push_str(&GoJson::from_json(&config).indented());
        text.push('\n');
        text.push_str("Errors::\n");
        let mut texts = BTreeMap::new();
        texts.insert("/apath/tsconfig.json".to_owned(), (*input).to_owned());
        text.push_str(&pretty_errors(&errors, "/", &texts, "\n")?);
        text.push('\n');
        if index + 1 != inputs.len() {
            text.push('\n');
        }
    }
    Ok(text)
}

/// `baselineParseConfigWith` for one config.
fn render_config(
    config: &TestConfig,
    api: Api,
    include_compiler_options: bool,
) -> Result<String, String> {
    let base_path = if config.base_path.is_empty() {
        directory_name(&absolute(config.config_file_name, ""))
    } else {
        config.base_path.to_owned()
    };
    let config_file_name = combine(&base_path, config.config_file_name);
    let mut files = config
        .all_file_list
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect::<BTreeMap<_, _>>();
    files.insert(config_file_name.clone(), config.json_text.to_owned());

    let mut text = String::from("Fs::\n");
    let mut paths = files.keys().cloned().collect::<Vec<_>>();
    // vfs.WalkDir visits each directory's entries in name order.
    paths.sort_by(|left, right| left.split('/').cmp(right.split('/')));
    for path in &paths {
        text.push_str(&format!("//// [{path}]\r\n{}\r\n\r\n", files[path]));
    }
    text.push('\n');
    text.push_str("configFileName:: ");
    text.push_str(config.config_file_name);
    text.push('\n');

    let plan = parse(&files, &config_file_name, &base_path, config.json_text)?;
    if include_compiler_options {
        text.push_str("CompilerOptions::\n");
        text.push_str(&compiler_options_json(&plan).indented());
        text.push_str("\n\n");
        text.push_str("TypeAcquisition::\n");
        text.push_str(
            &struct_json(TYPE_ACQUISITION_FIELDS, plan.type_acquisition_option_bag()).indented(),
        );
        text.push_str("\n\n");
    }
    text.push_str("FileNames::\n");
    let file_names = plan
        .file_names()
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    text.push_str(&file_names.join(","));
    text.push('\n');
    text.push_str("Errors::\n");
    let errors = match api {
        Api::JsonSourceFile => plan.errors().to_vec(),
        Api::Json => plan.errors().iter().map(without_location).collect(),
    };
    text.push_str(&pretty_errors(&errors, &base_path, &files, "\r\n")?);
    text.push('\n');
    Ok(text)
}

/// tsc-rs's parse of the config over the in-memory files.
fn parse(
    files: &BTreeMap<String, String>,
    config_file_name: &str,
    base_path: &str,
    json_text: &str,
) -> Result<ConfigRootPlan, String> {
    let current_directory = if base_path.starts_with('/') {
        base_path
    } else {
        "/"
    };
    let mut builder = MemoryCompilerHost::builder(current_directory).case_sensitive(true);
    for (path, content) in files {
        builder = builder.file(path.clone(), content.as_bytes().to_vec());
    }
    let host = builder.build().map_err(|error| format!("{error:?}"))?;
    parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: JsString::from(config_file_name),
            text: json_text.to_owned(),
            base_path: JsString::from(current_directory),
        },
    )
    .map_err(|error| error.to_string())
}

/// The json api reports a diagnostic without its location.
fn without_location(diagnostic: &Diagnostic) -> Diagnostic {
    let mut diagnostic = diagnostic.clone();
    diagnostic.file_name = None;
    diagnostic.start = None;
    diagnostic.length = None;
    diagnostic.related.clear();
    diagnostic
}

/// `diagnosticwriter.FormatDiagnosticsWithColorAndContext` with the given
/// current directory and new line.
fn pretty_errors(
    errors: &[Diagnostic],
    current_directory: &str,
    files: &BTreeMap<String, String>,
    new_line: &str,
) -> Result<String, String> {
    let host = FormatDiagnosticsHost::new(current_directory, files);
    tsc_diagnostics::format_diagnostics_with_color_and_context(errors, &host, new_line)
        .map(|text| text.to_string_lossy().into_owned())
        .map_err(|error| format!("{error:?}"))
}

/// `tspath.GetNormalizedAbsolutePath(path, base)` for the table's paths.
fn absolute(path: &str, base: &str) -> String {
    if path.starts_with('/') || base.is_empty() {
        path.to_owned()
    } else {
        format!("{}/{path}", base.trim_end_matches('/'))
    }
}

/// `tspath.CombinePaths(base, path)`.
fn combine(base: &str, path: &str) -> String {
    if path.starts_with('/') || base.is_empty() {
        path.to_owned()
    } else if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// `tspath.GetDirectoryPath`.
fn directory_name(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_row_is_a_case() {
        // 7 jsonParse titles, 32 tables × 2 apis, 8 type acquisition cases × 2.
        assert_eq!(cases().len(), 7 + 32 * 2 + 8 * 2);
    }

    #[test]
    fn paths_combine_like_tspath() {
        assert_eq!(combine("/", "tsconfig.json"), "/tsconfig.json");
        assert_eq!(combine("/apath", "tsconfig.json"), "/apath/tsconfig.json");
        assert_eq!(
            combine("tests/cases/unittests", "/apath/tsconfig.json"),
            "/apath/tsconfig.json"
        );
    }
}
