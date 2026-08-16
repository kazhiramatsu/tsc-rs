//! Typed validation of relationships between effective compiler options.
//!
//! The validator owns semantic option relationships, while callers own the
//! diagnostic location projection appropriate to their input surface. A
//! config parser can therefore attach a violation to retained JSON syntax and
//! a programmatic `createProgram` caller can report the same violation without
//! fabricating a source location.

use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, MessageChain};
use tsc_syntax::is_identifier_text_for_target;
use tsc_types::CompilerOptions;

use crate::prepared::{
    PathMapping, PathsOptionValidationPlan, PathsOptionViolation, PathsOptionViolationKind,
    ProgramOptions,
};

/// Whether TypeScript locates an option diagnostic on an option key or its
/// converted value when config syntax is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerOptionValidationLocation {
    Name,
    Value,
}

/// A failed relationship in the effective [`CompilerOptions`] snapshot.
///
/// Variants deliberately describe the failed invariant rather than a
/// diagnostic code or fixture. This keeps validation reusable by config,
/// command-line, and programmatic compiler entry points.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerOptionViolation {
    StrictPropertyInitializationRequiresStrictNullChecks,
    ExactOptionalPropertyTypesRequiresStrictNullChecks,
    IsolatedDeclarationsConflictsWithAllowJs,
    IsolatedDeclarationsRequiresDeclaration,
    ReactNamespaceConflictsWithJsxFactory,
    JsxFactoryConflictsWithAutomaticRuntime { jsx: &'static str },
    InvalidJsxFactory { value: String },
    InvalidReactNamespace { value: String },
    JsxFragmentFactoryRequiresJsxFactory,
    JsxFragmentFactoryConflictsWithAutomaticRuntime { jsx: &'static str },
    InvalidJsxFragmentFactory { value: String },
    ReactNamespaceConflictsWithAutomaticRuntime { jsx: &'static str },
    JsxImportSourceConflictsWithClassicRuntime,
}

impl CompilerOptionViolation {
    /// Option spellings inspected by `createDiagnosticForOption`. When more
    /// than one is present in retained config syntax, TypeScript reports the
    /// same relationship at every matching property in source order.
    pub const fn option_names(&self) -> &'static [&'static str] {
        match self {
            Self::StrictPropertyInitializationRequiresStrictNullChecks => {
                &["strictPropertyInitialization", "strictNullChecks"]
            }
            Self::ExactOptionalPropertyTypesRequiresStrictNullChecks => {
                &["exactOptionalPropertyTypes", "strictNullChecks"]
            }
            Self::IsolatedDeclarationsConflictsWithAllowJs => &["allowJs", "isolatedDeclarations"],
            Self::IsolatedDeclarationsRequiresDeclaration => {
                &["isolatedDeclarations", "declaration"]
            }
            Self::ReactNamespaceConflictsWithJsxFactory => &["reactNamespace", "jsxFactory"],
            Self::JsxFactoryConflictsWithAutomaticRuntime { .. }
            | Self::InvalidJsxFactory { .. } => &["jsxFactory"],
            Self::InvalidReactNamespace { .. }
            | Self::ReactNamespaceConflictsWithAutomaticRuntime { .. } => &["reactNamespace"],
            Self::JsxFragmentFactoryRequiresJsxFactory => &["jsxFragmentFactory", "jsxFactory"],
            Self::JsxFragmentFactoryConflictsWithAutomaticRuntime { .. }
            | Self::InvalidJsxFragmentFactory { .. } => &["jsxFragmentFactory"],
            Self::JsxImportSourceConflictsWithClassicRuntime => &["jsxImportSource"],
        }
    }

    pub const fn location(&self) -> CompilerOptionValidationLocation {
        match self {
            Self::InvalidJsxFactory { .. }
            | Self::InvalidReactNamespace { .. }
            | Self::InvalidJsxFragmentFactory { .. } => CompilerOptionValidationLocation::Value,
            _ => CompilerOptionValidationLocation::Name,
        }
    }

    pub fn message(&self) -> MessageChain {
        match self {
            Self::StrictPropertyInitializationRequiresStrictNullChecks => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "strictPropertyInitialization".to_owned(),
                    "strictNullChecks".to_owned(),
                ],
            ),
            Self::ExactOptionalPropertyTypesRequiresStrictNullChecks => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "exactOptionalPropertyTypes".to_owned(),
                    "strictNullChecks".to_owned(),
                ],
            ),
            Self::IsolatedDeclarationsConflictsWithAllowJs => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["allowJs".to_owned(), "isolatedDeclarations".to_owned()],
            ),
            Self::IsolatedDeclarationsRequiresDeclaration => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &[
                    "isolatedDeclarations".to_owned(),
                    "declaration".to_owned(),
                    "composite".to_owned(),
                ],
            ),
            Self::ReactNamespaceConflictsWithJsxFactory => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["reactNamespace".to_owned(), "jsxFactory".to_owned()],
            ),
            Self::JsxFactoryConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFactory".to_owned(), (*jsx).to_owned()],
            ),
            Self::InvalidJsxFactory { value } => MessageChain::new(
                &gen::Invalid_value_for_jsxFactory_0_is_not_a_valid_identifier_or_qualified_name,
                std::slice::from_ref(value),
            ),
            Self::InvalidReactNamespace { value } => MessageChain::new(
                &gen::Invalid_value_for_reactNamespace_0_is_not_a_valid_identifier,
                std::slice::from_ref(value),
            ),
            Self::JsxFragmentFactoryRequiresJsxFactory => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "jsxFragmentFactory".to_owned(),
                    "jsxFactory".to_owned(),
                ],
            ),
            Self::JsxFragmentFactoryConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFragmentFactory".to_owned(), (*jsx).to_owned()],
            ),
            Self::InvalidJsxFragmentFactory { value } => MessageChain::new(
                &gen::Invalid_value_for_jsxFragmentFactory_0_is_not_a_valid_identifier_or_qualified_name,
                std::slice::from_ref(value),
            ),
            Self::ReactNamespaceConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["reactNamespace".to_owned(), (*jsx).to_owned()],
            ),
            Self::JsxImportSourceConflictsWithClassicRuntime => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxImportSource".to_owned(), "react".to_owned()],
            ),
        }
    }
}

/// Validate the option relationships in their TypeScript 6.0.3
/// `verifyCompilerOptions` order.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (strict/isolated block)
/// tsc-hash: 2553c0a4e50ebd81142e6a0ef445ce7731ef7a91741f2c604a460c839d033ab9
/// tsc-span: _tsc.js:124751-124768
/// tsc-port: verifyCompilerOptions @6.0.3 (JSX block)
/// tsc-hash: 1972328cb915ef83d963c4d5f7f8abf148aa8651d6188a0dbdeee377490f69ad
/// tsc-span: _tsc.js:124954-124986
pub fn validate_compiler_options(options: &CompilerOptions) -> Vec<CompilerOptionViolation> {
    let mut violations = Vec::new();
    if options.strict_property_initialization == Some(true)
        && !options.strict_option_value(options.strict_null_checks)
    {
        violations
            .push(CompilerOptionViolation::StrictPropertyInitializationRequiresStrictNullChecks);
    }
    if options.exact_optional_property_types == Some(true)
        && !options.strict_option_value(options.strict_null_checks)
    {
        violations
            .push(CompilerOptionViolation::ExactOptionalPropertyTypesRequiresStrictNullChecks);
    }
    if options.isolated_declarations == Some(true) {
        if options.allow_js {
            violations.push(CompilerOptionViolation::IsolatedDeclarationsConflictsWithAllowJs);
        }
        if options.declaration != Some(true) && options.composite != Some(true) {
            violations.push(CompilerOptionViolation::IsolatedDeclarationsRequiresDeclaration);
        }
    }

    let target = options.emit_script_target();
    let jsx_factory = options
        .jsx_factory
        .as_deref()
        .filter(|value| !value.is_empty());
    let jsx_fragment_factory = options
        .jsx_fragment_factory
        .as_deref()
        .filter(|value| !value.is_empty());
    let react_namespace = options
        .react_namespace
        .as_deref()
        .filter(|value| !value.is_empty());
    let jsx_import_source = options
        .jsx_import_source
        .as_deref()
        .filter(|value| !value.is_empty());

    if let Some(factory) = jsx_factory {
        if react_namespace.is_some() {
            violations.push(CompilerOptionViolation::ReactNamespaceConflictsWithJsxFactory);
        }
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations
                .push(CompilerOptionViolation::JsxFactoryConflictsWithAutomaticRuntime { jsx });
        }
        if !is_isolated_entity_name(factory, target) {
            violations.push(CompilerOptionViolation::InvalidJsxFactory {
                value: factory.to_owned(),
            });
        }
    } else if let Some(namespace) = react_namespace {
        if !is_identifier_text_for_target(namespace, target) {
            violations.push(CompilerOptionViolation::InvalidReactNamespace {
                value: namespace.to_owned(),
            });
        }
    }

    if let Some(fragment_factory) = jsx_fragment_factory {
        if jsx_factory.is_none() {
            violations.push(CompilerOptionViolation::JsxFragmentFactoryRequiresJsxFactory);
        }
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations.push(
                CompilerOptionViolation::JsxFragmentFactoryConflictsWithAutomaticRuntime { jsx },
            );
        }
        if !is_isolated_entity_name(fragment_factory, target) {
            violations.push(CompilerOptionViolation::InvalidJsxFragmentFactory {
                value: fragment_factory.to_owned(),
            });
        }
    }
    if react_namespace.is_some() {
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations
                .push(CompilerOptionViolation::ReactNamespaceConflictsWithAutomaticRuntime { jsx });
        }
    }
    if jsx_import_source.is_some() && options.jsx == Some(2) {
        violations.push(CompilerOptionViolation::JsxImportSourceConflictsWithClassicRuntime);
    }
    violations
}

/// Render the raw-sensitive paths validation plan against the final effective
/// compiler options. In particular, TS5090 is intentionally delayed until an
/// embedding has applied its `baseUrl` override.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (paths block)
/// tsc-hash: e18b8511def0edd57da25ed1bbcbd52b5d675efdeba80d8f8e924b5cb2a9b391
/// tsc-span: _tsc.js:124805-124854
pub fn validate_paths_option_diagnostics(
    options: &CompilerOptions,
    program_options: &ProgramOptions,
) -> Vec<Diagnostic> {
    let Some(plan) = program_options.paths_option_validation() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for violation in plan.violations() {
        let message = match violation.kind() {
            PathsOptionViolationKind::PatternHasMultipleAsterisks { pattern } => MessageChain::new(
                &gen::Pattern_0_can_have_at_most_one_character,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::SubstitutionsNotArray { pattern } => MessageChain::new(
                &gen::Substitutions_for_pattern_0_should_be_an_array,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::EmptySubstitutions { pattern } => MessageChain::new(
                &gen::Substitutions_for_pattern_0_shouldn_t_be_an_empty_array,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::SubstitutionHasMultipleAsterisks {
                pattern,
                substitution,
            } => MessageChain::new(
                &gen::Substitution_0_in_pattern_1_can_have_at_most_one_character,
                &[substitution.clone(), pattern.clone()],
            ),
            PathsOptionViolationKind::SubstitutionHasIncorrectType {
                pattern,
                substitution,
                actual_type,
            } => MessageChain::new(
                &gen::Substitution_0_for_pattern_1_has_incorrect_type_expected_string_got_2,
                &[substitution.clone(), pattern.clone(), actual_type.clone()],
            ),
            PathsOptionViolationKind::NonRelativeSubstitutionWithoutBaseUrl => {
                if options
                    .base_url
                    .as_deref()
                    .is_some_and(|base_url| !base_url.is_empty())
                {
                    continue;
                }
                MessageChain::new(
                    &gen::Non_relative_paths_are_not_allowed_when_baseUrl_is_not_set_Did_you_forget_a_leading,
                    &[],
                )
            }
        };
        diagnostics.push(match violation.location() {
            Some(location) => Diagnostic::new(
                Some(location.file_name().to_owned()),
                Some(location.span().start()),
                Some(location.span().length()),
                message,
            ),
            None => Diagnostic::new(None, None, None, message),
        });
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

pub(crate) fn paths_validation_plan_for_typed_mappings(
    mappings: &[PathMapping],
) -> PathsOptionValidationPlan {
    let mut violations = Vec::new();
    for mapping in mappings {
        let pattern = mapping.pattern();
        if !has_zero_or_one_asterisk(pattern) {
            violations.push(PathsOptionViolation::new(
                PathsOptionViolationKind::PatternHasMultipleAsterisks {
                    pattern: pattern.to_owned(),
                },
                None,
            ));
        }
        if mapping.substitutions().is_empty() {
            violations.push(PathsOptionViolation::new(
                PathsOptionViolationKind::EmptySubstitutions {
                    pattern: pattern.to_owned(),
                },
                None,
            ));
        }
        for substitution in mapping.substitutions() {
            if !has_zero_or_one_asterisk(substitution) {
                violations.push(PathsOptionViolation::new(
                    PathsOptionViolationKind::SubstitutionHasMultipleAsterisks {
                        pattern: pattern.to_owned(),
                        substitution: substitution.clone(),
                    },
                    None,
                ));
            }
            if !path_is_relative(substitution) && !path_is_absolute(substitution) {
                violations.push(PathsOptionViolation::new(
                    PathsOptionViolationKind::NonRelativeSubstitutionWithoutBaseUrl,
                    None,
                ));
            }
        }
    }
    PathsOptionValidationPlan::new(violations)
}

/// tsc-port: hasZeroOrOneAsteriskCharacter @6.0.3
/// tsc-hash: 28a64969081ad59009ed6f3fcb192a4ccef94471b01213a5de12c284cdd6eb45
/// tsc-span: _tsc.js:18318-18330
pub(crate) fn has_zero_or_one_asterisk(value: &str) -> bool {
    value.bytes().filter(|byte| *byte == b'*').take(2).count() <= 1
}

/// tsc-port: pathIsRelative @6.0.3
/// tsc-hash: f202555c891d7a914e21c5fe1199667a8d221940ce66c814b4898adfb228aac9
/// tsc-span: _tsc.js:5314-5316
pub(crate) fn path_is_relative(path: &str) -> bool {
    matches!(path, "." | "..")
        || path.starts_with("./")
        || path.starts_with(".\\")
        || path.starts_with("../")
        || path.starts_with("..\\")
}

/// tsc-port: pathIsAbsolute @6.0.3
/// tsc-hash: 0e64b150a899a6eb39ac2a3b370896f59ec02bdefdf07106a8740624318eb3f3
/// tsc-span: _tsc.js:5311-5313
/// tsc-port: getEncodedRootLength @6.0.3 (absolute/nonzero projection)
/// tsc-hash: ad42b701dd98c53ad89476947bccf551e3ab3db9ce0c9fc5009e16a41b49b1f9
/// tsc-span: _tsc.js:5349-5386
pub(crate) fn path_is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return true;
    }
    if bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && (bytes.len() == 2 || matches!(bytes.get(2), Some(b'/' | b'\\')))
    {
        return true;
    }
    // TypeScript recognizes URL roots only with the literal forward-slash
    // separator. Normalizing backslashes first would incorrectly treat
    // `scheme:\\host` as absolute and suppress TS5090.
    path.contains("://")
}

fn automatic_jsx_runtime_name(jsx: Option<i32>) -> Option<&'static str> {
    match jsx {
        Some(4) => Some("react-jsx"),
        Some(5) => Some("react-jsxdev"),
        _ => None,
    }
}

fn is_isolated_entity_name(value: &str, target: tsc_types::ScriptTarget) -> bool {
    let mut parts = value.split('.').map(str::trim);
    let Some(first) = parts.next() else {
        return false;
    };
    !first.is_empty()
        && is_identifier_text_for_target(first, target)
        && parts.all(|part| !part.is_empty() && is_identifier_text_for_target(part, target))
}

#[cfg(test)]
#[path = "../tests/unit/option_validation_tests.rs"]
mod tests;
