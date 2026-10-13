//! tsgo `ls/lsutil/formatcodeoptions.go`: the format settings' types,
//! defaults and conversions from and to the protocol's formatting options.

use tsc_lsproto::FormattingOptions;

use super::{EditorSettings, FormatCodeSettings};

/// tsgo `IndentStyle`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct IndentStyle(pub i64);

impl IndentStyle {
    pub const NONE: Self = Self(0);
    pub const BLOCK: Self = Self(1);
    pub const SMART: Self = Self(2);
}

/// tsgo `SemicolonPreference`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SemicolonPreference(pub &'static str);

impl SemicolonPreference {
    pub const IGNORE: Self = Self("ignore");
    pub const INSERT: Self = Self("insert");
    pub const REMOVE: Self = Self("remove");
}

/// tsgo `printer.GetDefaultIndentSize`.
const DEFAULT_INDENT_SIZE: i64 = 4;

impl FormatCodeSettings {
    /// tsgo `FromLSFormatOptions`: the settings with the client's tab size,
    /// spaces and trailing whitespace.
    pub fn from_ls_format_options(&self, options: &FormattingOptions) -> Self {
        let mut settings = self.clone();
        settings.editor_settings.tab_size = i64::from(options.tab_size);
        settings.editor_settings.indent_size = i64::from(options.tab_size);
        settings.editor_settings.convert_tabs_to_spaces = Some(options.insert_spaces);
        if let Some(trim) = options.trim_trailing_whitespace {
            settings.editor_settings.trim_trailing_whitespace = Some(trim);
        }
        settings
    }

    /// tsgo `ToLSFormatOptions`.
    pub fn to_ls_format_options(&self) -> FormattingOptions {
        FormattingOptions {
            tab_size: self.editor_settings.tab_size as u32,
            insert_spaces: self.editor_settings.convert_tabs_to_spaces == Some(true),
            trim_trailing_whitespace: Some(
                self.editor_settings.trim_trailing_whitespace == Some(true),
            ),
            ..FormattingOptions::default()
        }
    }
}

/// tsgo `GetDefaultFormatCodeSettings`.
pub fn get_default_format_code_settings() -> FormatCodeSettings {
    FormatCodeSettings {
        editor_settings: EditorSettings {
            indent_size: DEFAULT_INDENT_SIZE,
            tab_size: DEFAULT_INDENT_SIZE,
            new_line_character: "\n".to_owned(),
            convert_tabs_to_spaces: Some(true),
            indent_style: IndentStyle::SMART,
            trim_trailing_whitespace: Some(true),
            ..EditorSettings::default()
        },
        insert_space_after_constructor: Some(false),
        insert_space_after_comma_delimiter: Some(true),
        insert_space_after_semicolon_in_for_statements: Some(true),
        insert_space_before_and_after_binary_operators: Some(true),
        insert_space_after_keywords_in_control_flow_statements: Some(true),
        insert_space_after_function_keyword_for_anonymous_functions: Some(false),
        insert_space_after_opening_and_before_closing_nonempty_parenthesis: Some(false),
        insert_space_after_opening_and_before_closing_nonempty_brackets: Some(false),
        insert_space_after_opening_and_before_closing_nonempty_braces: Some(true),
        insert_space_after_opening_and_before_closing_template_string_braces: Some(false),
        insert_space_after_opening_and_before_closing_jsx_expression_braces: Some(false),
        insert_space_before_function_parenthesis: Some(false),
        place_open_brace_on_new_line_for_functions: Some(false),
        place_open_brace_on_new_line_for_control_blocks: Some(false),
        semicolons: SemicolonPreference::IGNORE,
        indent_switch_case: Some(true),
        ..FormatCodeSettings::default()
    }
}
