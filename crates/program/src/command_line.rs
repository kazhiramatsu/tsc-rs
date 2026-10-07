//! tsgo's command-line parser (tsoptions/commandlineparser.go) over the
//! option catalog: the compile command line and the build command line, with
//! tsgo's option lookup (lowercase names, short names, watch options), value
//! parsing per option kind and diagnostics, and the conversions of the
//! parsed values into the config option bag (merged over a config's
//! options) or into a program's options (explicit files).

use serde_json::Number;
use tsc_diagnostics::{gen, Diagnostic, DiagnosticMessage, JsStr, JsString, MessageChain};
use tsc_types::CompilerOptions;

use crate::config::{
    bag_compiler_options, config_named_option_choices, config_named_string_option_choices,
    config_program_path, effective_discovery_options, ConfigOption, ConfigOptionBag,
    ConfigParseError, ConfigTypedListElement, ConfigTypedOptionValue,
};
use crate::config_options::{
    build_option_declaration_ignore_case, compiler_option_declaration,
    compiler_option_declaration_ignore_case, is_common_with_build, named_value_in,
    option_min_value, option_spelling_suggestion, CompilerOptionDeclaration,
    CompilerOptionListElementKind, CompilerOptionValueKind, BUILD_OPTION_DECLARATIONS,
    COMPILER_OPTION_DECLARATIONS, WATCH_OPTION_DECLARATIONS,
};
use crate::js_path::normalized_config_value_path;
use crate::json_value::JsonValue as Value;
use crate::prepared::ProgramOptions;

/// tsgo `ParsedCommandLine` as far as the command needs it: the options in
/// first-assignment order with their last value (`null` for an explicit
/// `null`), the file names and the parse errors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedCommandLine {
    pub options: Vec<(String, Value)>,
    pub file_names: Vec<String>,
    pub errors: Vec<Diagnostic>,
}

impl ParsedCommandLine {
    pub fn option_value(&self, name: &str) -> Option<&Value> {
        self.options
            .iter()
            .find(|(option, _)| option == name)
            .map(|(_, value)| value)
    }

    pub fn option_bool(&self, name: &str) -> Option<bool> {
        self.option_value(name).and_then(Value::as_bool)
    }

    pub fn option_string(&self, name: &str) -> Option<JsStr<'_>> {
        self.option_value(name).and_then(Value::as_js)
    }
}

/// tsgo `ParsedBuildCommandLine`: the build options, the compiler options
/// common with the build (applied to every project), the projects (`.` when
/// none was named) and the parse errors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedBuildCommandLine {
    pub build_options: Vec<(String, Value)>,
    pub options: Vec<(String, Value)>,
    pub projects: Vec<String>,
    pub errors: Vec<Diagnostic>,
}

impl ParsedBuildCommandLine {
    pub fn build_bool(&self, name: &str) -> bool {
        self.build_options
            .iter()
            .find(|(option, _)| option == name)
            .and_then(|(_, value)| value.as_bool())
            .unwrap_or(false)
    }

    pub fn option_bool(&self, name: &str) -> Option<bool> {
        self.options
            .iter()
            .find(|(option, _)| option == name)
            .and_then(|(_, value)| value.as_bool())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Compile,
    Build,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Group {
    Compiler,
    Build,
    Watch,
}

#[derive(Clone, Copy)]
struct OptionRef {
    declaration: &'static CompilerOptionDeclaration,
    group: Group,
}

/// The host read of a response file (`@file`): its text, or `None` when
/// the file cannot be read.
pub type ResponseFileReader<'a> = dyn Fn(JsStr<'_>) -> Option<String> + 'a;

struct Parser<'a> {
    mode: Mode,
    current_directory: JsStr<'a>,
    case_sensitive: bool,
    read_file: &'a ResponseFileReader<'a>,
    options: Vec<(String, Value)>,
    file_names: Vec<String>,
    errors: Vec<Diagnostic>,
    response_files: Vec<JsString>,
}

/// tsgo `ParseCommandLine`: the compile command line.
pub fn parse_command_line(
    args: &[String],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
    read_file: &ResponseFileReader<'_>,
) -> ParsedCommandLine {
    let mut parser = Parser::new(Mode::Compile, current_directory, case_sensitive, read_file);
    parser.parse_strings(args);
    ParsedCommandLine {
        options: parser.options,
        file_names: parser.file_names,
        errors: parser.errors,
    }
}

/// tsgo `ParseBuildCommandLine`: the build command line (the arguments
/// after the leading `-b`), with its nonsensical combinations (TS6370).
pub fn parse_build_command_line(
    args: &[String],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
    read_file: &ResponseFileReader<'_>,
) -> ParsedBuildCommandLine {
    let mut parser = Parser::new(Mode::Build, current_directory, case_sensitive, read_file);
    parser.parse_strings(args);
    let mut build_options = Vec::new();
    let mut options = Vec::new();
    for (name, value) in parser.options {
        if BUILD_OPTION_DECLARATIONS
            .iter()
            .any(|declaration| declaration.name() == name)
        {
            build_options.push((name, value));
        } else {
            options.push((name, value));
        }
    }
    let mut projects = parser.file_names;
    if projects.is_empty() {
        projects.push(".".to_owned());
    }
    let mut errors = parser.errors;
    let is_set = |list: &[(String, Value)], name: &str| {
        list.iter()
            .any(|(option, value)| option == name && value.as_bool() == Some(true))
    };
    let clean = is_set(&build_options, "clean");
    let watch = is_set(&options, "watch");
    for (left, right, combined) in [
        ("clean", "force", clean && is_set(&build_options, "force")),
        (
            "clean",
            "verbose",
            clean && is_set(&build_options, "verbose"),
        ),
        ("clean", "watch", clean && watch),
        ("watch", "dry", watch && is_set(&build_options, "dry")),
    ] {
        if combined {
            errors.push(fileless(
                &gen::Options_0_and_1_cannot_be_combined,
                &[left.to_owned(), right.to_owned()],
            ));
        }
    }
    ParsedBuildCommandLine {
        build_options,
        options,
        projects,
        errors,
    }
}

fn fileless(message: &'static DiagnosticMessage, arguments: &[String]) -> Diagnostic {
    Diagnostic::new(None, None, None, MessageChain::new(message, arguments))
}

/// tsgo `getInputOptionName`: up to two leading dashes stripped.
fn input_option_name(argument: &str) -> &str {
    let name = argument.strip_prefix('-').unwrap_or(argument);
    name.strip_prefix('-').unwrap_or(name)
}

/// tsgo `getCompilerOptionValueTypeString`.
fn value_type_string(declaration: &CompilerOptionDeclaration) -> &'static str {
    match declaration.value_kind() {
        CompilerOptionValueKind::Boolean => "boolean",
        CompilerOptionValueKind::Number => "number",
        CompilerOptionValueKind::String => "string",
        CompilerOptionValueKind::Named(_) => "enum",
        CompilerOptionValueKind::Object(_) => "object",
        CompilerOptionValueKind::List(_) => "Array",
    }
}

/// tsgo `createDiagnosticForInvalidEnumType`: the option as `--name` and the
/// spellings its map accepts (deprecated ones omitted).
fn invalid_enum_diagnostic(declaration: &CompilerOptionDeclaration) -> Diagnostic {
    let choices = match declaration.value_kind() {
        CompilerOptionValueKind::Named(values) => {
            config_named_option_choices(declaration.name(), values)
        }
        CompilerOptionValueKind::List(descriptor) => config_named_string_option_choices(descriptor),
        _ => String::new(),
    };
    fileless(
        &gen::Argument_for_0_option_must_be_1,
        &[format!("--{}", declaration.name()), choices],
    )
}

impl<'a> Parser<'a> {
    fn new(
        mode: Mode,
        current_directory: JsStr<'a>,
        case_sensitive: bool,
        read_file: &'a ResponseFileReader<'a>,
    ) -> Self {
        Self {
            mode,
            current_directory,
            case_sensitive,
            read_file,
            options: Vec::new(),
            file_names: Vec::new(),
            errors: Vec::new(),
            response_files: Vec::new(),
        }
    }

    /// tsgo `OrderedMap.Set`: the first assignment fixes the position, a
    /// later one replaces the value.
    fn set(&mut self, name: &str, value: Value) {
        if let Some(entry) = self.options.iter_mut().find(|(option, _)| option == name) {
            entry.1 = value;
        } else {
            self.options.push((name.to_owned(), value));
        }
    }

    /// The option an argument names in this mode (tsgo
    /// `GetOptionDeclarationFromName` with short names), or a watch option.
    fn lookup(&self, name: &str) -> Option<OptionRef> {
        let lower = name.to_ascii_lowercase();
        let (declarations, short_names): (&[&'static CompilerOptionDeclaration], &[(&str, &str)]) =
            match self.mode {
                Mode::Compile => (&[], COMPILER_SHORT_NAMES),
                Mode::Build => (&[], BUILD_SHORT_NAMES),
            };
        let _ = declarations;
        let full = short_names
            .iter()
            .find(|(short, _)| *short == lower)
            .map_or(lower.as_str(), |(_, full)| *full);
        let found = match self.mode {
            Mode::Compile => {
                compiler_option_declaration_ignore_case(full).map(|declaration| OptionRef {
                    declaration,
                    group: Group::Compiler,
                })
            }
            Mode::Build => {
                build_option_declaration_ignore_case(full).map(|declaration| OptionRef {
                    declaration,
                    group: if BUILD_OPTION_DECLARATIONS
                        .iter()
                        .any(|build| std::ptr::eq(build, declaration))
                    {
                        Group::Build
                    } else {
                        Group::Compiler
                    },
                })
            }
        };
        found.or_else(|| {
            WATCH_OPTION_DECLARATIONS
                .iter()
                .chain(WATCH_INTERVAL.iter())
                .rfind(|declaration| declaration.name().eq_ignore_ascii_case(&lower))
                .map(|declaration| OptionRef {
                    declaration,
                    group: Group::Watch,
                })
        })
    }

    /// tsgo `parseStrings`.
    fn parse_strings(&mut self, args: &[String]) {
        let mut index = 0usize;
        while index < args.len() {
            let argument = args[index].as_str();
            index += 1;
            if argument.is_empty() {
                continue;
            }
            match argument.as_bytes()[0] {
                b'@' => self.parse_response_file(&argument[1..]),
                b'-' => {
                    let name = input_option_name(argument);
                    match self.lookup(name) {
                        Some(option) => index = self.parse_option_value(args, index, option),
                        None => {
                            let diagnostic = self.unknown_option_error(name, argument);
                            self.errors.push(diagnostic);
                        }
                    }
                }
                _ => self.file_names.push(argument.to_owned()),
            }
        }
    }

    /// tsgo `parseResponseFile`.
    fn parse_response_file(&mut self, file_name: &str) {
        let file_name = normalized_config_value_path(file_name.into(), self.current_directory);
        let key = crate::js_path::file_name_key(file_name.as_js(), self.case_sensitive);
        if self.response_files.contains(&key) {
            return;
        }
        self.response_files.push(key);
        let text = match (self.read_file)(file_name.as_js()) {
            Some(text) => text,
            None => {
                self.errors.push(fileless(
                    &gen::Cannot_read_file_0,
                    &[file_name.to_string_lossy().into_owned()],
                ));
                String::new()
            }
        };
        if !text.is_empty() {
            let mut args = Vec::new();
            let text = text.chars().collect::<Vec<_>>();
            let mut pos = 0usize;
            while pos < text.len() {
                while pos < text.len() && text[pos] <= ' ' {
                    pos += 1;
                }
                if pos >= text.len() {
                    break;
                }
                let start = pos;
                if text[pos] == '"' {
                    pos += 1;
                    while pos < text.len() && text[pos] != '"' {
                        pos += 1;
                    }
                    if pos < text.len() {
                        args.push(text[start + 1..pos].iter().collect::<String>());
                        pos += 1;
                    } else {
                        self.errors.push(fileless(
                            &gen::Unterminated_quoted_string_in_response_file_0,
                            &[file_name.to_string_lossy().into_owned()],
                        ));
                    }
                } else {
                    while pos < text.len() && text[pos] > ' ' {
                        pos += 1;
                    }
                    args.push(text[start..pos].iter().collect::<String>());
                }
            }
            self.parse_strings(&args);
        }
        self.response_files.pop();
    }

    /// tsgo `createUnknownOptionError`: the other mode's option first
    /// (`build` must be first; a build-only option needs `--build`; a
    /// compiler option may not be used with `--build`), then a spelling
    /// suggestion over this mode's options, else the plain unknown option.
    fn unknown_option_error(&self, name: &str, argument: &str) -> Diagnostic {
        let lower = name.to_ascii_lowercase();
        match self.mode {
            Mode::Compile => {
                if lower == "build" {
                    return fileless(
                        &gen::Option_build_must_be_the_first_command_line_argument,
                        &[],
                    );
                }
                if let Some(declaration) = build_option_declaration_ignore_case(&lower) {
                    return fileless(
                        &gen::Compiler_option_0_may_only_be_used_with_build,
                        &[declaration.name().to_owned()],
                    );
                }
                match option_spelling_suggestion(name, COMPILER_OPTION_DECLARATIONS) {
                    Some(suggestion) => fileless(
                        &gen::Unknown_compiler_option_0_Did_you_mean_1,
                        &[argument.to_owned(), suggestion.name().to_owned()],
                    ),
                    None => fileless(&gen::Unknown_compiler_option_0, &[argument.to_owned()]),
                }
            }
            Mode::Build => {
                if let Some(declaration) = compiler_option_declaration_ignore_case(&lower) {
                    return fileless(
                        &gen::Compiler_option_0_may_not_be_used_with_build,
                        &[declaration.name().to_owned()],
                    );
                }
                match option_spelling_suggestion(
                    name,
                    BUILD_MODE_DECLARATIONS.get_or_init(build_mode_declarations),
                ) {
                    Some(suggestion) => fileless(
                        &gen::Unknown_build_option_0_Did_you_mean_1,
                        &[argument.to_owned(), suggestion.name().to_owned()],
                    ),
                    None => fileless(&gen::Unknown_build_option_0, &[argument.to_owned()]),
                }
            }
        }
    }

    /// The type-mismatch diagnostic of the mode (tsgo
    /// `OptionTypeMismatchDiagnostic`): TS6044 for the compile command line,
    /// TS5073 for the build command line, TS5080 for a watch option.
    fn type_mismatch(&self, option: OptionRef, type_name: &str) -> Diagnostic {
        let name = option.declaration.name().to_owned();
        match (option.group, self.mode) {
            (Group::Watch, _) => fileless(
                &gen::Watch_option_0_requires_a_value_of_type_1,
                &[name, type_name.to_owned()],
            ),
            (_, Mode::Build) => fileless(
                &gen::Build_option_0_requires_a_value_of_type_1,
                &[name, type_name.to_owned()],
            ),
            (_, Mode::Compile) => fileless(
                &gen::Compiler_option_0_expects_an_argument,
                &[name, type_name.to_owned()],
            ),
        }
    }

    /// tsgo `parseOptionValue`: returns the index after the value.
    fn parse_option_value(&mut self, args: &[String], index: usize, option: OptionRef) -> usize {
        let declaration = option.declaration;
        let name = declaration.name();
        let mut index = index;
        if declaration.is_tsconfig_only() {
            let value = args.get(index).map(String::as_str).unwrap_or("");
            if value == "null" {
                self.set(name, Value::Null);
                index += 1;
            } else if matches!(declaration.value_kind(), CompilerOptionValueKind::Boolean) {
                if value == "false" {
                    self.set(name, Value::Bool(false));
                    index += 1;
                } else {
                    if value == "true" {
                        index += 1;
                    }
                    self.errors.push(fileless(
                        &gen::Option_0_can_only_be_specified_in_tsconfig_json_file_or_set_to_false_or_null_on_command_line,
                        &[name.to_owned()],
                    ));
                }
            } else {
                self.errors.push(fileless(
                    &gen::Option_0_can_only_be_specified_in_tsconfig_json_file_or_set_to_null_on_command_line,
                    &[name.to_owned()],
                ));
                if !value.is_empty() && !value.starts_with('-') {
                    index += 1;
                }
            }
            return index;
        }
        let Some(value) = args.get(index).map(String::as_str) else {
            match declaration.value_kind() {
                CompilerOptionValueKind::Boolean => self.set(name, Value::Bool(true)),
                kind => {
                    let type_name = value_type_string(declaration);
                    self.errors.push(self.type_mismatch(option, type_name));
                    match kind {
                        CompilerOptionValueKind::List(_) => {
                            self.set(name, Value::Array(Vec::new()))
                        }
                        CompilerOptionValueKind::Named(_) => {
                            self.errors.push(invalid_enum_diagnostic(declaration));
                        }
                        _ => {}
                    }
                }
            }
            return index;
        };
        if value == "null" {
            self.set(name, Value::Null);
            return index + 1;
        }
        match declaration.value_kind() {
            CompilerOptionValueKind::Number => {
                match value.parse::<i64>() {
                    Ok(number) => {
                        let minimum = option_min_value(name);
                        if number >= minimum {
                            self.set(name, Value::Number(Number::from(number)));
                        } else {
                            self.errors.push(fileless(
                                &gen::Option_0_requires_value_to_be_greater_than_1,
                                &[name.to_owned(), minimum.to_string()],
                            ));
                        }
                    }
                    Err(_) => {
                        self.errors.push(self.type_mismatch(option, "number"));
                    }
                }
                index + 1
            }
            CompilerOptionValueKind::Boolean => {
                self.set(name, Value::Bool(value != "false"));
                if value == "false" || value == "true" {
                    index + 1
                } else {
                    index
                }
            }
            CompilerOptionValueKind::String => {
                if name == "locale" && !is_bcp47_language_tag(value) {
                    self.errors.push(fileless(
                        &gen::Locale_must_be_an_IETF_BCP_47_language_tag_Examples_0_1,
                        &["en".to_owned(), "ja-jp".to_owned()],
                    ));
                } else {
                    self.set(name, Value::String(value.into()));
                }
                index + 1
            }
            CompilerOptionValueKind::List(descriptor) => {
                let trimmed = value.trim();
                if trimmed.starts_with('-') {
                    self.set(name, Value::Array(Vec::new()));
                    return index;
                }
                let mut elements = Vec::new();
                let mut errors = Vec::new();
                if !trimmed.is_empty() {
                    for element in trimmed.split(',') {
                        match descriptor.element_kind() {
                            CompilerOptionListElementKind::NamedString(_) => {
                                let element = element.trim();
                                if element.is_empty() {
                                    continue;
                                }
                                if descriptor.named_string_value(element).is_some() {
                                    elements.push(Value::String(element.into()));
                                } else {
                                    errors.push(invalid_enum_diagnostic(declaration));
                                }
                            }
                            _ => {
                                if !element.is_empty() {
                                    elements.push(Value::String(element.into()));
                                }
                            }
                        }
                    }
                }
                let consumed = !elements.is_empty() || !errors.is_empty();
                self.set(name, Value::Array(elements));
                self.errors.extend(errors);
                if consumed {
                    index + 1
                } else {
                    index
                }
            }
            CompilerOptionValueKind::Named(values) => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    match named_value_in(values, trimmed) {
                        Some(_) => self.set(name, Value::String(trimmed.into())),
                        None => self.errors.push(invalid_enum_diagnostic(declaration)),
                    }
                }
                index + 1
            }
            CompilerOptionValueKind::Object(_) => {
                // Object options are tsconfig-only (handled above).
                index + 1
            }
        }
    }
}

/// A loose check of tsgo's `locale.Parse`: a language subtag of two or three
/// letters, optionally followed by `-`/`_` subtags.
fn is_bcp47_language_tag(value: &str) -> bool {
    let mut parts = value.split(['-', '_']);
    let Some(language) = parts.next() else {
        return false;
    };
    (2..=3).contains(&language.len())
        && language.bytes().all(|byte| byte.is_ascii_alphabetic())
        && parts
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

/// The options the command consumes before a program exists, and tsgo's
/// process options (`quiet`, `singleThreaded`, `checkers`, `pprofDir`).
const COMMAND_ONLY_OPTIONS: &[&str] = &[
    "project",
    "version",
    "init",
    "help",
    "all",
    "watch",
    "showConfig",
    "ignoreConfig",
    "locale",
    "runExternalCode",
    "quiet",
    "singleThreaded",
    "checkers",
    "pprofDir",
];

/// tsgo `shortOptionNames` of the compile command line.
const COMPILER_SHORT_NAMES: &[(&str, &str)] = &[
    ("h", "help"),
    ("?", "help"),
    ("w", "watch"),
    ("i", "incremental"),
    ("d", "declaration"),
    ("q", "quiet"),
    ("v", "version"),
    ("p", "project"),
    ("t", "target"),
    ("m", "module"),
];

/// tsgo `shortOptionNames` of the build command line: the common options'
/// short names, then the build options' (the later `d`/`v` win).
const BUILD_SHORT_NAMES: &[(&str, &str)] = &[
    ("h", "help"),
    ("?", "help"),
    ("w", "watch"),
    ("i", "incremental"),
    ("q", "quiet"),
    ("b", "build"),
    ("v", "verbose"),
    ("d", "dry"),
    ("f", "force"),
];

/// tsgo's `watchInterval` (not a config watch option).
static WATCH_INTERVAL: [CompilerOptionDeclaration; 1] =
    [crate::config_options::watch_interval_declaration()];

static BUILD_MODE_DECLARATIONS: std::sync::OnceLock<Vec<CompilerOptionDeclaration>> =
    std::sync::OnceLock::new();

/// tsgo `BuildOpts`: the compiler options common with the build, then the
/// build options.
fn build_mode_declarations() -> Vec<CompilerOptionDeclaration> {
    COMPILER_OPTION_DECLARATIONS
        .iter()
        .filter(|declaration| is_common_with_build(declaration))
        .chain(BUILD_OPTION_DECLARATIONS.iter())
        .copied()
        .collect()
}

/// The command line's compiler options as a config option bag (tsgo wraps
/// them as `compilerOptions` and merges them over a config's), with the
/// config converter's representations: file paths absolute against the
/// current directory, named values resolved, lists typed; an explicit
/// `null` removes the option.
pub fn command_line_option_bag(
    options: &[(String, Value)],
    base_path: JsStr<'_>,
) -> ConfigOptionBag {
    let mut bag = ConfigOptionBag::default();
    for (name, value) in options {
        // The options the command itself consumes (tsgo's executor reads
        // them from the parsed command line) and tsgo's own process options
        // have no meaning for the program.
        if COMMAND_ONLY_OPTIONS.contains(&name.as_str()) {
            continue;
        }
        let Some(declaration) = compiler_option_declaration(name.as_str()) else {
            continue;
        };
        if value.is_null() {
            bag.remove(name.as_str());
            continue;
        }
        bag.insert(ConfigOption {
            name: name.as_str().into(),
            value: value.clone(),
            base_path: base_path.to_owned(),
        });
        let typed = match declaration.value_kind() {
            CompilerOptionValueKind::Boolean | CompilerOptionValueKind::Number => {
                Some(ConfigTypedOptionValue::Json(value.clone()))
            }
            CompilerOptionValueKind::String => {
                Some(ConfigTypedOptionValue::Json(match value.as_js() {
                    Some(written) if declaration.is_file_path() => {
                        Value::String(normalized_config_value_path(
                            crate::js_path::normalize_slashes(written).as_js(),
                            base_path,
                        ))
                    }
                    _ => value.clone(),
                }))
            }
            CompilerOptionValueKind::Named(values) => value
                .as_js()
                .and_then(|written| named_value_in(values, written))
                .map(|converted| ConfigTypedOptionValue::Json(Value::from(converted))),
            CompilerOptionValueKind::List(descriptor) => {
                let elements = value
                    .as_array()
                    .map(|elements| {
                        elements
                            .iter()
                            .filter_map(Value::as_js)
                            .filter_map(|written| match descriptor.element_kind() {
                                CompilerOptionListElementKind::FilePath => {
                                    Some(Value::String(normalized_config_value_path(
                                        crate::js_path::normalize_slashes(written).as_js(),
                                        base_path,
                                    )))
                                }
                                CompilerOptionListElementKind::NamedString(_) => descriptor
                                    .named_string_value(written)
                                    .map(|mapped| Value::String(mapped.into())),
                                CompilerOptionListElementKind::String
                                | CompilerOptionListElementKind::Object => {
                                    Some(Value::String(written.to_owned()))
                                }
                            })
                            .filter(|element| element.as_js().is_some_and(|text| !text.is_empty()))
                            .map(ConfigTypedListElement::Value)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Some(ConfigTypedOptionValue::List(elements))
            }
            CompilerOptionValueKind::Object(_) => None,
        };
        bag.insert_typed(name.as_str(), typed);
    }
    bag
}

/// The program's options from the command line alone (explicit files): the
/// same conversion as a config's, without a config file.
pub fn command_line_program_inputs(
    bag: &ConfigOptionBag,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> Result<(CompilerOptions, ProgramOptions), ConfigParseError> {
    let discovery = effective_discovery_options(bag, current_directory)?;
    let compiler_options = bag_compiler_options(bag, &discovery);
    let mut program_options = ProgramOptions::default();
    if let Some(value) = bag.option_bool("noLib") {
        program_options = program_options.with_no_lib(value);
    }
    if let Some(value) = bag.option_bool("preserveSymlinks") {
        program_options = program_options.with_preserve_symlinks(value);
    }
    if let Some(value) = bag.option_string_list("types") {
        program_options = program_options.with_types(value);
    }
    if let Some(values) = bag.option_string_list("typeRoots") {
        program_options = program_options.with_type_roots(
            values
                .into_iter()
                .map(|value| config_program_path(&value, case_sensitive))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if let Some(values) = bag.option_string_list("rootDirs") {
        program_options = program_options.with_root_dirs(
            values
                .into_iter()
                .map(|value| config_program_path(&value, case_sensitive))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok((compiler_options, program_options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> ParsedCommandLine {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        parse_command_line(&args, "/work".into(), true, &|_| None)
    }

    fn parse_build(args: &[&str]) -> ParsedBuildCommandLine {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        parse_build_command_line(&args, "/work".into(), true, &|_| None)
    }

    fn messages(errors: &[Diagnostic]) -> Vec<(u32, String)> {
        errors
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic
                        .message_text()
                        .as_str()
                        .expect("scalar message")
                        .to_owned(),
                )
            })
            .collect()
    }

    // tsgo 7.1.0-dev-19dadef8, `tsgo -p . <args>` (scratchpad p36g/probe).
    #[test]
    fn unknown_options_are_reported_like_tsgo() {
        assert_eq!(
            messages(&parse(&["--bogus"]).errors),
            [(5023, "Unknown compiler option '--bogus'.".to_owned())]
        );
        assert_eq!(
            messages(&parse(&["--noEmits"]).errors),
            [(
                5025,
                "Unknown compiler option '--noEmits'. Did you mean 'noEmit'?".to_owned()
            )]
        );
        assert_eq!(
            messages(&parse(&["--listFilesOnly=maybe"]).errors),
            [(
                5025,
                "Unknown compiler option '--listFilesOnly=maybe'. Did you mean 'listFilesOnly'?"
                    .to_owned()
            )]
        );
        assert_eq!(
            messages(&parse(&["--build"]).errors),
            [(
                6369,
                "Option '--build' must be the first command line argument.".to_owned()
            )]
        );
        assert_eq!(
            messages(&parse(&["--verbose"]).errors),
            [(
                5093,
                "Compiler option '--verbose' may only be used with '--build'.".to_owned()
            )]
        );
    }

    #[test]
    fn enum_number_and_boolean_values_are_parsed_like_tsgo() {
        let expected_target = "Argument for '--target' option must be: 'es6', 'es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'es2022', 'es2023', 'es2024', 'es2025', 'es2026', 'esnext'.";
        assert_eq!(
            messages(&parse(&["--target"]).errors),
            [
                (
                    6044,
                    "Compiler option 'target' expects an argument.".to_owned()
                ),
                (6046, expected_target.to_owned())
            ]
        );
        assert_eq!(
            messages(&parse(&["--target", "es1"]).errors),
            [(6046, expected_target.to_owned())]
        );
        // `--pretty` is consumed as the target's value, `false` is a file.
        let parsed = parse(&["--target", "--pretty", "false"]);
        assert_eq!(
            messages(&parsed.errors),
            [(6046, expected_target.to_owned())]
        );
        assert_eq!(parsed.option_bool("pretty"), None);
        assert_eq!(parsed.file_names, ["false"]);
        let parsed = parse(&[
            "--target", "ES2022", "--noEmit", "false", "--strict", "a.ts",
        ]);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.option_string("target"), Some("ES2022".into()));
        assert_eq!(parsed.option_bool("noEmit"), Some(false));
        assert_eq!(parsed.option_bool("strict"), Some(true));
        assert_eq!(parsed.file_names, ["a.ts"]);
        assert_eq!(
            messages(&parse(&["--maxNodeModuleJsDepth", "abc"]).errors),
            [(
                6044,
                "Compiler option 'maxNodeModuleJsDepth' expects an argument.".to_owned()
            )]
        );
        // `maybe` is not a boolean value: the switch is true and `maybe` is
        // a file name.
        let parsed = parse(&["--noEmit", "maybe"]);
        assert_eq!(parsed.option_bool("noEmit"), Some(true));
        assert_eq!(parsed.file_names, ["maybe"]);
        let parsed = parse(&["--lib", "es2022,dom", "--types", "node", "-p", "."]);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.option_string("project"), Some(".".into()));
        assert_eq!(
            parsed.option_value("lib"),
            Some(&Value::Array(vec![
                Value::String("es2022".into()),
                Value::String("dom".into())
            ]))
        );
        let parsed = parse(&["--lib", "bogus"]);
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!(parsed.errors[0].code(), 6046);
        assert!(parsed.errors[0]
            .message_text()
            .as_str()
            .expect("scalar message")
            .starts_with("Argument for '--lib' option must be: 'es5', 'es6', 'es2015'"));
    }

    #[test]
    fn build_command_line_is_parsed_like_tsgo() {
        assert_eq!(
            messages(&parse_build(&[".", "--bogus"]).errors),
            [(5072, "Unknown build option '--bogus'.".to_owned())]
        );
        assert_eq!(
            messages(&parse_build(&[".", "--listFilesOnly"]).errors),
            [(
                5094,
                "Compiler option '--listFilesOnly' may not be used with '--build'.".to_owned()
            )]
        );
        let parsed = parse_build(&["app", "-v", "--noEmit", "-f", "--pretty", "false"]);
        assert!(parsed.errors.is_empty());
        assert!(parsed.build_bool("verbose"));
        assert!(parsed.build_bool("force"));
        assert_eq!(parsed.option_bool("noEmit"), Some(true));
        assert_eq!(parsed.option_bool("pretty"), Some(false));
        assert_eq!(parsed.projects, ["app"]);
        assert_eq!(parse_build(&[]).projects, ["."]);
        assert_eq!(
            messages(&parse_build(&["--clean", "--force"]).errors),
            [(
                6370,
                "Options 'clean' and 'force' cannot be combined.".to_owned()
            )]
        );
    }
}
