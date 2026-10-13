//! tsgo `ls/lsutil/userpreferences_test.go`.

use serde_json::{json, Map, Value};
use tsc_checker::{ImportModuleSpecifierEndingPreference, ImportModuleSpecifierPreference};

use crate::lsutil::*;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

/// A case's change to the expected preferences.
type Change = Box<dyn Fn(&mut UserPreferences)>;

/// tsgo `TestUserPreferencesParsingEdgeCases`.
#[test]
fn parsing_edge_cases() {
    let mut base = UserPreferences::new_default();
    base.quote_preference = QuotePreference::DOUBLE;
    base.maximum_hover_length = 17;
    base.organize_imports_ignore_case = Some(true);
    let cases: Vec<(&str, Value, Change)> = vec![
        (
            "null raw values leave preferences unchanged",
            json!({"quotePreference": null, "maximumHoverLength": null, "includeCompletionsForModuleExports": null}),
            Box::new(|_| {}),
        ),
        (
            "invalid boolean becomes unknown",
            json!({"includeCompletionsForModuleExports": "invalid"}),
            Box::new(|p| {
                p.completion_preferences
                    .include_completions_for_module_exports = None
            }),
        ),
        (
            "case-insensitive enums",
            json!({
                "quotePreference": "SINGLE", "jsxAttributeCompletionStyle": "BRACES",
                "organizeImportsCaseFirst": "LOWER", "includeInlayParameterNameHints": "ALL",
                "organizeImportsTypeOrder": "FIRST", "workspaceSymbolsScope": "CURRENTPROJECT",
            }),
            Box::new(|p| {
                p.quote_preference = QuotePreference::SINGLE;
                p.completion_preferences.jsx_attribute_completion_style =
                    JsxAttributeCompletionStyle::BRACES;
                p.organize_imports_case_first = OrganizeImportsCaseFirst::LOWER;
                p.inlay_hints_preferences.include_inlay_parameter_name_hints =
                    IncludeInlayParameterNameHints::ALL;
                p.organize_imports_type_order = OrganizeImportsTypeOrder::FIRST;
                p.workspace_symbols_scope = WorkspaceSymbolsScope::CURRENT_PROJECT;
            }),
        ),
        (
            "present null primary path prevents fallback",
            json!({"suggest": {"jsdoc": {"enabled": null}, "completeJSDocs": false}}),
            Box::new(|_| {}),
        ),
        (
            "null case sensitivity becomes unknown",
            json!({"preferences": {"organizeImports": {"caseSensitivity": null}}}),
            Box::new(|p| p.organize_imports_ignore_case = None),
        ),
        (
            "numeric conversion and array filtering",
            json!({
                "maximumHoverLength": 9.8, "indentStyle": 1.7,
                "autoImportFileExcludePatterns": ["first", false, 3, "second"],
            }),
            Box::new(|p| {
                p.maximum_hover_length = 9;
                p.format_code_settings.editor_settings.indent_style = IndentStyle::BLOCK;
                p.auto_import_file_exclude_patterns =
                    Some(vec!["first".to_owned(), "second".to_owned()]);
            }),
        ),
        (
            "empty array is not nil",
            json!({"autoImportFileExcludePatterns": []}),
            Box::new(|p| p.auto_import_file_exclude_patterns = Some(Vec::new())),
        ),
        (
            "invalid module specifier preference uses its default",
            json!({"importModuleSpecifierPreference": true}),
            Box::new(|p| {
                p.import_module_specifier_preference = ImportModuleSpecifierPreference::SHORTEST;
            }),
        ),
    ];
    for (name, config, change) in cases {
        let mut expected = base.clone();
        change(&mut expected);
        assert_eq!(
            base.clone().with_config(&object(config)),
            expected,
            "{name}"
        );
    }
}

/// The value tsgo's `fillNonZeroValues` gives each preference, as the raw
/// configuration that sets it: a tristate is false (Go's `Tristate(1)`), a
/// Go bool true, an integer 1 (an integer enumeration its value 1), a
/// string `test` (an enumeration the value `getValidStringValue` picks),
/// a list `["test"]`.
fn non_zero_config() -> (Map<String, Value>, usize) {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tools/userPreferences.schema.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut config = Map::new();
    for definition in schema["$defs"].as_object().unwrap().values() {
        for (name, property) in definition["properties"].as_object().unwrap() {
            let value = match (name.as_str(), property["type"].as_str().unwrap()) {
                ("quotePreference", _) => json!("single"),
                ("workspaceSymbolsScope", _) => json!("allOpenProjects"),
                ("jsxAttributeCompletionStyle", _) => json!("braces"),
                ("includeInlayParameterNameHints", _) => json!("all"),
                ("semicolons", _) => json!("insert"),
                ("importModuleSpecifierPreference", _) => json!("relative"),
                ("importModuleSpecifierEnding", _) => json!("js"),
                ("indentStyle", _) => json!("block"),
                ("organizeImportsSort", _) => json!("ordinal"),
                ("organizeImportsCaseFirst", _) => json!("lower"),
                ("organizeImportsTypeOrder", _) => json!("last"),
                ("organizeImportsCollation", _) => json!("unicode"),
                ("preferGoToSourceDefinition", _) => json!(true),
                (_, "boolean") => json!(false),
                (_, "integer") => json!(1),
                (_, "string") => json!("test"),
                (_, "array") => json!(["test"]),
                (_, other) => panic!("{name}: {other}"),
            };
            config.insert(name.clone(), value);
        }
    }
    let count = config.len();
    (config, count)
}

fn leaf_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.values().map(leaf_count).sum(),
        _ => 1,
    }
}

/// tsgo `TestUserPreferencesRoundtrip`: every preference set survives its
/// configuration form, read by `from_json` (tsgo `UnmarshalJSONFrom`) and
/// by `with_config`.
#[test]
fn roundtrip() {
    let (config, count) = non_zero_config();
    let original = UserPreferences::default().with_config(&config);
    let serialized = original.to_config();
    // Every preference is set: each has its path in the configuration form.
    assert_eq!(leaf_count(&serialized), count);
    assert_eq!(
        UserPreferences::from_json(&serialized),
        Ok(original.clone())
    );
    assert_eq!(
        UserPreferences::default().with_config(serialized.as_object().unwrap()),
        original
    );
}

/// tsgo `TestUserPreferencesSerialize`.
#[test]
fn serialize() {
    let preferences = UserPreferences {
        quote_preference: QuotePreference::SINGLE,
        ..UserPreferences::default()
    };
    assert_eq!(
        preferences.to_config()["preferences"]["quoteStyle"],
        "single"
    );

    let preferences = UserPreferences {
        disable_suggestions: Some(true),
        ..UserPreferences::default()
    };
    assert_eq!(
        preferences.to_config()["unstable"]["disableSuggestions"],
        true
    );

    let mut preferences = UserPreferences::default();
    preferences
        .inlay_hints_preferences
        .include_inlay_parameter_name_hints = IncludeInlayParameterNameHints::ALL;
    preferences
        .inlay_hints_preferences
        .include_inlay_parameter_name_hints_when_argument_matches_name = Some(true);
    let names = &preferences.to_config()["inlayHints"]["parameterNames"];
    assert_eq!(names["enabled"], "all");
    // Inverted.
    assert_eq!(names["suppressWhenArgumentMatchesName"], false);

    let preferences = UserPreferences {
        quote_preference: QuotePreference::SINGLE,
        disable_suggestions: Some(true),
        display_parts_for_js_doc: Some(true),
        ..UserPreferences::default()
    };
    let config = preferences.to_config();
    assert_eq!(config["preferences"]["quoteStyle"], "single");
    assert_eq!(config["unstable"]["disableSuggestions"], true);
    assert_eq!(config["unstable"]["displayPartsForJSDoc"], true);
}

/// Go's deterministic JSON: every object's keys in the configuration form
/// are sorted.
#[test]
fn serialized_keys_are_sorted() {
    fn sorted(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.keys()
                    .zip(map.keys().skip(1))
                    .all(|(left, right)| left < right)
                    && map.values().all(sorted)
            }
            _ => true,
        }
    }
    let (config, _) = non_zero_config();
    assert!(sorted(
        &UserPreferences::default().with_config(&config).to_config()
    ));
}

/// tsgo `TestUserPreferencesParseUnstable`.
#[test]
fn parse_unstable() {
    let cases: Vec<(&str, Value, UserPreferences)> = vec![
        (
            "unstable fields with correct casing",
            json!({"unstable": {"disableSuggestions": true, "maximumHoverLength": 100, "allowRenameOfImportPath": true}}),
            UserPreferences {
                disable_suggestions: Some(true),
                maximum_hover_length: 100,
                allow_rename_of_import_path: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "nested preferences path",
            json!({"preferences": {"quoteStyle": "single", "useAliasesForRenames": true}}),
            UserPreferences {
                quote_preference: QuotePreference::SINGLE,
                provide_prefix_and_suffix_text_for_rename: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "suggest section",
            json!({"suggest": {"autoImports": false, "includeCompletionsForImportStatements": true}}),
            UserPreferences {
                completion_preferences: CompletionPreferences {
                    include_completions_for_module_exports: Some(false),
                    include_completions_for_import_statements: Some(true),
                    ..CompletionPreferences::default()
                },
                ..UserPreferences::default()
            },
        ),
        (
            "inlayHints with invert",
            json!({"inlayHints": {"parameterNames": {"enabled": "all", "suppressWhenArgumentMatchesName": true}}}),
            UserPreferences {
                inlay_hints_preferences: InlayHintsPreferences {
                    include_inlay_parameter_name_hints: IncludeInlayParameterNameHints::ALL,
                    // Inverted.
                    include_inlay_parameter_name_hints_when_argument_matches_name: Some(false),
                    ..InlayHintsPreferences::default()
                },
                ..UserPreferences::default()
            },
        ),
        (
            "mixed config",
            json!({
                "unstable": {"displayPartsForJSDoc": true},
                "preferences": {"importModuleSpecifier": "relative"},
                "workspaceSymbols": {"excludeLibrarySymbols": true, "scope": "currentProject"},
            }),
            UserPreferences {
                display_parts_for_js_doc: Some(true),
                import_module_specifier_preference: ImportModuleSpecifierPreference::RELATIVE,
                exclude_library_symbols_in_nav_to: Some(true),
                workspace_symbols_scope: WorkspaceSymbolsScope::CURRENT_PROJECT,
                ..UserPreferences::default()
            },
        ),
        (
            "stable config overrides unstable",
            json!({"unstable": {"quotePreference": "double"}, "preferences": {"quoteStyle": "single"}}),
            UserPreferences {
                quote_preference: QuotePreference::SINGLE,
                ..UserPreferences::default()
            },
        ),
        (
            "unstable sets value when no stable config",
            json!({"unstable": {"includeAutomaticOptionalChainCompletions": false}}),
            UserPreferences {
                completion_preferences: CompletionPreferences {
                    include_automatic_optional_chain_completions: Some(false),
                    ..CompletionPreferences::default()
                },
                ..UserPreferences::default()
            },
        ),
        (
            "any field can be passed via unstable by its raw name",
            json!({"unstable": {"quotePreference": "double", "includeCompletionsForModuleExports": true, "excludeLibrarySymbolsInNavTo": true}}),
            UserPreferences {
                quote_preference: QuotePreference::DOUBLE,
                completion_preferences: CompletionPreferences {
                    include_completions_for_module_exports: Some(true),
                    ..CompletionPreferences::default()
                },
                exclude_library_symbols_in_nav_to: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "TypeScript raw names work in unstable section",
            json!({"unstable": {
                "includeCompletionsForModuleExports": true, "quotePreference": "single",
                "providePrefixAndSuffixTextForRename": true, "includeInlayParameterNameHints": "all",
                "organizeImportsLocale": "en",
            }}),
            UserPreferences {
                completion_preferences: CompletionPreferences {
                    include_completions_for_module_exports: Some(true),
                    ..CompletionPreferences::default()
                },
                quote_preference: QuotePreference::SINGLE,
                provide_prefix_and_suffix_text_for_rename: Some(true),
                organize_imports_locale: "en".to_owned(),
                inlay_hints_preferences: InlayHintsPreferences {
                    include_inlay_parameter_name_hints: IncludeInlayParameterNameHints::ALL,
                    ..InlayHintsPreferences::default()
                },
                ..UserPreferences::default()
            },
        ),
        (
            "old raw organize imports unicode preferences load as raw state",
            json!({"unstable": {
                "organizeImportsCollation": "unicode", "organizeImportsCaseFirst": "upper",
                "organizeImportsIgnoreCase": false, "organizeImportsNumericCollation": true,
            }}),
            UserPreferences {
                organize_imports_collation: OrganizeImportsCollation::UNICODE,
                organize_imports_case_first: OrganizeImportsCaseFirst::UPPER,
                organize_imports_ignore_case: Some(false),
                organize_imports_numeric_collation: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "old top-level raw organize imports unicode preferences load as raw state",
            json!({"organizeImportsCollation": "unicode", "organizeImportsIgnoreCase": true}),
            UserPreferences {
                organize_imports_collation: OrganizeImportsCollation::UNICODE,
                organize_imports_ignore_case: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "new top-level raw organize imports sort is accepted",
            json!({"organizeImportsSort": "natural"}),
            UserPreferences {
                organize_imports_sort: OrganizeImportsSort::NATURAL,
                ..UserPreferences::default()
            },
        ),
        (
            "old raw organize imports ignore case loads as raw state",
            json!({"unstable": {"organizeImportsIgnoreCase": true}}),
            UserPreferences {
                organize_imports_ignore_case: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "new raw organize imports sort loads alongside old raw preferences",
            json!({"unstable": {"organizeImportsSort": "ordinal", "organizeImportsCollation": "unicode", "organizeImportsIgnoreCase": true}}),
            UserPreferences {
                organize_imports_sort: OrganizeImportsSort::ORDINAL,
                organize_imports_collation: OrganizeImportsCollation::UNICODE,
                organize_imports_ignore_case: Some(true),
                ..UserPreferences::default()
            },
        ),
        (
            "old nested organize imports unicode preferences load as raw state",
            json!({"preferences": {"organizeImports": {
                "unicodeCollation": "unicode", "caseSensitivity": "caseSensitive",
                "numericCollation": true, "caseFirst": "upper",
            }}}),
            UserPreferences {
                organize_imports_collation: OrganizeImportsCollation::UNICODE,
                organize_imports_ignore_case: Some(false),
                organize_imports_numeric_collation: Some(true),
                organize_imports_case_first: OrganizeImportsCaseFirst::UPPER,
                ..UserPreferences::default()
            },
        ),
        (
            "new nested organize imports sort loads alongside old nested preferences",
            json!({"preferences": {"organizeImports": {
                "sort": "ordinalIgnoreCase", "unicodeCollation": "unicode", "caseSensitivity": "caseSensitive",
            }}}),
            UserPreferences {
                organize_imports_sort: OrganizeImportsSort::ORDINAL_IGNORE_CASE,
                organize_imports_collation: OrganizeImportsCollation::UNICODE,
                organize_imports_ignore_case: Some(false),
                ..UserPreferences::default()
            },
        ),
    ];
    for (name, config, expected) in cases {
        assert_eq!(
            UserPreferences::default().with_config(&object(config)),
            expected,
            "{name}"
        );
    }
}

/// tsgo `TestUserPreferencesLocale`.
#[test]
fn locale() {
    let preferences = parse_user_preferences(&object(json!({
        "typescript": {"locale": "de"},
        "js/ts": {"locale": "fr"},
    })));
    assert_eq!(preferences.locale, "fr");
}

/// tsgo `TestUserPreferencesReportStyleChecksAsWarnings`.
#[test]
fn report_style_checks_as_warnings() {
    let preferences = parse_user_preferences(&object(
        json!({"js/ts": {"reportStyleChecksAsWarnings": false}}),
    ));
    assert_eq!(preferences.report_style_checks_as_warnings, Some(false));
    assert_eq!(
        UserPreferences::new_default().report_style_checks_as_warnings,
        Some(true)
    );
    let preferences = parse_user_preferences(&object(
        json!({"js/ts": {"unstable": {"reportStyleChecksAsWarnings": false}}}),
    ));
    assert_eq!(preferences.report_style_checks_as_warnings, Some(false));
}

/// tsgo `TestUserPreferencesParseServerFeaturePreferences`.
#[test]
fn server_feature_preferences() {
    let preferred = parse_user_preferences(&object(json!({"js/ts": {
        "validate": {"enabled": false}, "format": {"enabled": false},
        "autoClosingTags": {"enabled": false},
    }})));
    assert_eq!(preferred.validate_enabled, Some(false));
    assert_eq!(preferred.format_enabled, Some(false));
    assert_eq!(preferred.auto_closing_tags, Some(false));

    let legacy = parse_user_preferences(&object(json!({"typescript": {
        "validate": {"enable": false}, "format": {"enable": false}, "autoClosingTags": false,
    }})));
    assert_eq!(legacy.validate_enabled, Some(false));
    assert_eq!(legacy.format_enabled, Some(false));
    assert_eq!(legacy.auto_closing_tags, Some(false));

    let both = parse_user_preferences(&object(json!({
        "typescript": {"validate": {"enable": false}, "format": {"enable": false}, "autoClosingTags": false},
        "js/ts": {"validate": {"enabled": true}, "format": {"enabled": true}, "autoClosingTags": {"enabled": true}},
    })));
    assert_eq!(both.validate_enabled, Some(true));
    assert_eq!(both.format_enabled, Some(true));
    assert_eq!(both.auto_closing_tags, Some(true));
}

/// tsgo `TestParseUserPreferencesEditorFormatting`.
#[test]
fn editor_formatting() {
    let preferences = parse_user_preferences(&object(
        json!({"editor": {"tabSize": 2, "insertSpaces": false}}),
    ));
    let editor = &preferences.format_code_settings.editor_settings;
    assert_eq!(editor.tab_size, 2);
    assert_eq!(editor.indent_size, 2);
    assert_eq!(editor.convert_tabs_to_spaces, Some(false));
}

/// tsgo `TestUserPreferencesParseJSDocCompletionPreferences`.
#[test]
fn jsdoc_completion_preferences() {
    let parse = |value: Value| parse_user_preferences(&object(value));
    assert_eq!(
        parse(json!({"js/ts": {"suggest": {"jsdoc": {"enabled": false}}}})).complete_js_docs,
        Some(false)
    );
    assert_eq!(
        parse(json!({"typescript": {"suggest": {"completeJSDocs": false}}})).complete_js_docs,
        Some(false)
    );
    assert_eq!(
        parse(json!({
            "typescript": {"suggest": {"completeJSDocs": false}},
            "js/ts": {"suggest": {"jsdoc": {"enabled": true}}},
        }))
        .complete_js_docs,
        Some(true)
    );
    assert_eq!(
        parse(json!({"js/ts": {"suggest": {"jsdoc": {"generateReturns": false}}}}))
            .generate_return_in_doc_template,
        Some(false)
    );
    assert_eq!(
        parse(json!({"typescript": {"suggest": {"jsdoc": {"generateReturns": false}}}}))
            .generate_return_in_doc_template,
        Some(false)
    );
}

/// tsgo `TestUserPreferencesParseATA`.
#[test]
fn automatic_type_acquisition() {
    let parse = |value: Value| parse_user_preferences(&object(value));
    let unified =
        parse(json!({"js/ts": {"tsserver": {"automaticTypeAcquisition": {"enabled": false}}}}));
    assert!(unified.is_ata_disabled());
    assert_eq!(unified.automatic_type_acquisition_enabled, Some(false));

    let deprecated = parse(json!({"typescript": {"disableAutomaticTypeAcquisition": true}}));
    assert!(deprecated.is_ata_disabled());
    assert_eq!(deprecated.disable_automatic_type_acquisition, Some(true));

    let both = parse(json!({
        "typescript": {"disableAutomaticTypeAcquisition": true},
        "js/ts": {"tsserver": {"automaticTypeAcquisition": {"enabled": true}}},
    }));
    assert!(!both.is_ata_disabled());
    assert_eq!(both.automatic_type_acquisition_enabled, Some(true));

    assert!(!UserPreferences::new_default().is_ata_disabled());
}

/// The module specifier preferences go to the checker's module specifiers.
#[test]
fn module_specifier_preferences() {
    let preferences = UserPreferences::default().with_config(&object(json!({
        "preferences": {"importModuleSpecifier": "non-relative", "importModuleSpecifierEnding": "minimal"},
    })));
    let module = preferences.module_specifier_preferences();
    assert_eq!(
        module.import_module_specifier_preference,
        ImportModuleSpecifierPreference::NON_RELATIVE
    );
    assert_eq!(
        module.import_module_specifier_ending,
        ImportModuleSpecifierEndingPreference::MINIMAL
    );
}
