use std::path::Path;

use serde_json::{Map, Number, Value};
use tsc_syntax::{scan_tokens, LanguageVariant, NodeId, SourceFile, SyntaxKind};

/// Keep malformed or adversarial manifests from reaching the recursive JSON
/// parser with unbounded structural nesting. Package consumers expose empty
/// semantics beyond this explicit boundary, just as they do for invalid text.
const MAX_PACKAGE_JSON_DEPTH: usize = 256;

/// Parse the object view returned by TypeScript's `readJson` helper while
/// retaining the exact decoded text owned by the caller.
///
/// Strict JSON is attempted first. On failure, TypeScript parses the text as
/// JSONC and accepts it only when conversion produces no diagnostics. Empty,
/// invalid, and non-object inputs consequently expose an empty object to
/// package consumers instead of becoming infrastructure failures.
///
/// tsc-port: readJsonOrUndefined/readJson @6.0.3
/// tsc-hash: 0be1077ca0dcab5ef44710716a6fb660d94811c5b51312f6c2fb20fc3029786e
/// tsc-span: _tsc.js:17261-17275
/// JSONC conversion also follows `_tsc.js:38331-38344,38475-38553`.
pub(crate) fn parse_json_object(file_name: &Path, text: String) -> (String, Map<String, Value>) {
    if package_json_tokens_are_unsafe(&text) {
        return (text, Map::new());
    }

    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        return (
            text,
            match value {
                Value::Object(object) => object,
                _ => Map::new(),
            },
        );
    }

    let source = tsc_syntax::parse_json_text(file_name.to_string_lossy(), text);
    let object = parse_jsonc_object(&source).unwrap_or_default();
    (source.text, object)
}

enum ConversionTask {
    Visit(NodeId),
    FinishArray(usize),
    FinishObject(Vec<String>),
}

fn parse_jsonc_object(source: &SourceFile) -> Option<Map<String, Value>> {
    if !source.parse_diagnostics.is_empty() {
        return None;
    }
    let source_file = source.arena.node(source.root).data.as_source_file()?;
    let statements = source
        .arena
        .node_array(source_file.statements?)
        .nodes
        .as_slice();
    let [statement] = statements else {
        return None;
    };
    let expression = source
        .arena
        .node(*statement)
        .data
        .as_expression_statement()?
        .expression?;
    let Value::Object(object) = convert_jsonc_value(source, expression)? else {
        return None;
    };
    Some(object)
}

fn convert_jsonc_value(source: &SourceFile, root: NodeId) -> Option<Value> {
    let mut tasks = vec![ConversionTask::Visit(root)];
    let mut values = Vec::new();

    while let Some(task) = tasks.pop() {
        match task {
            ConversionTask::Visit(value) => {
                let node = source.arena.node(value);
                match node.kind {
                    SyntaxKind::StringLiteral => {
                        if !is_double_quoted_json_string(source, value) {
                            return None;
                        }
                        values.push(Value::String(
                            node.data.as_string_literal()?.text.to_owned(),
                        ));
                    }
                    SyntaxKind::NumericLiteral => {
                        values.push(Value::Number(json_number(
                            &node.data.as_numeric_literal()?.text,
                        )?));
                    }
                    SyntaxKind::TrueKeyword => values.push(Value::Bool(true)),
                    SyntaxKind::FalseKeyword => values.push(Value::Bool(false)),
                    SyntaxKind::NullKeyword => values.push(Value::Null),
                    SyntaxKind::PrefixUnaryExpression => {
                        let unary = node.data.as_prefix_unary_expression()?;
                        if unary.operator != SyntaxKind::MinusToken {
                            return None;
                        }
                        let operand = source.arena.node(unary.operand?);
                        if operand.kind != SyntaxKind::NumericLiteral {
                            return None;
                        }
                        let number = json_number(&operand.data.as_numeric_literal()?.text)?;
                        values.push(Value::Number(negate_json_number(&number)?));
                    }
                    SyntaxKind::ArrayLiteralExpression => {
                        let elements = source
                            .arena
                            .node_array(node.data.as_array_literal_expression()?.elements?)
                            .nodes
                            .clone();
                        tasks.push(ConversionTask::FinishArray(elements.len()));
                        tasks.extend(elements.into_iter().rev().map(ConversionTask::Visit));
                    }
                    SyntaxKind::ObjectLiteralExpression => {
                        let properties = source
                            .arena
                            .node_array(node.data.as_object_literal_expression()?.properties?)
                            .nodes
                            .clone();
                        let mut keys = Vec::with_capacity(properties.len());
                        let mut initializers = Vec::with_capacity(properties.len());
                        for property in properties {
                            let property =
                                source.arena.node(property).data.as_property_assignment()?;
                            // convertToJson diagnoses `?`, but deliberately
                            // ignores TypeScript-only `!` and modifiers on a
                            // property assignment in the JSONC fallback.
                            if property.question_token.is_some() {
                                return None;
                            }
                            let name = property.name?;
                            if !is_double_quoted_json_string(source, name) {
                                return None;
                            }
                            keys.push(
                                source
                                    .arena
                                    .node(name)
                                    .data
                                    .as_string_literal()?
                                    .text
                                    .to_owned(),
                            );
                            initializers.push(property.initializer?);
                        }
                        tasks.push(ConversionTask::FinishObject(keys));
                        tasks.extend(initializers.into_iter().rev().map(ConversionTask::Visit));
                    }
                    _ => return None,
                }
            }
            ConversionTask::FinishArray(length) => {
                let start = values.len().checked_sub(length)?;
                let elements = values.split_off(start);
                values.push(Value::Array(elements));
            }
            ConversionTask::FinishObject(keys) => {
                let start = values.len().checked_sub(keys.len())?;
                let object_values = values.split_off(start);
                let mut object = Map::new();
                for (key, value) in keys.into_iter().zip(object_values) {
                    object.insert(key, value);
                }
                values.push(Value::Object(object));
            }
        }
    }

    let [value] = values.as_mut_slice() else {
        return None;
    };
    Some(std::mem::take(value))
}

fn json_number(text: &str) -> Option<Number> {
    let text = text.replace('_', "");
    let value = if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        radix_number(digits, 16)?
    } else if let Some(digits) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        radix_number(digits, 2)?
    } else if let Some(digits) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
        radix_number(digits, 8)?
    } else {
        text.parse::<f64>().ok()?
    };
    finite_json_number(value)
}

fn radix_number(digits: &str, radix: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    let mut value = 0.0;
    for digit in digits.chars() {
        let digit = digit.to_digit(radix)?;
        value = value * f64::from(radix) + f64::from(digit);
    }
    Some(value)
}

fn negate_json_number(number: &Number) -> Option<Number> {
    finite_json_number(-number.as_f64()?)
}

fn package_json_tokens_are_unsafe(text: &str) -> bool {
    let tokens = scan_tokens(text, LanguageVariant::Standard);
    let mut delimiters = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            SyntaxKind::OpenBraceToken | SyntaxKind::OpenBracketToken => {
                delimiters.push(token.kind);
                if delimiters.len() > MAX_PACKAGE_JSON_DEPTH {
                    return true;
                }
            }
            SyntaxKind::CloseBraceToken => {
                if delimiters.pop() != Some(SyntaxKind::OpenBraceToken) {
                    return true;
                }
            }
            SyntaxKind::CloseBracketToken => {
                if delimiters.pop() != Some(SyntaxKind::OpenBracketToken) {
                    return true;
                }
            }
            SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::CommaToken
            | SyntaxKind::ColonToken => {}
            SyntaxKind::MinusToken
                if tokens.get(index + 1).map(|next| next.kind)
                    == Some(SyntaxKind::NumericLiteral) => {}
            SyntaxKind::ExclamationToken
                if index > 0
                    && tokens[index - 1].kind == SyntaxKind::StringLiteral
                    && tokens.get(index + 1).map(|next| next.kind)
                        == Some(SyntaxKind::ColonToken) => {}
            kind if is_jsonc_property_modifier(kind) => {}
            // Anything else cannot survive convertToJson. Reject it before
            // the general expression parser can recurse through parentheses,
            // prefix operators, conditionals, functions, or templates.
            _ => return true,
        }
    }
    !delimiters.is_empty()
}

fn is_jsonc_property_modifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AbstractKeyword
            | SyntaxKind::AccessorKeyword
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::DefaultKeyword
            | SyntaxKind::ExportKeyword
            | SyntaxKind::InKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::OutKeyword
            | SyntaxKind::OverrideKeyword
    )
}

fn finite_json_number(value: f64) -> Option<Number> {
    // JavaScript Number permits infinities while serde_json::Number does not.
    // Package consumers observe only the number/zero distinction, so a
    // same-sign finite sentinel preserves every resolver branch.
    let value = if value == f64::INFINITY {
        f64::MAX
    } else if value == f64::NEG_INFINITY {
        f64::MIN
    } else {
        value
    };
    Number::from_f64(value)
}

fn is_double_quoted_json_string(source: &SourceFile, value: NodeId) -> bool {
    let node = source.arena.node(value);
    if node.kind != SyntaxKind::StringLiteral {
        return false;
    }
    source
        .text
        .as_bytes()
        .get(tsc_syntax::skip_trivia(&source.text, node.pos as usize))
        == Some(&b'"')
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{parse_json_object, MAX_PACKAGE_JSON_DEPTH};

    #[test]
    fn strict_and_jsonc_objects_share_the_same_owned_projection() {
        let path = Path::new("/types/pkg/package.json");
        let strict = r#"{"name":"pkg","exports":{".":"./index.d.ts"}}"#.to_owned();
        let (retained, object) = parse_json_object(path, strict.clone());
        assert_eq!(retained, strict);
        assert_eq!(object["name"], json!("pkg"));
        assert_eq!(object["exports"], json!({".": "./index.d.ts"}));

        let jsonc = r#"{/* comment */"typings":null,"nested":[1,-2,{}],}"#.to_owned();
        let (retained, object) = parse_json_object(path, jsonc.clone());
        assert_eq!(retained, jsonc);
        assert_eq!(object["typings"], Value::Null);
        assert_eq!(object["nested"], json!([1.0, -2.0, {}]));
    }

    #[test]
    fn duplicate_keys_are_last_wins_and_jsonc_numbers_are_converted() {
        let (_, object) = parse_json_object(
            Path::new("package.json"),
            r#"{/* fallback */"typings":"first","typings":null,"hex":0x10,"negativeZero":-0}"#
                .to_owned(),
        );
        assert_eq!(object["typings"], Value::Null);
        assert_eq!(object["hex"], json!(16.0));
        let (_, rounded) = parse_json_object(
            Path::new("package.json"),
            r#"{/* fallback */"value":9007199254740993}"#.to_owned(),
        );
        assert_eq!(rounded["value"].as_f64(), Some(9_007_199_254_740_992.0));
        assert!(object["negativeZero"]
            .as_f64()
            .expect("negative zero remains numeric")
            .is_sign_negative());
    }

    #[test]
    fn jsonc_fallback_matches_modifier_and_structural_depth_boundaries() {
        let (_, object) = parse_json_object(
            Path::new("package.json"),
            r#"{readonly "name":"pkg","typings"!:null}"#.to_owned(),
        );
        assert_eq!(object["name"], json!("pkg"));
        assert_eq!(object["typings"], Value::Null);

        let at_limit = format!(
            "{{\"value\":{}0{}}}",
            "[".repeat(MAX_PACKAGE_JSON_DEPTH - 1),
            "]".repeat(MAX_PACKAGE_JSON_DEPTH - 1)
        );
        let (_, object) = parse_json_object(Path::new("package.json"), at_limit);
        assert!(object.contains_key("value"));

        let too_deep = format!(
            "{{\"value\":{}0{}}}",
            "[".repeat(MAX_PACKAGE_JSON_DEPTH),
            "]".repeat(MAX_PACKAGE_JSON_DEPTH)
        );
        let (_, object) = parse_json_object(Path::new("package.json"), too_deep);
        assert!(object.is_empty());

        for invalid in [
            format!(
                "{{\"value\":{}0{}}}",
                "(".repeat(MAX_PACKAGE_JSON_DEPTH + 1),
                ")".repeat(MAX_PACKAGE_JSON_DEPTH + 1)
            ),
            format!("{{\"value\":{}0}}", "!".repeat(MAX_PACKAGE_JSON_DEPTH + 1)),
            format!(
                "[{}0{}",
                "}[".repeat(MAX_PACKAGE_JSON_DEPTH + 1),
                "]".repeat(MAX_PACKAGE_JSON_DEPTH + 2)
            ),
        ] {
            let (_, object) = parse_json_object(Path::new("package.json"), invalid);
            assert!(object.is_empty());
        }
    }

    #[test]
    fn invalid_empty_and_non_object_inputs_expose_an_empty_object() {
        for input in [
            "",
            "null",
            "true",
            "[null]",
            "{typings: null}",
            "{'typings': null}",
            r#"{"typings": null, "x": 'value'}"#,
            r#"{"typings": null, "x": {bad: 1}}"#,
            r#"{"typings": null, "x": undefined}"#,
            r#"{"typings": null, "x": [1,,2]}"#,
            r#"{"typings"?: null}"#,
            r#"{"typings": null"#,
        ] {
            let (_, object) = parse_json_object(Path::new("package.json"), input.to_owned());
            assert!(
                object.is_empty(),
                "input must behave like readJson: {input}"
            );
        }
    }
}
