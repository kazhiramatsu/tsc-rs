//! Frozen TypeScript 6.0.3 compiler-option metadata.
//!
//! Config parsing needs two properties that are easy to accidentally conflate:
//! exact, case-sensitive property lookup and source-order spelling suggestions.
//! This catalog keeps the declaration order (including the duplicate `help`
//! declaration) so both operations can follow `tsc` without loading its
//! JavaScript bundle at runtime.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerOptionNamedValue {
    name: &'static str,
    value: i32,
}

impl CompilerOptionNamedValue {
    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn value(self) -> i32 {
        self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerOptionNamedStringValue {
    name: &'static str,
    value: &'static str,
}

impl CompilerOptionNamedStringValue {
    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn value(self) -> &'static str {
        self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerOptionListElementKind {
    String,
    FilePath,
    NamedString(&'static [CompilerOptionNamedStringValue]),
    Object,
}

impl CompilerOptionListElementKind {
    /// Resolve a named string element as `convertJsonOptionOfCustomType` does.
    /// All names in TypeScript's compiler-option maps are ASCII.
    pub fn named_string_value(self, name: &str) -> Option<&'static str> {
        let Self::NamedString(values) = self else {
            return None;
        };
        let name = name.to_lowercase();
        values
            .iter()
            .find(|candidate| candidate.name == name.as_str())
            .map(|candidate| candidate.value)
    }

    pub const fn named_string_choices(self) -> Option<&'static [CompilerOptionNamedStringValue]> {
        let Self::NamedString(values) = self else {
            return None;
        };
        Some(values)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerOptionListDescriptor {
    element_name: &'static str,
    element_kind: CompilerOptionListElementKind,
    preserve_falsy_values: bool,
    allow_config_dir_template_substitution: bool,
}

impl CompilerOptionListDescriptor {
    pub const fn element_name(self) -> &'static str {
        self.element_name
    }

    pub const fn element_kind(self) -> CompilerOptionListElementKind {
        self.element_kind
    }

    pub const fn preserve_falsy_values(self) -> bool {
        self.preserve_falsy_values
    }

    pub const fn allow_config_dir_template_substitution(self) -> bool {
        self.allow_config_dir_template_substitution
    }

    pub fn named_string_value(self, name: &str) -> Option<&'static str> {
        self.element_kind.named_string_value(name)
    }

    pub const fn named_string_choices(self) -> Option<&'static [CompilerOptionNamedStringValue]> {
        self.element_kind.named_string_choices()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerOptionObjectDescriptor {
    allow_config_dir_template_substitution: bool,
}

impl CompilerOptionObjectDescriptor {
    pub const fn allow_config_dir_template_substitution(self) -> bool {
        self.allow_config_dir_template_substitution
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerOptionValueKind {
    Boolean,
    Number,
    String,
    Object(CompilerOptionObjectDescriptor),
    List(CompilerOptionListDescriptor),
    Named(&'static [CompilerOptionNamedValue]),
}

impl CompilerOptionValueKind {
    /// Resolve a custom option value as `convertJsonOptionOfCustomType` does.
    /// All names in TypeScript's custom compiler-option maps are ASCII.
    pub fn named_value(self, name: &str) -> Option<i32> {
        let Self::Named(values) = self else {
            return None;
        };
        let name = name.to_lowercase();
        values
            .iter()
            .find(|candidate| candidate.name == name.as_str())
            .map(|candidate| candidate.value)
    }

    pub const fn list_descriptor(self) -> Option<CompilerOptionListDescriptor> {
        let Self::List(descriptor) = self else {
            return None;
        };
        Some(descriptor)
    }

    pub const fn object_descriptor(self) -> Option<CompilerOptionObjectDescriptor> {
        let Self::Object(descriptor) = self else {
            return None;
        };
        Some(descriptor)
    }
}

/// TypeScript 6.0.3's exact libEntries/libMap insertion order.
///
/// This single catalog backs both compiler-option conversion and standard-
/// library loading, so aliases and diagnostic choices cannot drift apart.
///
/// tsc-port: libEntries/libMap @6.0.3
/// tsc-hash: 06ee42a546f222ef70b4aef6f138d2919318f94f96a3cb065ff83ab52eb8de55
/// tsc-span: _tsc.js:36426-36542
pub static TYPESCRIPT_6_0_3_LIBRARIES: &[CompilerOptionNamedStringValue] = &[
    CompilerOptionNamedStringValue {
        name: "es5",
        value: "lib.es5.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es6",
        value: "lib.es2015.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015",
        value: "lib.es2015.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es7",
        value: "lib.es2016.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2016",
        value: "lib.es2016.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017",
        value: "lib.es2017.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018",
        value: "lib.es2018.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019",
        value: "lib.es2019.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020",
        value: "lib.es2020.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2021",
        value: "lib.es2021.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022",
        value: "lib.es2022.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2023",
        value: "lib.es2023.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024",
        value: "lib.es2024.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025",
        value: "lib.es2025.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext",
        value: "lib.esnext.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "dom",
        value: "lib.dom.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "dom.iterable",
        value: "lib.dom.iterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "dom.asynciterable",
        value: "lib.dom.asynciterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "webworker",
        value: "lib.webworker.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "webworker.importscripts",
        value: "lib.webworker.importscripts.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "webworker.iterable",
        value: "lib.webworker.iterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "webworker.asynciterable",
        value: "lib.webworker.asynciterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "scripthost",
        value: "lib.scripthost.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.core",
        value: "lib.es2015.core.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.collection",
        value: "lib.es2015.collection.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.generator",
        value: "lib.es2015.generator.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.iterable",
        value: "lib.es2015.iterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.promise",
        value: "lib.es2015.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.proxy",
        value: "lib.es2015.proxy.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.reflect",
        value: "lib.es2015.reflect.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.symbol",
        value: "lib.es2015.symbol.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2015.symbol.wellknown",
        value: "lib.es2015.symbol.wellknown.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2016.array.include",
        value: "lib.es2016.array.include.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2016.intl",
        value: "lib.es2016.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.arraybuffer",
        value: "lib.es2017.arraybuffer.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.date",
        value: "lib.es2017.date.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.object",
        value: "lib.es2017.object.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.sharedmemory",
        value: "lib.es2017.sharedmemory.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.string",
        value: "lib.es2017.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.intl",
        value: "lib.es2017.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2017.typedarrays",
        value: "lib.es2017.typedarrays.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018.asyncgenerator",
        value: "lib.es2018.asyncgenerator.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018.asynciterable",
        value: "lib.es2018.asynciterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018.intl",
        value: "lib.es2018.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018.promise",
        value: "lib.es2018.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2018.regexp",
        value: "lib.es2018.regexp.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019.array",
        value: "lib.es2019.array.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019.object",
        value: "lib.es2019.object.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019.string",
        value: "lib.es2019.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019.symbol",
        value: "lib.es2019.symbol.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2019.intl",
        value: "lib.es2019.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.bigint",
        value: "lib.es2020.bigint.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.date",
        value: "lib.es2020.date.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.promise",
        value: "lib.es2020.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.sharedmemory",
        value: "lib.es2020.sharedmemory.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.string",
        value: "lib.es2020.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.symbol.wellknown",
        value: "lib.es2020.symbol.wellknown.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.intl",
        value: "lib.es2020.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2020.number",
        value: "lib.es2020.number.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2021.promise",
        value: "lib.es2021.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2021.string",
        value: "lib.es2021.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2021.weakref",
        value: "lib.es2021.weakref.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2021.intl",
        value: "lib.es2021.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.array",
        value: "lib.es2022.array.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.error",
        value: "lib.es2022.error.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.intl",
        value: "lib.es2022.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.object",
        value: "lib.es2022.object.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.string",
        value: "lib.es2022.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2022.regexp",
        value: "lib.es2022.regexp.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2023.array",
        value: "lib.es2023.array.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2023.collection",
        value: "lib.es2023.collection.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2023.intl",
        value: "lib.es2023.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.arraybuffer",
        value: "lib.es2024.arraybuffer.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.collection",
        value: "lib.es2024.collection.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.object",
        value: "lib.es2024.object.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.promise",
        value: "lib.es2024.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.regexp",
        value: "lib.es2024.regexp.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.sharedmemory",
        value: "lib.es2024.sharedmemory.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2024.string",
        value: "lib.es2024.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.collection",
        value: "lib.es2025.collection.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.float16",
        value: "lib.es2025.float16.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.intl",
        value: "lib.es2025.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.iterator",
        value: "lib.es2025.iterator.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.promise",
        value: "lib.es2025.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "es2025.regexp",
        value: "lib.es2025.regexp.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.asynciterable",
        value: "lib.es2018.asynciterable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.symbol",
        value: "lib.es2019.symbol.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.bigint",
        value: "lib.es2020.bigint.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.weakref",
        value: "lib.es2021.weakref.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.object",
        value: "lib.es2024.object.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.regexp",
        value: "lib.es2024.regexp.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.string",
        value: "lib.es2024.string.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.float16",
        value: "lib.es2025.float16.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.iterator",
        value: "lib.es2025.iterator.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.promise",
        value: "lib.es2025.promise.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.array",
        value: "lib.esnext.array.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.collection",
        value: "lib.esnext.collection.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.date",
        value: "lib.esnext.date.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.decorators",
        value: "lib.esnext.decorators.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.disposable",
        value: "lib.esnext.disposable.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.error",
        value: "lib.esnext.error.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.intl",
        value: "lib.esnext.intl.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.sharedmemory",
        value: "lib.esnext.sharedmemory.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.temporal",
        value: "lib.esnext.temporal.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "esnext.typedarrays",
        value: "lib.esnext.typedarrays.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "decorators",
        value: "lib.decorators.d.ts",
    },
    CompilerOptionNamedStringValue {
        name: "decorators.legacy",
        value: "lib.decorators.legacy.d.ts",
    },
];

pub const LIB_LIST_DESCRIPTOR: CompilerOptionListDescriptor = CompilerOptionListDescriptor {
    element_name: "lib",
    element_kind: CompilerOptionListElementKind::NamedString(TYPESCRIPT_6_0_3_LIBRARIES),
    preserve_falsy_values: false,
    allow_config_dir_template_substitution: false,
};

pub const ROOT_DIRS_LIST_DESCRIPTOR: CompilerOptionListDescriptor = CompilerOptionListDescriptor {
    element_name: "rootDirs",
    element_kind: CompilerOptionListElementKind::FilePath,
    preserve_falsy_values: false,
    allow_config_dir_template_substitution: true,
};

pub const TYPE_ROOTS_LIST_DESCRIPTOR: CompilerOptionListDescriptor = CompilerOptionListDescriptor {
    element_name: "typeRoots",
    element_kind: CompilerOptionListElementKind::FilePath,
    preserve_falsy_values: false,
    allow_config_dir_template_substitution: true,
};

pub const TYPES_LIST_DESCRIPTOR: CompilerOptionListDescriptor = CompilerOptionListDescriptor {
    element_name: "types",
    element_kind: CompilerOptionListElementKind::String,
    preserve_falsy_values: false,
    allow_config_dir_template_substitution: false,
};

pub const MODULE_SUFFIXES_LIST_DESCRIPTOR: CompilerOptionListDescriptor =
    CompilerOptionListDescriptor {
        element_name: "suffix",
        element_kind: CompilerOptionListElementKind::String,
        preserve_falsy_values: true,
        allow_config_dir_template_substitution: false,
    };

pub const CUSTOM_CONDITIONS_LIST_DESCRIPTOR: CompilerOptionListDescriptor =
    CompilerOptionListDescriptor {
        element_name: "condition",
        element_kind: CompilerOptionListElementKind::String,
        preserve_falsy_values: false,
        allow_config_dir_template_substitution: false,
    };

pub const PLUGINS_LIST_DESCRIPTOR: CompilerOptionListDescriptor = CompilerOptionListDescriptor {
    element_name: "plugin",
    element_kind: CompilerOptionListElementKind::Object,
    preserve_falsy_values: false,
    allow_config_dir_template_substitution: false,
};

/// tsc-port: paths option declaration @6.0.3
/// tsc-hash: 45b11e8144176a93da60d3ace7c618f3d9d1dffebbd0bc610df8e04ebc45ffcf
/// tsc-span: _tsc.js:37363-37374
pub const PATHS_OBJECT_DESCRIPTOR: CompilerOptionObjectDescriptor =
    CompilerOptionObjectDescriptor {
        allow_config_dir_template_substitution: true,
    };

pub const fn typescript_6_0_3_libraries() -> &'static [CompilerOptionNamedStringValue] {
    TYPESCRIPT_6_0_3_LIBRARIES
}

pub(crate) fn typescript_6_0_3_library_value(name: &str) -> Option<&'static str> {
    LIB_LIST_DESCRIPTOR.named_string_value(name)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsConfigDefaultValue {
    Boolean(bool),
    Number(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerOptionDeclaration {
    name: &'static str,
    value_kind: CompilerOptionValueKind,
    is_file_path: bool,
    is_command_line_only: bool,
    is_tsconfig_only: bool,
    jsconfig_default: Option<JsConfigDefaultValue>,
}

impl CompilerOptionDeclaration {
    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn value_kind(self) -> CompilerOptionValueKind {
        self.value_kind
    }

    pub const fn is_file_path(self) -> bool {
        self.is_file_path
    }

    pub const fn is_command_line_only(self) -> bool {
        self.is_command_line_only
    }

    pub const fn is_tsconfig_only(self) -> bool {
        self.is_tsconfig_only
    }

    pub const fn jsconfig_default(self) -> Option<JsConfigDefaultValue> {
        self.jsconfig_default
    }
}

const fn option(
    name: &'static str,
    value_kind: CompilerOptionValueKind,
) -> CompilerOptionDeclaration {
    CompilerOptionDeclaration {
        name,
        value_kind,
        is_file_path: false,
        is_command_line_only: false,
        is_tsconfig_only: false,
        jsconfig_default: None,
    }
}

const fn file_option(
    name: &'static str,
    value_kind: CompilerOptionValueKind,
) -> CompilerOptionDeclaration {
    CompilerOptionDeclaration {
        is_file_path: true,
        ..option(name, value_kind)
    }
}

const fn command_line_option(
    name: &'static str,
    value_kind: CompilerOptionValueKind,
) -> CompilerOptionDeclaration {
    CompilerOptionDeclaration {
        is_command_line_only: true,
        ..option(name, value_kind)
    }
}

const fn tsconfig_option(
    name: &'static str,
    value_kind: CompilerOptionValueKind,
) -> CompilerOptionDeclaration {
    CompilerOptionDeclaration {
        is_tsconfig_only: true,
        ..option(name, value_kind)
    }
}

const fn jsconfig_option(
    name: &'static str,
    value_kind: CompilerOptionValueKind,
    jsconfig_default: JsConfigDefaultValue,
) -> CompilerOptionDeclaration {
    CompilerOptionDeclaration {
        jsconfig_default: Some(jsconfig_default),
        ..option(name, value_kind)
    }
}

const TARGET_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "es3",
        value: 0,
    },
    CompilerOptionNamedValue {
        name: "es5",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "es6",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "es2015",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "es2016",
        value: 3,
    },
    CompilerOptionNamedValue {
        name: "es2017",
        value: 4,
    },
    CompilerOptionNamedValue {
        name: "es2018",
        value: 5,
    },
    CompilerOptionNamedValue {
        name: "es2019",
        value: 6,
    },
    CompilerOptionNamedValue {
        name: "es2020",
        value: 7,
    },
    CompilerOptionNamedValue {
        name: "es2021",
        value: 8,
    },
    CompilerOptionNamedValue {
        name: "es2022",
        value: 9,
    },
    CompilerOptionNamedValue {
        name: "es2023",
        value: 10,
    },
    CompilerOptionNamedValue {
        name: "es2024",
        value: 11,
    },
    CompilerOptionNamedValue {
        name: "es2025",
        value: 12,
    },
    CompilerOptionNamedValue {
        name: "esnext",
        value: 99,
    },
];

const MODULE_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "none",
        value: 0,
    },
    CompilerOptionNamedValue {
        name: "commonjs",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "amd",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "system",
        value: 4,
    },
    CompilerOptionNamedValue {
        name: "umd",
        value: 3,
    },
    CompilerOptionNamedValue {
        name: "es6",
        value: 5,
    },
    CompilerOptionNamedValue {
        name: "es2015",
        value: 5,
    },
    CompilerOptionNamedValue {
        name: "es2020",
        value: 6,
    },
    CompilerOptionNamedValue {
        name: "es2022",
        value: 7,
    },
    CompilerOptionNamedValue {
        name: "esnext",
        value: 99,
    },
    CompilerOptionNamedValue {
        name: "node16",
        value: 100,
    },
    CompilerOptionNamedValue {
        name: "node18",
        value: 101,
    },
    CompilerOptionNamedValue {
        name: "node20",
        value: 102,
    },
    CompilerOptionNamedValue {
        name: "nodenext",
        value: 199,
    },
    CompilerOptionNamedValue {
        name: "preserve",
        value: 200,
    },
];

const JSX_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "preserve",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "react-native",
        value: 3,
    },
    CompilerOptionNamedValue {
        name: "react-jsx",
        value: 4,
    },
    CompilerOptionNamedValue {
        name: "react-jsxdev",
        value: 5,
    },
    CompilerOptionNamedValue {
        name: "react",
        value: 2,
    },
];

const IMPORTS_NOT_USED_AS_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "remove",
        value: 0,
    },
    CompilerOptionNamedValue {
        name: "preserve",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "error",
        value: 2,
    },
];

const MODULE_RESOLUTION_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "node10",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "node",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "classic",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "node16",
        value: 3,
    },
    CompilerOptionNamedValue {
        name: "nodenext",
        value: 99,
    },
    CompilerOptionNamedValue {
        name: "bundler",
        value: 100,
    },
];

const NEW_LINE_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "crlf",
        value: 0,
    },
    CompilerOptionNamedValue {
        name: "lf",
        value: 1,
    },
];

const MODULE_DETECTION_VALUES: &[CompilerOptionNamedValue] = &[
    CompilerOptionNamedValue {
        name: "auto",
        value: 2,
    },
    CompilerOptionNamedValue {
        name: "legacy",
        value: 1,
    },
    CompilerOptionNamedValue {
        name: "force",
        value: 3,
    },
];

/// TypeScript's compiler option declarations in their observable 6.0.3 order.
///
/// The full JSON kind is recorded for every declaration. `List` carries the
/// element schema, falsy filtering mode, and `${configDir}` substitution bit;
/// `Object` retains the corresponding substitution metadata for `paths`;
/// `Named` preserves each custom map's spelling order and numeric compiler
/// value.
///
/// tsc-port: optionDeclarations @6.0.3
/// tsc-hash: fce4b03c1ee24c384f0c7d0d62dd1fcc3e7b8af7e8d16a000e8bc7b5f1ac461c
/// tsc-span: _tsc.js:37925-37928
pub static COMPILER_OPTION_DECLARATIONS: &[CompilerOptionDeclaration] = &[
    command_line_option("help", CompilerOptionValueKind::Boolean),
    command_line_option("help", CompilerOptionValueKind::Boolean),
    command_line_option("watch", CompilerOptionValueKind::Boolean),
    option("preserveWatchOutput", CompilerOptionValueKind::Boolean),
    option("listFiles", CompilerOptionValueKind::Boolean),
    option("explainFiles", CompilerOptionValueKind::Boolean),
    option("listEmittedFiles", CompilerOptionValueKind::Boolean),
    option("pretty", CompilerOptionValueKind::Boolean),
    option("traceResolution", CompilerOptionValueKind::Boolean),
    option("diagnostics", CompilerOptionValueKind::Boolean),
    option("extendedDiagnostics", CompilerOptionValueKind::Boolean),
    file_option("generateCpuProfile", CompilerOptionValueKind::String),
    file_option("generateTrace", CompilerOptionValueKind::String),
    option("incremental", CompilerOptionValueKind::Boolean),
    option("declaration", CompilerOptionValueKind::Boolean),
    option("declarationMap", CompilerOptionValueKind::Boolean),
    option("emitDeclarationOnly", CompilerOptionValueKind::Boolean),
    option("sourceMap", CompilerOptionValueKind::Boolean),
    option("inlineSourceMap", CompilerOptionValueKind::Boolean),
    option("noCheck", CompilerOptionValueKind::Boolean),
    jsconfig_option(
        "noEmit",
        CompilerOptionValueKind::Boolean,
        JsConfigDefaultValue::Boolean(true),
    ),
    option(
        "assumeChangesOnlyAffectDirectDependencies",
        CompilerOptionValueKind::Boolean,
    ),
    command_line_option("locale", CompilerOptionValueKind::String),
    option("all", CompilerOptionValueKind::Boolean),
    option("version", CompilerOptionValueKind::Boolean),
    option("init", CompilerOptionValueKind::Boolean),
    file_option("project", CompilerOptionValueKind::String),
    command_line_option("showConfig", CompilerOptionValueKind::Boolean),
    command_line_option("listFilesOnly", CompilerOptionValueKind::Boolean),
    command_line_option("ignoreConfig", CompilerOptionValueKind::Boolean),
    option("target", CompilerOptionValueKind::Named(TARGET_VALUES)),
    option("module", CompilerOptionValueKind::Named(MODULE_VALUES)),
    option("lib", CompilerOptionValueKind::List(LIB_LIST_DESCRIPTOR)),
    jsconfig_option(
        "allowJs",
        CompilerOptionValueKind::Boolean,
        JsConfigDefaultValue::Boolean(true),
    ),
    option("checkJs", CompilerOptionValueKind::Boolean),
    option("jsx", CompilerOptionValueKind::Named(JSX_VALUES)),
    file_option("outFile", CompilerOptionValueKind::String),
    file_option("outDir", CompilerOptionValueKind::String),
    file_option("rootDir", CompilerOptionValueKind::String),
    tsconfig_option("composite", CompilerOptionValueKind::Boolean),
    file_option("tsBuildInfoFile", CompilerOptionValueKind::String),
    option("removeComments", CompilerOptionValueKind::Boolean),
    option("importHelpers", CompilerOptionValueKind::Boolean),
    option(
        "importsNotUsedAsValues",
        CompilerOptionValueKind::Named(IMPORTS_NOT_USED_AS_VALUES),
    ),
    option("downlevelIteration", CompilerOptionValueKind::Boolean),
    option("isolatedModules", CompilerOptionValueKind::Boolean),
    option("verbatimModuleSyntax", CompilerOptionValueKind::Boolean),
    option("isolatedDeclarations", CompilerOptionValueKind::Boolean),
    option("erasableSyntaxOnly", CompilerOptionValueKind::Boolean),
    option("libReplacement", CompilerOptionValueKind::Boolean),
    option("strict", CompilerOptionValueKind::Boolean),
    option("noImplicitAny", CompilerOptionValueKind::Boolean),
    option("strictNullChecks", CompilerOptionValueKind::Boolean),
    option("strictFunctionTypes", CompilerOptionValueKind::Boolean),
    option("strictBindCallApply", CompilerOptionValueKind::Boolean),
    option(
        "strictPropertyInitialization",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "strictBuiltinIteratorReturn",
        CompilerOptionValueKind::Boolean,
    ),
    option("stableTypeOrdering", CompilerOptionValueKind::Boolean),
    option("noImplicitThis", CompilerOptionValueKind::Boolean),
    option(
        "useUnknownInCatchVariables",
        CompilerOptionValueKind::Boolean,
    ),
    option("alwaysStrict", CompilerOptionValueKind::Boolean),
    option("noUnusedLocals", CompilerOptionValueKind::Boolean),
    option("noUnusedParameters", CompilerOptionValueKind::Boolean),
    option(
        "exactOptionalPropertyTypes",
        CompilerOptionValueKind::Boolean,
    ),
    option("noImplicitReturns", CompilerOptionValueKind::Boolean),
    option(
        "noFallthroughCasesInSwitch",
        CompilerOptionValueKind::Boolean,
    ),
    option("noUncheckedIndexedAccess", CompilerOptionValueKind::Boolean),
    option("noImplicitOverride", CompilerOptionValueKind::Boolean),
    option(
        "noPropertyAccessFromIndexSignature",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "moduleResolution",
        CompilerOptionValueKind::Named(MODULE_RESOLUTION_VALUES),
    ),
    file_option("baseUrl", CompilerOptionValueKind::String),
    tsconfig_option(
        "paths",
        CompilerOptionValueKind::Object(PATHS_OBJECT_DESCRIPTOR),
    ),
    tsconfig_option(
        "rootDirs",
        CompilerOptionValueKind::List(ROOT_DIRS_LIST_DESCRIPTOR),
    ),
    option(
        "typeRoots",
        CompilerOptionValueKind::List(TYPE_ROOTS_LIST_DESCRIPTOR),
    ),
    option(
        "types",
        CompilerOptionValueKind::List(TYPES_LIST_DESCRIPTOR),
    ),
    jsconfig_option(
        "allowSyntheticDefaultImports",
        CompilerOptionValueKind::Boolean,
        JsConfigDefaultValue::Boolean(true),
    ),
    option("esModuleInterop", CompilerOptionValueKind::Boolean),
    option("preserveSymlinks", CompilerOptionValueKind::Boolean),
    option("allowUmdGlobalAccess", CompilerOptionValueKind::Boolean),
    option(
        "moduleSuffixes",
        CompilerOptionValueKind::List(MODULE_SUFFIXES_LIST_DESCRIPTOR),
    ),
    option(
        "allowImportingTsExtensions",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "rewriteRelativeImportExtensions",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "resolvePackageJsonExports",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "resolvePackageJsonImports",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "customConditions",
        CompilerOptionValueKind::List(CUSTOM_CONDITIONS_LIST_DESCRIPTOR),
    ),
    option(
        "noUncheckedSideEffectImports",
        CompilerOptionValueKind::Boolean,
    ),
    option("sourceRoot", CompilerOptionValueKind::String),
    option("mapRoot", CompilerOptionValueKind::String),
    option("inlineSources", CompilerOptionValueKind::Boolean),
    option("experimentalDecorators", CompilerOptionValueKind::Boolean),
    option("emitDecoratorMetadata", CompilerOptionValueKind::Boolean),
    option("jsxFactory", CompilerOptionValueKind::String),
    option("jsxFragmentFactory", CompilerOptionValueKind::String),
    option("jsxImportSource", CompilerOptionValueKind::String),
    option("resolveJsonModule", CompilerOptionValueKind::Boolean),
    option("allowArbitraryExtensions", CompilerOptionValueKind::Boolean),
    option("out", CompilerOptionValueKind::String),
    option("reactNamespace", CompilerOptionValueKind::String),
    option("skipDefaultLibCheck", CompilerOptionValueKind::Boolean),
    option("charset", CompilerOptionValueKind::String),
    option("emitBOM", CompilerOptionValueKind::Boolean),
    option("newLine", CompilerOptionValueKind::Named(NEW_LINE_VALUES)),
    option("noErrorTruncation", CompilerOptionValueKind::Boolean),
    option("noLib", CompilerOptionValueKind::Boolean),
    option("noResolve", CompilerOptionValueKind::Boolean),
    option("stripInternal", CompilerOptionValueKind::Boolean),
    option("disableSizeLimit", CompilerOptionValueKind::Boolean),
    tsconfig_option(
        "disableSourceOfProjectReferenceRedirect",
        CompilerOptionValueKind::Boolean,
    ),
    tsconfig_option("disableSolutionSearching", CompilerOptionValueKind::Boolean),
    tsconfig_option(
        "disableReferencedProjectLoad",
        CompilerOptionValueKind::Boolean,
    ),
    option("noImplicitUseStrict", CompilerOptionValueKind::Boolean),
    option("noEmitHelpers", CompilerOptionValueKind::Boolean),
    option("noEmitOnError", CompilerOptionValueKind::Boolean),
    option("preserveConstEnums", CompilerOptionValueKind::Boolean),
    file_option("declarationDir", CompilerOptionValueKind::String),
    jsconfig_option(
        "skipLibCheck",
        CompilerOptionValueKind::Boolean,
        JsConfigDefaultValue::Boolean(true),
    ),
    option("allowUnusedLabels", CompilerOptionValueKind::Boolean),
    option("allowUnreachableCode", CompilerOptionValueKind::Boolean),
    option(
        "suppressExcessPropertyErrors",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "suppressImplicitAnyIndexErrors",
        CompilerOptionValueKind::Boolean,
    ),
    option(
        "forceConsistentCasingInFileNames",
        CompilerOptionValueKind::Boolean,
    ),
    jsconfig_option(
        "maxNodeModuleJsDepth",
        CompilerOptionValueKind::Number,
        JsConfigDefaultValue::Number(2),
    ),
    option("noStrictGenericChecks", CompilerOptionValueKind::Boolean),
    option("useDefineForClassFields", CompilerOptionValueKind::Boolean),
    option("preserveValueImports", CompilerOptionValueKind::Boolean),
    option("keyofStringsOnly", CompilerOptionValueKind::Boolean),
    tsconfig_option(
        "plugins",
        CompilerOptionValueKind::List(PLUGINS_LIST_DESCRIPTOR),
    ),
    option(
        "moduleDetection",
        CompilerOptionValueKind::Named(MODULE_DETECTION_VALUES),
    ),
    option("ignoreDeprecations", CompilerOptionValueKind::String),
];

/// Return the frozen declaration sequence used by spelling suggestions.
pub const fn compiler_option_declarations() -> &'static [CompilerOptionDeclaration] {
    COMPILER_OPTION_DECLARATIONS
}

/// Look up a `compilerOptions` property using TypeScript's exact spelling.
///
/// `convertOptionsFromJson` uses a case-sensitive `Map.get`, unlike CLI option
/// lookup, so `allowJs` is recognized while `ALLOWJS` is not.
pub fn compiler_option_declaration(name: &str) -> Option<&'static CompilerOptionDeclaration> {
    COMPILER_OPTION_DECLARATIONS
        .iter()
        .rfind(|declaration| declaration.name == name)
}

/// Whether a root property participates in TypeScript's misplaced compiler
/// option hint.
///
/// `optionDeclarations` is the concatenation of `commonOptionsWithBuild` and
/// `commandOptionsWithoutBuild`; the latter begins at the unique `all`
/// declaration in the frozen catalog. Keeping this query tied to that pinned
/// order avoids incorrectly diagnosing common build/watch options at the
/// config root.
pub fn is_command_option_without_build(name: &str) -> bool {
    COMPILER_OPTION_DECLARATIONS
        .iter()
        .skip_while(|declaration| declaration.name != "all")
        .any(|declaration| declaration.name == name)
}

/// Suggest a compiler-option spelling in TypeScript declaration order.
///
/// tsc-port: getSpellingSuggestion @6.0.3
/// tsc-hash: 37b9cd417fd83af45f9fa8584ae1a3aa05e3f7ac3764438bb0627a7d61591ab6
/// tsc-span: _tsc.js:951-975
pub fn compiler_option_spelling_suggestion(
    name: &str,
) -> Option<&'static CompilerOptionDeclaration> {
    let name_units = name.encode_utf16().collect::<Vec<_>>();
    let maximum_length_difference = 2usize.max((name_units.len() as f64 * 0.34).floor() as usize);
    let mut best_distance = (name_units.len() as f64 * 0.4).floor() + 1.0;
    let mut best_candidate = None;
    for candidate in COMPILER_OPTION_DECLARATIONS {
        let candidate_units = candidate.name.encode_utf16().collect::<Vec<_>>();
        if name_units.len().abs_diff(candidate_units.len()) > maximum_length_difference
            || candidate.name == name
            || (candidate_units.len() < 3 && candidate.name.to_lowercase() != name.to_lowercase())
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

fn lowercase_utf16_unit(unit: u16) -> Vec<u16> {
    match char::from_u32(u32::from(unit)) {
        Some(scalar) => scalar
            .to_lowercase()
            .flat_map(|lowered| {
                let mut buffer = [0u16; 2];
                lowered.encode_utf16(&mut buffer).to_vec()
            })
            .collect(),
        // JavaScript string indexing exposes an unpaired surrogate as a
        // one-code-unit string; lowercasing leaves it unchanged.
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
            (left_index as f64 - maximum).ceil() as usize
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
            let substitution = if lowercase_utf16_unit(left[left_index - 1])
                == lowercase_utf16_unit(right[right_index - 1])
            {
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

/// TypeScript's filename-sensitive defaults in object insertion order.
///
/// tsc-port: getDefaultCompilerOptions @6.0.3
/// tsc-hash: 34b70a77540f5b4d751fb295bdb7c59a2edb4043e98950027945840ed7646d80
/// tsc-span: _tsc.js:39507-39510
pub static JSCONFIG_DEFAULTS: &[(&str, JsConfigDefaultValue)] = &[
    ("allowJs", JsConfigDefaultValue::Boolean(true)),
    ("maxNodeModuleJsDepth", JsConfigDefaultValue::Number(2)),
    (
        "allowSyntheticDefaultImports",
        JsConfigDefaultValue::Boolean(true),
    ),
    ("skipLibCheck", JsConfigDefaultValue::Boolean(true)),
    ("noEmit", JsConfigDefaultValue::Boolean(true)),
];

pub const fn jsconfig_defaults() -> &'static [(&'static str, JsConfigDefaultValue)] {
    JSCONFIG_DEFAULTS
}
