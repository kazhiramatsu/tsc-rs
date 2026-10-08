//! The version and the help the command line prints (tsgo
//! execute/tsc/help.go), with the colors of tsgo's `createColors`.

use tsc_diagnostics::{gen, DiagnosticMessage, MessageCatalog};

use crate::locale::Locale;
use crate::options::{
    DefaultValue, EnumValue, OptionDeclaration, OptionKind, BUILD_OPTION,
    COMMON_OPTIONS_WITH_BUILD, OPTIONS_FOR_BUILD, OPTIONS_FOR_WATCH,
};
use crate::system::System;

/// tsgo's `colors`: ANSI styles when the output takes colors.
pub(crate) struct Colors {
    show: bool,
    windows: bool,
    windows_terminal: bool,
    vscode: bool,
    richer: bool,
}

impl Colors {
    /// tsgo `createColors`: colors follow `defaultIsPretty`, and the
    /// environment picks the blues.
    pub(crate) fn of(system: &dyn System, pretty: bool) -> Self {
        if !pretty {
            return Self {
                show: false,
                windows: false,
                windows_terminal: false,
                vscode: false,
                richer: false,
            };
        }
        let env = |name| system.env_var(name).unwrap_or_default();
        Self {
            show: true,
            windows: env("OS").to_lowercase().contains("windows"),
            windows_terminal: !env("WT_SESSION").is_empty(),
            vscode: env("TERM_PROGRAM") == "vscode",
            richer: env("COLORTERM") == "truecolor" || env("TERM") == "xterm-256color",
        }
    }

    pub(crate) fn bold(&self, text: &str) -> String {
        if self.show {
            format!("\x1b[1m{text}\x1b[22m")
        } else {
            text.to_owned()
        }
    }

    pub(crate) fn blue(&self, text: &str) -> String {
        if !self.show {
            text.to_owned()
        } else if self.windows && !self.windows_terminal && !self.vscode {
            // PowerShell and the command prompt show blue with too little
            // contrast.
            self.bright_white(text)
        } else {
            format!("\x1b[94m{text}\x1b[39m")
        }
    }

    fn blue_background(&self, text: &str) -> String {
        if !self.show {
            text.to_owned()
        } else if self.richer {
            format!("\x1b[48;5;68m{text}\x1b[39;49m")
        } else {
            format!("\x1b[44m{text}\x1b[39;49m")
        }
    }

    fn bright_white(&self, text: &str) -> String {
        if self.show {
            format!("\x1b[97m{text}\x1b[39m")
        } else {
            text.to_owned()
        }
    }
}

/// The help's writer: the system's terminal, colors and language.
pub(crate) struct Help<'a> {
    width: usize,
    colors: Colors,
    catalog: Option<&'a dyn MessageCatalog>,
    version: &'a str,
}

impl<'a> Help<'a> {
    pub(crate) fn new(system: &dyn System, pretty: bool, locale: Locale, version: &'a str) -> Self {
        Self {
            width: system.terminal_width().unwrap_or(0),
            colors: Colors::of(system, pretty),
            catalog: locale.messages(),
            version,
        }
    }

    fn text(&self, message: &'static DiagnosticMessage) -> String {
        message.template_in(self.catalog).to_owned()
    }

    fn format(&self, message: &'static DiagnosticMessage, args: &[&str]) -> String {
        message
            .format_in(self.catalog, args)
            .to_string_lossy()
            .into_owned()
    }

    /// tsgo `PrintVersion`.
    pub(crate) fn version(&self) -> String {
        format!("{}\n", self.format(&gen::Version_0, &[self.version]))
    }

    /// The `tsc: The TypeScript Compiler - Version …` line.
    fn title(&self) -> String {
        format!(
            "{} - {}",
            self.text(&gen::tsc_The_TypeScript_Compiler),
            self.format(&gen::Version_0, &[self.version])
        )
    }

    /// tsgo `getHeader`: the title, with the TypeScript icon at the right
    /// when the terminal is wide enough.
    pub(crate) fn header(&self, message: &str) -> String {
        const ICON: &str = "     ";
        const ICON_TS: &str = "  TS ";
        // Go compares the byte length and pads by runes.
        if self.width >= message.len() + ICON.len() {
            let right_align = self.width.min(120);
            let left_align = right_align - ICON.len();
            let padding = left_align.saturating_sub(message.chars().count());
            format!(
                "{message}{}{}\n{}{}\n",
                " ".repeat(padding),
                self.colors.blue_background(ICON),
                " ".repeat(left_align),
                self.colors
                    .blue_background(&self.colors.bright_white(ICON_TS)),
            )
        } else {
            format!("{message}\n\n")
        }
    }

    /// tsgo `PrintHelp`: the simplified help, or every option with
    /// `--all`.
    pub(crate) fn help(&self, all: bool) -> String {
        let mut options = OptionDeclaration::compiler_options()
            .chain(std::iter::once(&BUILD_OPTION))
            .collect::<Vec<_>>();
        if all {
            options.sort_by_key(|option| option.name.to_lowercase());
            self.all_help(&options)
        } else {
            options.retain(|option| option.show_in_simplified_help_view);
            self.easy_help(&options)
        }
    }

    /// tsgo `printEasyHelp`.
    fn easy_help(&self, options: &[&OptionDeclaration]) -> String {
        let mut output = self.header(&self.title());
        output.push_str(&self.colors.bold(&self.text(&gen::COMMON_COMMANDS)));
        output.push_str("\n\n");
        let mut example = |examples: &[&str], description: &'static DiagnosticMessage| {
            for example in examples {
                output.push_str("  ");
                output.push_str(&self.colors.blue(example));
                output.push('\n');
            }
            output.push_str("  ");
            output.push_str(&self.text(description));
            output.push_str("\n\n");
        };
        example(
            &["tsc"],
            &gen::Compiles_the_current_project_tsconfig_json_in_the_working_directory,
        );
        example(
            &["tsc app.ts util.ts"],
            &gen::Ignoring_tsconfig_json_compiles_the_specified_files_with_default_compiler_options,
        );
        example(
            &["tsc -b"],
            &gen::Build_a_composite_project_in_the_working_directory,
        );
        example(
            &["tsc --init"],
            &gen::Creates_a_tsconfig_json_with_the_recommended_settings_in_the_working_directory,
        );
        example(
            &["tsc -p ./path/to/tsconfig.json"],
            &gen::Compiles_the_TypeScript_project_located_at_the_specified_path,
        );
        example(
            &["tsc --help --all"],
            &gen::An_expanded_version_of_this_information_showing_all_possible_compiler_options,
        );
        example(
            &["tsc --noEmit", "tsc --target esnext"],
            &gen::Compiles_the_current_project_with_additional_settings,
        );
        let (commands, config): (Vec<_>, Vec<_>) = options.iter().partition(|option| {
            option.is_command_line_only
                || option
                    .category
                    .is_some_and(|category| std::ptr::eq(category, &gen::Command_line_Options))
        });
        output.push_str(&self.section(
            &self.text(&gen::COMMAND_LINE_FLAGS),
            &commands,
            false,
            None,
            None,
        ));
        let after = self.format(
            &gen::You_can_learn_about_all_of_the_compiler_options_at_0,
            &["https://aka.ms/tsc"],
        );
        output.push_str(&self.section(
            &self.text(&gen::COMMON_COMPILER_OPTIONS),
            &config,
            false,
            None,
            Some(&after),
        ));
        output
    }

    /// tsgo `printAllHelp`.
    fn all_help(&self, options: &[&OptionDeclaration]) -> String {
        let mut output = self.header(&self.title());
        let after = self.format(
            &gen::You_can_learn_about_all_of_the_compiler_options_at_0,
            &["https://aka.ms/tsc"],
        );
        output.push_str(&self.section(
            &self.text(&gen::ALL_COMPILER_OPTIONS),
            options,
            true,
            None,
            Some(&after),
        ));
        let before_watch = self.text(
            &gen::Including_watch_w_will_start_watching_the_current_project_for_the_file_changes_Once_set_you_can_config_watch_mode_with,
        );
        let watch = OPTIONS_FOR_WATCH.iter().collect::<Vec<_>>();
        output.push_str(&self.section(
            &self.text(&gen::WATCH_OPTIONS),
            &watch,
            false,
            Some(&before_watch),
            None,
        ));
        output.push_str(&self.build_section(&OPTIONS_FOR_BUILD.iter().collect::<Vec<_>>()));
        output
    }

    /// tsgo `PrintBuildHelp` over `BuildOpts`: the options common with the
    /// build, then the build's own.
    pub(crate) fn build_help(&self) -> String {
        let mut output = self.header(&self.title());
        let options = COMMON_OPTIONS_WITH_BUILD
            .iter()
            .chain(OPTIONS_FOR_BUILD.iter())
            .collect::<Vec<_>>();
        output.push_str(&self.build_section(&options));
        output
    }

    fn build_section(&self, build: &[&OptionDeclaration]) -> String {
        let before = self.format(
            &gen::Using_build_b_will_make_tsc_behave_more_like_a_build_orchestrator_than_a_compiler_This_is_used_to_trigger_building_composite_projects_which_you_can_learn_more_about_at_0,
            &["https://aka.ms/tsc-composite-builds"],
        );
        self.section(
            &self.text(&gen::BUILD_OPTIONS),
            build,
            false,
            Some(&before),
            None,
        )
    }

    /// tsgo `generateSectionOptionsOutput`.
    fn section(
        &self,
        name: &str,
        options: &[&OptionDeclaration],
        sub_category: bool,
        before: Option<&str>,
        after: Option<&str>,
    ) -> String {
        let mut output = self.colors.bold(name);
        output.push_str("\n\n");
        if let Some(before) = before {
            output.push_str(before);
            output.push_str("\n\n");
        }
        if !sub_category {
            output.push_str(&self.group(options));
        } else {
            let mut categories: Vec<(String, Vec<&OptionDeclaration>)> = Vec::new();
            for option in options {
                let Some(category) = option.category else {
                    continue;
                };
                let category = self.text(category);
                match categories.iter_mut().find(|(name, _)| *name == category) {
                    Some((_, group)) => group.push(option),
                    None => categories.push((category, vec![option])),
                }
            }
            for (category, group) in categories {
                output.push_str(&format!("### {category}\n\n"));
                output.push_str(&self.group(&group));
            }
        }
        if let Some(after) = after {
            output.push_str(after);
            output.push_str("\n\n");
        }
        output
    }

    /// tsgo `generateGroupOptionOutput`.
    fn group(&self, options: &[&OptionDeclaration]) -> String {
        let max_length = options
            .iter()
            .map(|option| display_name(option).len())
            .max()
            .unwrap_or(0);
        // Two spaces before the names, two between the columns.
        let right_align_of_left = max_length + 2;
        let left_align_of_right = right_align_of_left + 2;
        let mut lines = Vec::new();
        for option in options {
            lines.extend(self.option(option, right_align_of_left, left_align_of_right));
        }
        // Always end with a blank line.
        if lines.len() < 2 || lines[lines.len() - 2] != "\n" {
            lines.push("\n".to_owned());
        }
        lines.concat()
    }

    /// tsgo `generateOptionOutput`.
    fn option(
        &self,
        option: &OptionDeclaration,
        right_align_of_left: usize,
        left_align_of_right: usize,
    ) -> Vec<String> {
        let mut text = Vec::new();
        let name = display_name(option);
        let candidates = value_candidates(self, option);
        let default = match option.default {
            DefaultValue::Message(message) => self.text(message),
            default => {
                let shape = match option.kind {
                    OptionKind::List => option.elements().unwrap_or(option),
                    _ => option,
                };
                format_default_value(default, shape)
            }
        };
        let description = option
            .description
            .map(|description| self.text(description))
            .unwrap_or_default();
        let additional = show_additional_info(&candidates, option);
        if self.width >= 80 {
            text.extend(self.pretty(
                &name,
                &description,
                right_align_of_left,
                left_align_of_right,
                true,
            ));
            text.push("\n".to_owned());
            if additional {
                if let Some((value_type, possible_values)) = &candidates {
                    text.extend(self.pretty(
                        value_type,
                        possible_values,
                        right_align_of_left,
                        left_align_of_right,
                        false,
                    ));
                    text.push("\n".to_owned());
                }
                if !default.is_empty() {
                    text.extend(self.pretty(
                        &self.text(&gen::default),
                        &default,
                        right_align_of_left,
                        left_align_of_right,
                        false,
                    ));
                    text.push("\n".to_owned());
                }
            }
            text.push("\n".to_owned());
        } else {
            text.push(self.colors.blue(&name));
            text.push("\n".to_owned());
            text.push(description);
            text.push("\n".to_owned());
            if additional {
                if let Some((value_type, possible_values)) = &candidates {
                    text.push(format!("{value_type} {possible_values}"));
                }
                if !default.is_empty() {
                    if candidates.is_some() {
                        text.push("\n".to_owned());
                    }
                    text.push(format!("{} {default}", self.text(&gen::default)));
                }
                text.push("\n".to_owned());
            }
            text.push("\n".to_owned());
        }
        text
    }

    /// tsgo `getPrettyOutput`: the left text right-aligned in its column,
    /// the right text wrapped at the terminal's width (by bytes, as Go
    /// slices it).
    fn pretty(
        &self,
        left: &str,
        right: &str,
        right_align_of_left: usize,
        left_align_of_right: usize,
        color_left: bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        let width = self.width.saturating_sub(left_align_of_right).max(1);
        let mut remaining = right.as_bytes();
        let mut first = true;
        while !remaining.is_empty() {
            let current_left = if first {
                let aligned = format!("{left:>right_align_of_left$}");
                let aligned = format!("{aligned:<left_align_of_right$}");
                if color_left {
                    self.colors.blue(&aligned)
                } else {
                    aligned
                }
            } else {
                " ".repeat(left_align_of_right)
            };
            let cut = width.min(remaining.len());
            let (head, tail) = remaining.split_at(cut);
            out.push(current_left);
            out.push(String::from_utf8_lossy(head).into_owned());
            out.push("\n".to_owned());
            remaining = tail;
            first = false;
        }
        out
    }
}

/// tsgo `getDisplayNameTextOfOption`.
fn display_name(option: &OptionDeclaration) -> String {
    match option.short_name {
        Some(short) => format!("--{}, -{short}", option.name),
        None => format!("--{}", option.name),
    }
}

/// tsgo `getValueCandidate`: `type:`/`one or more:`/`one of:` and the
/// values; nothing for an object option.
fn value_candidates(help: &Help<'_>, option: &OptionDeclaration) -> Option<(String, String)> {
    let value_type = match option.kind {
        OptionKind::Object => return None,
        OptionKind::String | OptionKind::Number | OptionKind::Boolean => help.text(&gen::r#type),
        OptionKind::List => help.text(&gen::one_or_more),
        _ => help.text(&gen::one_of),
    };
    Some((value_type, possible_values(option)))
}

/// tsgo `getPossibleValues`: the kind, or the enum's keys with synonyms
/// joined by `/` (deprecated keys left out).
fn possible_values(option: &OptionDeclaration) -> String {
    match option.kind {
        OptionKind::String | OptionKind::Number | OptionKind::Boolean => {
            option.kind.name().to_owned()
        }
        OptionKind::List => option.elements().map(possible_values).unwrap_or_default(),
        OptionKind::Object => String::new(),
        OptionKind::Enum => {
            let deprecated = option.deprecated_keys();
            let mut groups: Vec<(EnumValue, Vec<&str>)> = Vec::new();
            for (key, value) in option.enum_map().unwrap_or_default() {
                if deprecated.contains(key) {
                    continue;
                }
                match groups.iter_mut().find(|(candidate, _)| candidate == value) {
                    Some((_, keys)) => keys.push(key),
                    None => groups.push((*value, vec![key])),
                }
            }
            groups
                .iter()
                .map(|(_, keys)| keys.join("/"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// tsgo `showAdditionalInfoOutput`.
fn show_additional_info(candidates: &Option<(String, String)>, option: &OptionDeclaration) -> bool {
    if option
        .category
        .is_some_and(|category| std::ptr::eq(category, &gen::Command_line_Options))
    {
        return false;
    }
    // Go compares the description with nil and the strings "false" and
    // "n/a" (a boolean false is not one of them).
    if let Some((_, possible_values)) = candidates {
        let default_is_plain = matches!(option.default, DefaultValue::None)
            || matches!(option.default, DefaultValue::Text(text) if text == "false" || text == "n/a");
        if possible_values == "string" && default_is_plain {
            return false;
        }
    }
    true
}

/// tsgo `formatDefaultValue`: `undefined` without one, an enum default by
/// its keys, else the value as Go prints it.
fn format_default_value(default: DefaultValue, option: &OptionDeclaration) -> String {
    if matches!(default, DefaultValue::None | DefaultValue::Unknown) {
        return "undefined".to_owned();
    }
    if option.kind == OptionKind::Enum {
        // The keys whose value equals the default; a default of another
        // type (newLine's "lf") equals none of them.
        let value = match default {
            DefaultValue::Enum(value) => Some(EnumValue::Number(value)),
            DefaultValue::Text(text) => Some(EnumValue::Text(text)),
            _ => None,
        };
        return option
            .enum_map()
            .unwrap_or_default()
            .iter()
            .filter(|(_, candidate)| Some(*candidate) == value)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>()
            .join("/");
    }
    match default {
        DefaultValue::Bool(value) => value.to_string(),
        DefaultValue::Number(value) | DefaultValue::Enum(value) => value.to_string(),
        DefaultValue::Text(text) => text.to_owned(),
        DefaultValue::Message(message) => message.text.to_owned(),
        DefaultValue::None | DefaultValue::Unknown => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(width: usize) -> Help<'static> {
        Help {
            width,
            colors: Colors {
                show: false,
                windows: false,
                windows_terminal: false,
                vscode: false,
                richer: false,
            },
            catalog: None,
            version: "7.1.0-dev",
        }
    }

    fn option(name: &str) -> &'static OptionDeclaration {
        OptionDeclaration::compiler_option(name).expect("a compiler option")
    }

    #[test]
    fn the_version_line_is_tsgos() {
        assert_eq!(plain(0).version(), "Version 7.1.0-dev\n");
        let czech = Help {
            catalog: crate::locale::Locale::Czech.messages(),
            ..plain(0)
        };
        assert_eq!(czech.version(), "Verze 7.1.0-dev\n");
    }

    // tsgo getHeader: the icon at the right of a wide terminal (120 at
    // most), the title alone on a narrow one.
    #[test]
    fn the_header_puts_the_icon_right_of_the_title() {
        let wide = plain(200).header("tsc");
        assert_eq!(
            wide,
            format!("tsc{}     \n{}  TS \n", " ".repeat(112), " ".repeat(115))
        );
        assert_eq!(plain(7).header("tsc"), "tsc\n\n");
    }

    // tsgo's help-all baseline: synonyms joined by `/`, deprecated keys
    // left out, an enum default by its keys and none for newLine's "lf".
    #[test]
    fn values_and_defaults_follow_the_help_all_baseline() {
        assert_eq!(
            possible_values(option("module")),
            "commonjs, es6/es2015, es2020, es2022, esnext, node16, node18, node20, nodenext, preserve"
        );
        assert_eq!(
            possible_values(option("target")),
            "es6/es2015, es2016, es2017, es2018, es2019, es2020, es2021, es2022, es2023, es2024, es2025, es2026, esnext"
        );
        let target = option("target");
        assert_eq!(format_default_value(target.default, target), "es2026");
        let new_line = option("newLine");
        assert_eq!(format_default_value(new_line.default, new_line), "");
        let lib = option("lib");
        assert_eq!(possible_values(lib).split(", ").next(), Some("es5"));
    }

    // tsgo getPrettyOutput: the name right-aligned, the description
    // wrapped by bytes at the terminal's width.
    #[test]
    fn wide_output_aligns_names_and_wraps_descriptions() {
        let lines = plain(30).pretty("--a", "0123456789abcdef", 5, 7, false);
        assert_eq!(lines.concat(), "  --a  0123456789abcdef\n");
        let wrapped = plain(17).pretty("--a", "0123456789abcdef", 5, 7, false);
        assert_eq!(wrapped.concat(), "  --a  0123456789\n       abcdef\n");
    }
}
