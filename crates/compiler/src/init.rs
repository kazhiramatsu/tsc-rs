//! `tsc --init` (tsgo execute/tsc/init.go): a `tsconfig.json` with the
//! recommended settings and the command line's own options.

use tsc_diagnostics::{gen, DiagnosticMessage, JsStr, MessageCatalog};
use tsc_program::JsonValue;

use crate::options::{EnumValue, OptionDeclaration};

/// A value `--init` writes: one of the command line's, or a default.
#[derive(Clone, Copy)]
enum InitValue<'v> {
    Raw(&'v JsonValue),
    Bool(bool),
    Text(&'static str),
    Enum(EnumValue),
    EmptyList,
}

/// Whether an option is written commented out (tsgo `commented`).
#[derive(Clone, Copy, Eq, PartialEq)]
enum Commented {
    /// Never: the value is always set.
    Never,
    /// Unless the command line sets it.
    Optional,
}

/// The lines of the new `tsconfig.json` and the command-line options not
/// written yet.
struct ConfigWriter<'o> {
    options: &'o [(String, JsonValue)],
    catalog: Option<&'o dyn MessageCatalog>,
    unwritten: Vec<&'o str>,
    lines: Vec<String>,
}

const TAB: &str = "  ";

impl<'o> ConfigWriter<'o> {
    /// The value the command line gave an option last.
    fn raw(&self, name: &str) -> Option<&'o JsonValue> {
        self.options
            .iter()
            .rev()
            .find(|(option, _)| option == name)
            .map(|(_, value)| value)
    }

    fn line(&mut self, text: String) {
        self.lines.push(text);
    }

    fn header(&mut self, message: &'static DiagnosticMessage) {
        let text = format!("{TAB}{TAB}// {}", message.template_in(self.catalog));
        self.line(text);
    }

    /// tsgo `emitOption`: the command line's value or the default,
    /// commented out when asked.
    fn option(&mut self, setting: &str, default: InitValue<'o>, commented: Commented) {
        self.unwritten.retain(|name| *name != setting);
        let raw = self.raw(setting);
        let comment = commented == Commented::Optional && raw.is_none();
        let value = raw.map_or(default, InitValue::Raw);
        let text = format!(
            "{TAB}{TAB}{}\"{setting}\": {},",
            if comment { "// " } else { "" },
            format_value_or_array(setting, &value)
        );
        self.line(text);
    }

    fn options(&mut self, settings: &[&str], default: InitValue<'o>, commented: Commented)
    where
        InitValue<'o>: Copy,
    {
        for setting in settings {
            self.option(setting, default, commented);
        }
    }
}

/// tsgo `generateTSConfig`: the text of the new `tsconfig.json` for the
/// command line's raw options (in their order), in a catalog's language.
pub(crate) fn generate_tsconfig(
    options: &[(String, JsonValue)],
    catalog: Option<&dyn MessageCatalog>,
) -> String {
    let mut unwritten: Vec<&str> = Vec::new();
    for (name, _) in options {
        if !matches!(name.as_str(), "init" | "help" | "watch")
            && !unwritten.contains(&name.as_str())
        {
            unwritten.push(name);
        }
    }
    let mut config = ConfigWriter {
        options,
        catalog,
        unwritten,
        lines: Vec::new(),
    };
    use Commented::{Never, Optional};

    config.line("{".to_owned());
    config.line(format!(
        "{TAB}// {}",
        gen::Visit_https_aka_ms_tsconfig_to_read_more_about_this_file.template_in(catalog)
    ));
    config.line(format!("{TAB}\"compilerOptions\": {{"));

    config.header(&gen::File_Layout);
    config.option("rootDir", InitValue::Text("./src"), Optional);
    config.option("outDir", InitValue::Text("./dist"), Optional);
    config.line(String::new());

    config.header(&gen::Environment_Settings);
    config.header(&gen::See_also_https_aka_ms_tsconfig_module);
    config.option(
        "module",
        InitValue::Enum(EnumValue::Number(MODULE_NODE_NEXT)),
        Never,
    );
    config.option(
        "target",
        InitValue::Enum(EnumValue::Number(TARGET_ES_NEXT)),
        Never,
    );
    config.option("types", InitValue::EmptyList, Never);
    if let Some(lib) = config.raw("lib") {
        config.option("lib", InitValue::Raw(lib), Never);
    }
    config.header(&gen::For_nodejs);
    config.line(format!("{TAB}{TAB}// \"lib\": [\"esnext\"],"));
    config.line(format!("{TAB}{TAB}// \"types\": [\"node\"],"));
    config.header(&gen::and_npm_install_D_types_node);
    config.line(String::new());

    config.header(&gen::Other_Outputs);
    config.options(
        &["sourceMap", "declaration", "declarationMap"],
        InitValue::Bool(true),
        Never,
    );
    config.line(String::new());

    config.header(&gen::Stricter_Typechecking_Options);
    config.options(
        &["noUncheckedIndexedAccess", "exactOptionalPropertyTypes"],
        InitValue::Bool(true),
        Never,
    );
    config.line(String::new());

    config.header(&gen::Style_Options);
    config.options(
        &[
            "noImplicitReturns",
            "noImplicitOverride",
            "noUnusedLocals",
            "noUnusedParameters",
            "noFallthroughCasesInSwitch",
            "noPropertyAccessFromIndexSignature",
        ],
        InitValue::Bool(true),
        Optional,
    );
    config.line(String::new());

    config.header(&gen::Recommended_Options);
    config.option("strict", InitValue::Bool(true), Never);
    config.option(
        "jsx",
        InitValue::Enum(EnumValue::Number(JSX_REACT_JSX)),
        Never,
    );
    config.options(
        &[
            "verbatimModuleSyntax",
            "isolatedModules",
            "noUncheckedSideEffectImports",
        ],
        InitValue::Bool(true),
        Never,
    );
    config.option(
        "moduleDetection",
        InitValue::Enum(EnumValue::Number(MODULE_DETECTION_FORCE)),
        Never,
    );
    config.option("skipLibCheck", InitValue::Bool(true), Never);

    // The command line's other options, in their order.
    if !config.unwritten.is_empty() {
        config.line(String::new());
        while let Some(&setting) = config.unwritten.first() {
            let value = config
                .raw(setting)
                .expect("a command-line option has a value");
            config.option(setting, InitValue::Raw(value), Never);
        }
    }

    config.line(format!("{TAB}}}"));
    config.line("}".to_owned());
    config.line(String::new());
    config.lines.join("\n")
}

/// tsgo's `core.ModuleKindNodeNext`, `core.ScriptTargetESNext`,
/// `core.JsxEmitReactJSX` and `core.ModuleDetectionKindForce`.
const MODULE_NODE_NEXT: i64 = 199;
const TARGET_ES_NEXT: i64 = 99;
const JSX_REACT_JSX: i64 = 4;
const MODULE_DETECTION_FORCE: i64 = 3;

/// tsgo `formatValueOrArray`: a list element by element, through the
/// element's enum map; anything else through the option's.
fn format_value_or_array(setting: &str, value: &InitValue<'_>) -> String {
    let option = OptionDeclaration::compiler_option(setting);
    match value {
        InitValue::EmptyList => "[]".to_owned(),
        InitValue::Raw(JsonValue::Array(elements)) => {
            let element = option.and_then(OptionDeclaration::elements);
            let formatted = elements
                .iter()
                .map(|element_value| format_single_value(&InitValue::Raw(element_value), element))
                .collect::<Vec<_>>();
            format!("[{}]", formatted.join(", "))
        }
        value => format_single_value(value, option),
    }
}

/// tsgo `formatSingleValue`: an enum value by the first key with that
/// value, as JSON.
fn format_single_value(value: &InitValue<'_>, option: Option<&OptionDeclaration>) -> String {
    let enum_value = match value {
        InitValue::Enum(value) => Some(*value),
        InitValue::Raw(raw) => option.and_then(|option| {
            option.enum_map()?;
            let spelling = raw.as_js()?.to_string_lossy();
            option.enum_value(&spelling)
        }),
        _ => None,
    };
    if let Some(value) = enum_value {
        if let Some(key) = option.and_then(|option| option.enum_key(value)) {
            return json_string(key);
        }
    }
    match value {
        InitValue::Bool(value) => value.to_string(),
        InitValue::Text(text) => json_string(text),
        InitValue::Raw(raw) => json_value(raw),
        InitValue::Enum(EnumValue::Number(number)) => number.to_string(),
        InitValue::Enum(EnumValue::Text(text)) => json_string(text),
        InitValue::EmptyList => "[]".to_owned(),
    }
}

/// A command-line value as Go's JSON writes it.
fn json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_owned(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 1e21 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        JsonValue::String(text) => json_js_string(text.as_js()),
        JsonValue::Array(values) => format!(
            "[{}]",
            values.iter().map(json_value).collect::<Vec<_>>().join(", ")
        ),
        JsonValue::Object(object) => format!(
            "{{{}}}",
            object
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    json_js_string(key.as_js()),
                    json_value(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn json_js_string(text: JsStr<'_>) -> String {
    json_string(&text.to_string_lossy())
}

/// A JSON string as Go's `encoding/json/v2` writes it (no HTML escaping).
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_diagnostics::JsString;

    fn text(value: &str) -> JsonValue {
        JsonValue::String(JsString::from(value))
    }

    fn option(name: &str, value: JsonValue) -> (String, JsonValue) {
        (name.to_owned(), value)
    }

    // tsgo's Initialized-TSConfig-with-enum-value-compiler-options baseline.
    #[test]
    fn enum_options_replace_their_defaults_by_key() {
        let options = [
            option("init", JsonValue::Bool(true)),
            option("target", text("es5")),
            option("jsx", text("react")),
        ];
        assert_eq!(
            generate_tsconfig(&options, None),
            r#"{
  // Visit https://aka.ms/tsconfig to read more about this file
  "compilerOptions": {
    // File Layout
    // "rootDir": "./src",
    // "outDir": "./dist",

    // Environment Settings
    // See also https://aka.ms/tsconfig/module
    "module": "nodenext",
    "target": "es5",
    "types": [],
    // For nodejs:
    // "lib": ["esnext"],
    // "types": ["node"],
    // and npm install -D @types/node

    // Other Outputs
    "sourceMap": true,
    "declaration": true,
    "declarationMap": true,

    // Stricter Typechecking Options
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,

    // Style Options
    // "noImplicitReturns": true,
    // "noImplicitOverride": true,
    // "noUnusedLocals": true,
    // "noUnusedParameters": true,
    // "noFallthroughCasesInSwitch": true,
    // "noPropertyAccessFromIndexSignature": true,

    // Recommended Options
    "strict": true,
    "jsx": "react",
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "noUncheckedSideEffectImports": true,
    "moduleDetection": "force",
    "skipLibCheck": true,
  }
}
"#
        );
    }

    // tsgo's list-compiler-options-with-enum-value and advanced-options
    // baselines: a library list by its keys after `types`, an optional
    // setting uncommented, the other options last in command-line order.
    #[test]
    fn command_line_options_fill_the_template_in_order() {
        let options = [
            option("init", JsonValue::Bool(true)),
            option(
                "lib",
                JsonValue::Array(vec![text("es5"), text("es2015.core")]),
            ),
            option("declarationDir", text("lib")),
            option("noUnusedLocals", JsonValue::Bool(true)),
            option("noErrorTruncation", JsonValue::Bool(true)),
        ];
        let config = generate_tsconfig(&options, None);
        assert!(config.contains(
            "    \"types\": [],\n    \"lib\": [\"es5\", \"es2015.core\"],\n    // For nodejs:\n"
        ));
        assert!(config.contains("\n    \"noUnusedLocals\": true,\n"));
        assert!(config.ends_with(
            "    \"skipLibCheck\": true,\n\n    \"declarationDir\": \"lib\",\n    \"noErrorTruncation\": true,\n  }\n}\n"
        ));
    }

    // tsgo writes an enum value by the first key with that value.
    #[test]
    fn a_synonym_is_written_by_the_first_key() {
        let options = [option("target", text("ES2015"))];
        assert!(generate_tsconfig(&options, None).contains("    \"target\": \"es6\",\n"));
    }
}
