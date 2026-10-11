//! TypeScript 7.1's command-line option declarations as its help, `--init`
//! and `--showConfig` read them (tsgo `tsoptions.CommandLineOption`): names,
//! kinds, categories, descriptions, default values, help visibility, enum
//! maps and deprecated keys. The table is generated from the vendored Go
//! sources by `scripts/tsgo_option_declarations.py`.

use tsc_diagnostics::DiagnosticMessage;

// The generator's output is checked byte for byte (`--check`).
#[rustfmt::skip]
mod gen;

pub(crate) use gen::{
    deprecated_keys, elements, enum_map, BUILD_OPTION, COMMON_OPTIONS_WITH_BUILD,
    COMPILER_OPTION_FIELDS, OPTIONS_FOR_BUILD, OPTIONS_FOR_COMPILER,
};

/// tsgo `CommandLineOptionKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OptionKind {
    String,
    Number,
    Boolean,
    Object,
    List,
    Enum,
}

impl OptionKind {
    /// The kind's name as tsgo's help prints it (`string`, `number`, ...).
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::List => "list",
            Self::Enum => "enum",
        }
    }
}

/// tsgo `DefaultValueDescription`: what the help says an option defaults to.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DefaultValue {
    /// No description (`nil`).
    None,
    /// `core.TSUnknown`.
    Unknown,
    Message(&'static DiagnosticMessage),
    Bool(bool),
    Number(i64),
    Text(&'static str),
    /// An enum constant's value.
    Enum(i64),
}

/// An enum map value: a core enum constant or a library file name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnumValue {
    Number(i64),
    Text(&'static str),
}

/// tsgo `CommandLineOption`, as far as the output reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OptionDeclaration {
    pub(crate) name: &'static str,
    pub(crate) short_name: Option<&'static str>,
    pub(crate) kind: OptionKind,
    pub(crate) is_file_path: bool,
    pub(crate) is_command_line_only: bool,
    pub(crate) show_in_simplified_help_view: bool,
    pub(crate) description: Option<&'static DiagnosticMessage>,
    pub(crate) category: Option<&'static DiagnosticMessage>,
    pub(crate) default: DefaultValue,
}

impl OptionDeclaration {
    /// tsgo `OptionsDeclarations`: every compiler option, in declaration
    /// order.
    pub(crate) fn compiler_options() -> impl Iterator<Item = &'static OptionDeclaration> {
        COMMON_OPTIONS_WITH_BUILD
            .iter()
            .chain(OPTIONS_FOR_COMPILER.iter())
    }

    /// The declaration of a compiler option by its name.
    pub(crate) fn compiler_option(name: &str) -> Option<&'static OptionDeclaration> {
        Self::compiler_options().find(|option| option.name == name)
    }

    pub(crate) fn enum_map(&self) -> Option<&'static [(&'static str, EnumValue)]> {
        if self.kind == OptionKind::Enum {
            enum_map(self.name)
        } else {
            None
        }
    }

    pub(crate) fn elements(&self) -> Option<&'static OptionDeclaration> {
        if self.kind == OptionKind::List {
            elements(self.name)
        } else {
            None
        }
    }

    pub(crate) fn deprecated_keys(&self) -> &'static [&'static str] {
        if self.kind == OptionKind::Enum {
            deprecated_keys(self.name)
        } else {
            &[]
        }
    }

    /// The first key of the enum map whose value is `value` (tsgo's
    /// `getNameOfCompilerOptionValue`).
    pub(crate) fn enum_key(&self, value: EnumValue) -> Option<&'static str> {
        self.enum_map()?
            .iter()
            .find(|(_, candidate)| *candidate == value)
            .map(|(key, _)| *key)
    }

    /// The value of an enum key, compared in lowercase as the parser does.
    pub(crate) fn enum_value(&self, key: &str) -> Option<EnumValue> {
        let key = key.to_ascii_lowercase();
        self.enum_map()?
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, value)| *value)
    }
}
