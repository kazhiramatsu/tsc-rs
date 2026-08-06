use std::path::Path;

use serde_json::{Map, Number, Value};
use tsc_syntax::{scan_token_kinds, LanguageVariant, NodeId, SourceFile, SyntaxKind};

/// Keep malformed or adversarial manifests from reaching the recursive JSON
/// parser with unbounded structural nesting. Package consumers expose empty
/// semantics beyond this explicit boundary, just as they do for invalid text.
const MAX_PACKAGE_JSON_DEPTH: usize = 256;
pub(crate) const JSONC_PROTOTYPE_MARKER: &str = "\0tsc-rs:jsonc-prototype\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsonParserPreflight {
    Safe,
    UnsafeSyntax,
    ResourceLimit,
}

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
    if json_parser_preflight(&text) != JsonParserPreflight::Safe {
        return (text, Map::new());
    }

    if let Ok(mut value) = serde_json::from_str::<Value>(&text) {
        encode_user_object_keys(&mut value);
        return (
            text,
            match value {
                Value::Object(object) => object,
                _ => Map::new(),
            },
        );
    }

    let strict_json = text_is_strict_json(&text);
    let source = tsc_syntax::parse_json_text(file_name.to_string_lossy(), text);
    let object = parse_jsonc_object(&source, strict_json).unwrap_or_default();
    (source.text, object)
}

fn encode_user_object_keys(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                encode_user_object_keys(value);
            }
        }
        Value::Object(object) => {
            let old = std::mem::take(object);
            for (key, mut value) in old {
                encode_user_object_keys(&mut value);
                object.insert(encode_user_object_key(key), value);
            }
        }
        Value::Number(number) => {
            let value = number
                .as_f64()
                .expect("strict JSON numbers are finite JavaScript numbers");
            *number = Number::from_f64(value)
                .expect("a finite JavaScript number remains a serde JSON number");
        }
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
}

fn encode_user_object_key(mut key: String) -> String {
    if key.starts_with('\0') {
        key.insert(0, '\0');
    }
    key
}

/// JSON.parse accepts finite-syntax numbers which overflow to JavaScript
/// infinity, while serde_json's default number parser rejects them. Detect
/// the strict grammar independently so that such a document still receives
/// JSON.parse's ordinary own-property semantics instead of JSONC's
/// `convertToJson` assignment semantics. The parser is intentionally only a
/// validator; the syntax arena below remains the single fallback converter.
fn text_is_strict_json(text: &str) -> bool {
    StrictJsonParser::new(text).parse()
}

struct StrictJsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> StrictJsonParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    fn parse(mut self) -> bool {
        self.skip_whitespace();
        self.parse_value(0) && {
            self.skip_whitespace();
            self.position == self.bytes.len()
        }
    }

    fn parse_value(&mut self, depth: usize) -> bool {
        if depth > MAX_PACKAGE_JSON_DEPTH {
            return false;
        }
        match self.bytes.get(self.position).copied() {
            Some(b'{') => self.parse_object(depth + 1),
            Some(b'[') => self.parse_array(depth + 1),
            Some(b'"') => self.parse_string(),
            Some(b't') => self.parse_literal(b"true"),
            Some(b'f') => self.parse_literal(b"false"),
            Some(b'n') => self.parse_literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => false,
        }
    }

    fn parse_object(&mut self, depth: usize) -> bool {
        self.position += 1;
        self.skip_whitespace();
        if self.consume(b'}') {
            return true;
        }
        loop {
            if !self.parse_string() {
                return false;
            }
            self.skip_whitespace();
            if !self.consume(b':') {
                return false;
            }
            self.skip_whitespace();
            if !self.parse_value(depth) {
                return false;
            }
            self.skip_whitespace();
            if self.consume(b'}') {
                return true;
            }
            if !self.consume(b',') {
                return false;
            }
            self.skip_whitespace();
            // A closing delimiter here would be a JSONC trailing comma.
            if self.bytes.get(self.position) == Some(&b'}') {
                return false;
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> bool {
        self.position += 1;
        self.skip_whitespace();
        if self.consume(b']') {
            return true;
        }
        loop {
            if !self.parse_value(depth) {
                return false;
            }
            self.skip_whitespace();
            if self.consume(b']') {
                return true;
            }
            if !self.consume(b',') {
                return false;
            }
            self.skip_whitespace();
            if self.bytes.get(self.position) == Some(&b']') {
                return false;
            }
        }
    }

    fn parse_string(&mut self) -> bool {
        if !self.consume(b'"') {
            return false;
        }
        while let Some(byte) = self.bytes.get(self.position).copied() {
            self.position += 1;
            match byte {
                b'"' => return true,
                0x00..=0x1f => return false,
                b'\\' => {
                    let Some(escape) = self.bytes.get(self.position).copied() else {
                        return false;
                    };
                    self.position += 1;
                    match escape {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            let Some(end) = self.position.checked_add(4) else {
                                return false;
                            };
                            let Some(digits) = self.bytes.get(self.position..end) else {
                                return false;
                            };
                            if !digits.iter().all(u8::is_ascii_hexdigit) {
                                return false;
                            }
                            self.position = end;
                        }
                        _ => return false,
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn parse_number(&mut self) -> bool {
        self.consume(b'-');
        match self.bytes.get(self.position).copied() {
            Some(b'0') => {
                self.position += 1;
                if self
                    .bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    return false;
                }
            }
            Some(b'1'..=b'9') => {
                self.position += 1;
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            _ => return false,
        }
        if self.consume(b'.') {
            let start = self.position;
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if self.position == start {
                return false;
            }
        }
        if matches!(self.bytes.get(self.position), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.bytes.get(self.position), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let start = self.position;
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if self.position == start {
                return false;
            }
        }
        true
    }

    fn parse_literal(&mut self, literal: &[u8]) -> bool {
        let Some(end) = self.position.checked_add(literal.len()) else {
            return false;
        };
        if self.bytes.get(self.position..end) != Some(literal) {
            return false;
        }
        self.position = end;
        true
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\r' | b'\n')
        ) {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.position) == Some(&expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

enum ConversionTask {
    Visit {
        value: NodeId,
        structural_depth: usize,
    },
    FinishArray(usize),
    FinishObject(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RecoverableJsonValue {
    Defined(Value),
    Undefined,
}

fn parse_jsonc_object(
    source: &SourceFile,
    use_strict_object_assignment: bool,
) -> Option<Map<String, Value>> {
    let value = if use_strict_object_assignment {
        convert_json_source_file_to_value_with_assignment(
            source, true, /* allow_parse_recovery */ false,
        )
    } else {
        convert_json_source_file_to_value(source)
    }?;
    let Value::Object(object) = value else {
        return None;
    };
    Some(object)
}

/// Convert one parsed JSON source value using TypeScript's `convertToJson`
/// object-assignment semantics.
///
/// This entry deliberately never selects the package reader's strict
/// `JSON.parse` path: in particular, assigning `__proto__` may update the
/// converted object's prototype. Parse diagnostics, an empty source, or a
/// recovered source containing anything other than one expression fail closed.
/// Conversion remains iterative and independently enforces the same structural
/// depth ceiling as the package-JSON boundary.
///
/// tsc-port: convertToJson @6.0.3 (value and object-assignment semantics)
/// tsc-hash: 372b1d27b4881e537f81282d2515fa1868cabd14eda1b75f0533b1b386dec971
/// tsc-span: _tsc.js:38521-38600
pub(crate) fn convert_json_source_file_to_value(source: &SourceFile) -> Option<Value> {
    convert_json_source_file_to_value_with_assignment(
        source, /* use_strict_object_assignment */ false,
        /* allow_parse_recovery */ false,
    )
}

/// Convert the recoverable JSON syntax tree even when the parser also
/// reported diagnostics.
///
/// TypeScript's config-file path keeps the root source's parse diagnostics
/// separate from `ParsedCommandLine.errors`, then still runs
/// `convertConfigFileToObject` over the recovered tree. Package metadata does
/// not use that recovery contract, so the ordinary converter above remains
/// fail-closed.
pub(crate) fn convert_recoverable_json_source_file_to_value(source: &SourceFile) -> Option<Value> {
    convert_json_source_file_to_value_with_assignment(
        source, /* use_strict_object_assignment */ false, /* allow_parse_recovery */ true,
    )
}

/// Convert one recovered config-syntax node with the same ordinary object
/// assignment semantics as [`convert_recoverable_json_source_file_to_value`].
///
/// Config parsing observes every property assignment before duplicate keys
/// are collapsed into the returned raw object. Keeping this node-level entry
/// point lets the config notifier validate those assignments in source order
/// without weakening the fail-closed package-JSON boundary above.
pub(crate) fn convert_recoverable_json_node_to_value(
    source: &SourceFile,
    node: NodeId,
) -> Option<RecoverableJsonValue> {
    convert_jsonc_value_worker(
        source, node, /* use_strict_object_assignment */ false,
        /* recover_undefined */ true,
    )
}

pub(crate) fn json_source_file_is_empty(source: &SourceFile) -> bool {
    source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .and_then(|source_file| source_file.statements)
        .map(|statements| source.arena.node_array(statements).nodes.is_empty())
        .unwrap_or(false)
}

fn convert_json_source_file_to_value_with_assignment(
    source: &SourceFile,
    use_strict_object_assignment: bool,
    allow_parse_recovery: bool,
) -> Option<Value> {
    if !allow_parse_recovery && !source.parse_diagnostics.is_empty() {
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
    if allow_parse_recovery {
        match convert_jsonc_value_worker(
            source,
            expression,
            use_strict_object_assignment,
            /* recover_undefined */ true,
        )? {
            RecoverableJsonValue::Defined(value) => Some(value),
            // A root-level invalid expression is replaced by the caller's
            // ordinary non-object recovery path.
            RecoverableJsonValue::Undefined => Some(Value::Null),
        }
    } else {
        convert_jsonc_value(source, expression, use_strict_object_assignment)
    }
}

fn convert_jsonc_value(
    source: &SourceFile,
    root: NodeId,
    use_strict_object_assignment: bool,
) -> Option<Value> {
    match convert_jsonc_value_worker(
        source,
        root,
        use_strict_object_assignment,
        /* recover_undefined */ false,
    )? {
        RecoverableJsonValue::Defined(value) => Some(value),
        RecoverableJsonValue::Undefined => None,
    }
}

fn convert_jsonc_value_worker(
    source: &SourceFile,
    root: NodeId,
    use_strict_object_assignment: bool,
    recover_undefined: bool,
) -> Option<RecoverableJsonValue> {
    let mut tasks = vec![ConversionTask::Visit {
        value: root,
        structural_depth: 0,
    }];
    let mut values = Vec::new();

    while let Some(task) = tasks.pop() {
        match task {
            ConversionTask::Visit {
                value,
                structural_depth,
            } => {
                let node = source.arena.node(value);
                match node.kind {
                    SyntaxKind::StringLiteral => {
                        if !recover_undefined && !is_double_quoted_json_string(source, value) {
                            return None;
                        }
                        values.push(RecoverableJsonValue::Defined(Value::String(
                            node.data.as_string_literal()?.text.to_owned(),
                        )));
                    }
                    SyntaxKind::NumericLiteral => {
                        values.push(RecoverableJsonValue::Defined(Value::Number(json_number(
                            &node.data.as_numeric_literal()?.text,
                        )?)));
                    }
                    SyntaxKind::TrueKeyword => {
                        values.push(RecoverableJsonValue::Defined(Value::Bool(true)));
                    }
                    SyntaxKind::FalseKeyword => {
                        values.push(RecoverableJsonValue::Defined(Value::Bool(false)));
                    }
                    SyntaxKind::NullKeyword => {
                        values.push(RecoverableJsonValue::Defined(Value::Null));
                    }
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
                        values.push(RecoverableJsonValue::Defined(Value::Number(
                            negate_json_number(&number)?,
                        )));
                    }
                    SyntaxKind::ArrayLiteralExpression => {
                        let child_depth = structural_depth.checked_add(1)?;
                        if child_depth > MAX_PACKAGE_JSON_DEPTH {
                            return None;
                        }
                        let elements = source
                            .arena
                            .node_array(node.data.as_array_literal_expression()?.elements?)
                            .nodes
                            .clone();
                        tasks.push(ConversionTask::FinishArray(elements.len()));
                        tasks.extend(elements.into_iter().rev().map(|value| {
                            ConversionTask::Visit {
                                value,
                                structural_depth: child_depth,
                            }
                        }));
                    }
                    SyntaxKind::ObjectLiteralExpression => {
                        let child_depth = structural_depth.checked_add(1)?;
                        if child_depth > MAX_PACKAGE_JSON_DEPTH {
                            return None;
                        }
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
                            keys.push(jsonc_property_name(source, name, recover_undefined)?);
                            initializers.push(property.initializer?);
                        }
                        tasks.push(ConversionTask::FinishObject(keys));
                        tasks.extend(initializers.into_iter().rev().map(|value| {
                            ConversionTask::Visit {
                                value,
                                structural_depth: child_depth,
                            }
                        }));
                    }
                    _ if recover_undefined => values.push(RecoverableJsonValue::Undefined),
                    _ => return None,
                }
            }
            ConversionTask::FinishArray(length) => {
                let start = values.len().checked_sub(length)?;
                let elements = values
                    .split_off(start)
                    .into_iter()
                    .filter_map(|value| match value {
                        RecoverableJsonValue::Defined(value) => Some(value),
                        RecoverableJsonValue::Undefined => None,
                    })
                    .collect();
                values.push(RecoverableJsonValue::Defined(Value::Array(elements)));
            }
            ConversionTask::FinishObject(keys) => {
                let start = values.len().checked_sub(keys.len())?;
                let object_values = values.split_off(start);
                let mut object = Map::new();
                for (key, value) in keys.into_iter().zip(object_values) {
                    let RecoverableJsonValue::Defined(value) = value else {
                        // Ordinary JavaScript assignment overwrites a prior
                        // duplicate with `undefined`. serde_json cannot retain
                        // that value, so remove the stale projection. A
                        // reachable legacy `__proto__` setter instead ignores
                        // primitive right-hand sides and leaves the prototype
                        // transition intact.
                        if key == "__proto__" {
                            if json_object_own_get(&object, "__proto__").is_some()
                                || !jsonc_object_inherits_proto_setter(&object)
                            {
                                object.remove("__proto__");
                            }
                        } else {
                            object.remove(&encode_user_object_key(key));
                        }
                        continue;
                    };
                    if use_strict_object_assignment {
                        object.insert(encode_user_object_key(key), value);
                    } else {
                        assign_jsonc_object_property(&mut object, key, value);
                    }
                }
                values.push(RecoverableJsonValue::Defined(Value::Object(object)));
            }
        }
    }

    let [value] = values.as_mut_slice() else {
        return None;
    };
    Some(std::mem::replace(value, RecoverableJsonValue::Undefined))
}

fn assign_jsonc_object_property(object: &mut Map<String, Value>, key: String, value: Value) {
    if key != "__proto__" {
        object.insert(encode_user_object_key(key), value);
        return;
    }

    // `convertToJson` starts with `{}` and performs ordinary bracket
    // assignments in source order. The inherited Object.prototype setter
    // changes [[Prototype]] only while it remains reachable. Once a null
    // prototype or inherited/own data property hides that setter, a later
    // `__proto__` assignment creates or updates an ordinary own property.
    if json_object_own_get(object, "__proto__").is_some()
        || !jsonc_object_inherits_proto_setter(object)
    {
        object.insert(key, value);
    } else if matches!(value, Value::Object(_) | Value::Array(_) | Value::Null) {
        object.insert(JSONC_PROTOTYPE_MARKER.to_owned(), value);
    }
    // The native setter deliberately ignores primitive right-hand sides.
}

fn jsonc_object_inherits_proto_setter(object: &Map<String, Value>) -> bool {
    match jsonc_prototype(object) {
        // A freshly converted object has Object.prototype and therefore its
        // legacy `__proto__` accessor in the chain.
        None => true,
        Some(Value::Null) => false,
        Some(Value::Array(_)) => true,
        Some(Value::Object(prototype)) => {
            if json_object_own_get(prototype, "__proto__").is_some() {
                false
            } else {
                jsonc_object_inherits_proto_setter(prototype)
            }
        }
        Some(_) => {
            unreachable!("the JSONC converter stores only object, array, or null prototypes")
        }
    }
}

fn json_number(text: &str) -> Option<Number> {
    let text = text.replace('_', "");
    if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        finite_json_number(radix_number(digits, 16)?)
    } else if let Some(digits) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        finite_json_number(radix_number(digits, 2)?)
    } else if let Some(digits) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
        finite_json_number(radix_number(digits, 8)?)
    } else {
        let value = text.parse::<f64>().ok()?;
        if value.is_finite() {
            Number::from_f64(value)
        } else {
            finite_json_number(value)
        }
    }
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
    if number.as_u64() == Some(u64::MAX) {
        Some(Number::from(i64::MIN))
    } else if number.as_i64() == Some(i64::MIN) {
        Some(Number::from(u64::MAX))
    } else {
        finite_json_number(-number.as_f64()?)
    }
}

/// Bound the recursive syntax parser to the JSON/JSONC token surface before
/// constructing its arena. The scanner is iterative; unsupported expression
/// syntax and excessive structural nesting therefore fail before they can
/// create an unbounded parser call chain.
pub(crate) fn json_parser_preflight(text: &str) -> JsonParserPreflight {
    let mut tokens = scan_token_kinds(text, LanguageVariant::Standard).peekable();
    let mut delimiters = Vec::new();
    let mut recursive_value_depth = 0;
    let mut previous = None;
    while let Some(kind) = tokens.next() {
        if matches!(
            kind,
            SyntaxKind::OpenBraceToken | SyntaxKind::OpenBracketToken
        ) {
            delimiters.push((kind, recursive_value_depth));
            if delimiters.len() > MAX_PACKAGE_JSON_DEPTH {
                return JsonParserPreflight::ResourceLimit;
            }
            previous = Some(kind);
            continue;
        }

        let next = tokens.peek().copied();
        let is_property_name = next == Some(SyntaxKind::ColonToken);
        if is_recursive_value_token(kind) && !is_property_name {
            recursive_value_depth += 1;
            if recursive_value_depth > MAX_PACKAGE_JSON_DEPTH {
                return JsonParserPreflight::ResourceLimit;
            }
        }

        match kind {
            SyntaxKind::CloseBraceToken => {
                let Some((open, parent_depth)) = delimiters.pop() else {
                    return JsonParserPreflight::UnsafeSyntax;
                };
                if open != SyntaxKind::OpenBraceToken {
                    return JsonParserPreflight::UnsafeSyntax;
                }
                recursive_value_depth = parent_depth;
            }
            SyntaxKind::CloseBracketToken => {
                let Some((open, parent_depth)) = delimiters.pop() else {
                    return JsonParserPreflight::UnsafeSyntax;
                };
                if open != SyntaxKind::OpenBracketToken {
                    return JsonParserPreflight::UnsafeSyntax;
                }
                recursive_value_depth = parent_depth;
            }
            SyntaxKind::StringLiteral
            | SyntaxKind::Identifier
            | SyntaxKind::NumericLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::CommaToken
            | SyntaxKind::ColonToken => {
                if matches!(kind, SyntaxKind::CommaToken | SyntaxKind::ColonToken) {
                    recursive_value_depth = delimiters
                        .last()
                        .map(|(_, parent_depth)| *parent_depth)
                        .unwrap_or(0);
                }
            }
            SyntaxKind::MinusToken if next == Some(SyntaxKind::NumericLiteral) => {}
            SyntaxKind::ExclamationToken
                if previous == Some(SyntaxKind::StringLiteral)
                    && next == Some(SyntaxKind::ColonToken) => {}
            kind if is_jsonc_keyword(kind) => {}
            // Anything else cannot survive convertToJson. Reject it before
            // the general expression parser can recurse through parentheses,
            // prefix operators, conditionals, functions, or templates.
            _ => return JsonParserPreflight::UnsafeSyntax,
        }
        previous = Some(kind);
    }
    // Missing closing delimiters are ordinary parser-recovery input. The
    // iterative stack above has already established the depth bound, so it is
    // safe to let parseJsonText produce its located EOF diagnostic and the
    // config path can still consume the recovered prefix object. Package JSON
    // remains fail-closed because its converter rejects parse diagnostics.
    JsonParserPreflight::Safe
}

fn is_jsonc_keyword(kind: SyntaxKind) -> bool {
    (kind as u16) >= (SyntaxKind::FirstKeyword as u16)
        && (kind as u16) <= (SyntaxKind::LastKeyword as u16)
}

fn is_recursive_value_token(kind: SyntaxKind) -> bool {
    // These tokens enter or extend recursive expression/type productions
    // before JSON conversion gets a chance to reject the value. Count them
    // cumulatively within each comma/colon-delimited value, rather than only
    // when consecutive: identifiers commonly separate infer constraints and
    // class heritage clauses. Property names are excluded by the caller.
    matches!(
        kind,
        SyntaxKind::ClassKeyword
            | SyntaxKind::DeleteKeyword
            | SyntaxKind::ExtendsKeyword
            | SyntaxKind::ImplementsKeyword
            | SyntaxKind::TypeOfKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::AwaitKeyword
            | SyntaxKind::YieldKeyword
            | SyntaxKind::NewKeyword
            | SyntaxKind::AsKeyword
            | SyntaxKind::AssertsKeyword
            | SyntaxKind::SatisfiesKeyword
            | SyntaxKind::InferKeyword
            | SyntaxKind::IsKeyword
            | SyntaxKind::KeyOfKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::UniqueKeyword
    )
}

fn finite_json_number(value: f64) -> Option<Number> {
    // Strict package numbers are normalized to serde's Float variant above,
    // so the extreme integer variants are unreachable from source JSON and
    // can losslessly tag JavaScript infinities without colliding with a
    // finite f64 value.
    if value == f64::INFINITY {
        Some(Number::from(u64::MAX))
    } else if value == f64::NEG_INFINITY {
        Some(Number::from(i64::MIN))
    } else {
        Number::from_f64(value)
    }
}

pub(crate) fn json_number_as_f64(number: &Number) -> Option<f64> {
    if number.as_u64() == Some(u64::MAX) {
        Some(f64::INFINITY)
    } else if number.as_i64() == Some(i64::MIN) {
        Some(f64::NEG_INFINITY)
    } else {
        number.as_f64()
    }
}

pub(crate) fn json_object_get<'a>(
    object: &'a Map<String, Value>,
    property: &str,
) -> Option<&'a Value> {
    json_object_own_get(object, property).or_else(|| match object.get(JSONC_PROTOTYPE_MARKER) {
        Some(Value::Object(prototype)) => json_object_get(prototype, property),
        Some(Value::Array(prototype)) => property
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == property)
            .and_then(|index| prototype.get(index)),
        Some(Value::Null) | None => None,
        Some(_) => {
            unreachable!("the JSONC converter stores only object, array, or null prototypes")
        }
    })
}

pub(crate) fn jsonc_prototype(object: &Map<String, Value>) -> Option<&Value> {
    object.get(JSONC_PROTOTYPE_MARKER)
}

pub(crate) fn json_object_own_get<'a>(
    object: &'a Map<String, Value>,
    property: &str,
) -> Option<&'a Value> {
    if property.starts_with('\0') {
        object.get(&format!("\0{property}"))
    } else {
        object.get(property)
    }
}

pub(crate) fn decode_user_object_key(key: &str) -> Option<&str> {
    if key == JSONC_PROTOTYPE_MARKER {
        None
    } else if key.starts_with("\0\0") {
        key.get(1..)
    } else {
        Some(key)
    }
}

fn jsonc_property_name(source: &SourceFile, name: NodeId, allow_recovery: bool) -> Option<String> {
    let node = source.arena.node(name);
    match node.kind {
        SyntaxKind::StringLiteral
            if allow_recovery || is_double_quoted_json_string(source, name) =>
        {
            node.data
                .as_string_literal()
                .map(|literal| literal.text.clone())
        }
        SyntaxKind::Identifier if allow_recovery => node
            .data
            .as_identifier()
            .map(|identifier| identifier.text.clone()),
        SyntaxKind::NumericLiteral if allow_recovery => node
            .data
            .as_numeric_literal()
            .map(|literal| literal.text.clone()),
        _ => None,
    }
}

pub(crate) fn is_double_quoted_json_string(source: &SourceFile, value: NodeId) -> bool {
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
#[path = "../tests/unit/json/tests.rs"]
mod tests;
