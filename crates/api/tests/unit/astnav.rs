//! tsgo `astnav/tokens_test.go`: the Go baselines of token navigation over
//! `mapCode.ts` (`baselineGoTokensJSON`), the kind and range of what each
//! search finds at every position, in runs of positions with the same
//! result.

use std::path::Path;

use serde::Serialize;

use super::*;
use crate::encoder::{kind_name, tsgo_kind, ScriptKind};

const PROFILE: &str = "7.1.0-dev-aa814927";

fn upstream(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/typescript-native")
        .join(PROFILE)
        .join("upstream/tsc/testdata")
        .join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// tsgo `tokenRun`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenRun {
    start_pos: usize,
    end_pos: usize,
    kind: String,
    node_pos: u32,
    node_end: u32,
}

/// tsgo `toTokenInfo`: the kind's name without `Kind`, and the range.
fn token_info(navigator: &Navigator<'_>, found: Found) -> (String, u32, u32) {
    let kind = tsgo_kind(navigator.kind(found)).expect("a TypeScript 7.1 kind");
    let mut name = kind_name(kind).replacen("Kind", "", 1);
    if name == "EndOfFile" {
        name = "EndOfFileToken".to_owned();
    }
    (name, navigator.pos(found), navigator.end(found))
}

/// tsgo `baselineGoTokensJSON`.
fn baseline(test_name: &str, find: impl Fn(&Navigator<'_>, u32) -> Option<Found>) {
    let text = upstream("fixtures/services/mapCode.ts");
    let file = crate::parse_source_file("/file.ts", &text, ScriptKind::Ts);
    let navigator = Navigator::new(&file);
    let mut runs = Vec::new();
    let mut current: Option<TokenRun> = None;
    for pos in 0..text.len() {
        let token = find(&navigator, pos as u32).map(|found| token_info(&navigator, found));
        match (&mut current, token) {
            (Some(run), Some((kind, node_pos, node_end)))
                if run.kind == kind && run.node_pos == node_pos && run.node_end == node_end =>
            {
                run.end_pos = pos;
            }
            (_, token) => {
                runs.extend(current.take());
                current = token.map(|(kind, node_pos, node_end)| TokenRun {
                    start_pos: pos,
                    end_pos: pos,
                    kind,
                    node_pos,
                    node_end,
                });
            }
        }
    }
    runs.extend(current);
    let output = serde_json::to_string_pretty(&runs).unwrap();
    let expected = upstream(&format!(
        "baselines/reference/astnav/{test_name}.mapCode.ts.baseline.json"
    ));
    if output != expected {
        let line = output
            .lines()
            .zip(expected.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let context = |text: &str| {
            text.lines()
                .skip(line.saturating_sub(8))
                .take(16)
                .collect::<Vec<_>>()
                .join("\n")
        };
        panic!(
            "{test_name}: differs at line {line}\n--- tsc-rs\n{}\n--- tsgo\n{}",
            context(&output),
            context(&expected)
        );
    }
}

#[test]
fn get_touching_property_name_matches_tsgo() {
    baseline("GetTouchingPropertyName", |navigator, pos| {
        Some(navigator.touching_property_name(pos))
    });
}

#[test]
fn get_token_at_position_matches_tsgo() {
    baseline("GetTokenAtPosition", |navigator, pos| {
        Some(navigator.token_at_position(pos, true, None))
    });
}

#[test]
fn find_preceding_token_matches_tsgo() {
    baseline("FindPrecedingToken", |navigator, pos| {
        navigator.find_preceding_token_ex(pos, None, false)
    });
}
