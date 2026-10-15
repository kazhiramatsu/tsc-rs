//! tsgo `lsp/server_contentmapper_internal_test.go`: an editor's content
//! mapper contributions as the session's mappers.

use std::collections::BTreeMap;

use serde_json::Value;
use tsc_lsp::content_mappers::parse_content_mapper_contributions;
use tsc_lsproto::{
    ContentMapperContribution, ContentMapperManifest, InferredProjectContentMapperContribution,
};

fn inferred_project_contribution(name: &str) -> InferredProjectContentMapperContribution {
    InferredProjectContentMapperContribution {
        options: None,
        manifest: ContentMapperManifest {
            name: name.to_owned(),
            exec: vec![name.to_owned()],
            ..ContentMapperManifest::default()
        },
    }
}

fn contribution(
    contributor_id: &str,
    extensions: &[&str],
    inferred_project: Option<InferredProjectContentMapperContribution>,
) -> ContentMapperContribution {
    ContentMapperContribution {
        contributor_id: contributor_id.to_owned(),
        extensions: extensions
            .iter()
            .map(|&extension| extension.to_owned())
            .collect(),
        inferred_project_contribution: inferred_project,
    }
}

/// tsgo `TestParseContentMapperContributions`.
#[test]
fn contributions_become_inferred_project_mappers() {
    let contributions = parse_content_mapper_contributions(&[
        contribution(
            "publisher.extension",
            &[".vue"],
            Some(InferredProjectContentMapperContribution {
                options: Some(BTreeMap::from([(
                    "mode".to_owned(),
                    Value::String("embedded".to_owned()),
                )])),
                manifest: ContentMapperManifest {
                    name: "Vue mapper".to_owned(),
                    version: Some("2.3.4".to_owned()),
                    exec: vec!["node".to_owned(), "mapper.js".to_owned()],
                    cwd: Some("/workspace/mapper".to_owned()),
                    compiler_options: Some(vec!["strict".to_owned()]),
                    dynamic_config: None,
                },
            }),
        ),
        contribution("publisher.extension", &[".svelte"], None),
    ])
    .unwrap();
    assert_eq!(contributions.mappers.len(), 1);
    assert_eq!(contributions.extensions, [".vue"]);
    let mapper = &contributions.mappers[0];
    assert_eq!(
        mapper.identity(),
        "publisher.extension[0] (Vue mapper@2.3.4)"
    );
    assert_eq!(mapper.package_directory, "/workspace/mapper");
    assert_eq!(
        mapper.definition.options.as_deref(),
        Some(r#"{"mode":"embedded"}"#)
    );
}

/// tsgo `TestParseContentMapperContributionsRejectsConflictingInlineMappers`.
#[test]
fn two_contributions_cannot_claim_one_extension() {
    let error = parse_content_mapper_contributions(&[
        contribution(
            "first",
            &[".vue"],
            Some(inferred_project_contribution("first")),
        ),
        contribution(
            "second",
            &[".vue"],
            Some(inferred_project_contribution("second")),
        ),
    ])
    .unwrap_err();
    assert!(error.contains(r#"both claim extension ".vue""#), "{error}");
}

/// tsgo `TestParseContentMapperContributionsUsesCaseInsensitiveExtensions`.
#[test]
fn extensions_are_compared_case_insensitively() {
    let error = parse_content_mapper_contributions(&[
        contribution(
            "first",
            &[".vue"],
            Some(inferred_project_contribution("first")),
        ),
        contribution(
            "second",
            &[".VUE"],
            Some(inferred_project_contribution("second")),
        ),
    ])
    .unwrap_err();
    assert!(error.contains(r#"both claim extension ".VUE""#), "{error}");
    let error = parse_content_mapper_contributions(&[contribution("built-in", &[".TS"], None)])
        .unwrap_err();
    assert!(error.contains(r#"invalid extension ".TS""#), "{error}");
}

/// tsgo `TestParseContentMapperContributionsDefaultsOptionsToObject`.
#[test]
fn a_contribution_without_options_has_an_empty_object() {
    let contributions = parse_content_mapper_contributions(&[contribution(
        "publisher.extension",
        &[".vue"],
        Some(inferred_project_contribution("mapper")),
    )])
    .unwrap();
    assert_eq!(
        contributions.mappers[0].definition.options.as_deref(),
        Some("{}")
    );
}
