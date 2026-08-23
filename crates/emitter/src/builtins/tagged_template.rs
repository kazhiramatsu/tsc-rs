//! The `tagged-template` shared module (owner-graph `shared_modules[1]`,
//! `src/compiler/transformers/taggedTemplate.ts`).
//!
//! `transformES2015` is the module's only registered consumer
//! (`ProcessLevel::All`); the upstream ES2018 consumer
//! (`ProcessLevel::LiftRestriction`, `_tsc.js:102047-102056`) remains
//! unwired because parse records cannot carry the invalid-escape ES2018
//! transform-flag facet (the B-4 classifier disposition,
//! `builtins.rs` `createTaggedTemplateExpression` row) — the level arm is
//! representable here from day one so that lane's later owner wires a
//! call, not a rewrite.
//!
//! `templateFlags & TokenFlags.IsInvalid` is not persisted on parse
//! records (`nodes.rs` is generated); [`template_cooked_is_invalid`]
//! recomputes the only template-reachable half —
//! `TokenFlags::CONTAINS_INVALID_ESCAPE` — from the raw fragment bytes,
//! mirroring `scan_escape_sequence`'s decision structure exactly
//! (scanner.rs:1114-1282). The flag is a pure function of those bytes,
//! and the untagged parse path re-scans invalid templates into parse
//! errors (parser.rs:7116), so only tagged-position fragments ever reach
//! the predicate; the B-5 focused invalid-escape projections byte-compare
//! the result against oracle output.

use tsc_syntax::{nodes::NodeData, SyntaxKind};

use crate::{TransformError, TransformNode};

use super::es2015::Es2015Visitor;

/// tsc `ProcessLevel` (taggedTemplate.ts): `LiftRestriction` lowers only
/// templates whose cooked text is invalid (the ES2018 lane); `All` lowers
/// every tagged template (the ES2015 lane).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessLevel {
    #[allow(dead_code)] // the es2018 consumer's lane; see the module doc
    LiftRestriction,
    All,
}

/// tsc-port: processTaggedTemplateExpression @6.0.3
/// tsc-hash: d318d2539195d77c458bac08f12a8adfd7b03a2c933876e9f27df4bc4782446d
/// tsc-span: _tsc.js:93972-94018
pub(super) fn process_tagged_template_expression(
    host: &mut Es2015Visitor<'_, '_, '_>,
    node: TransformNode,
    level: ProcessLevel,
) -> Result<TransformNode, TransformError> {
    let (tag, template) = {
        let NodeData::TaggedTemplateExpression(data) = &host.arena_node(node)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TaggedTemplateExpression,
                field: "tagged template",
            });
        };
        let tag = data.tag.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::TaggedTemplateExpression,
            field: "tag",
        })?;
        let template = data.template.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::TaggedTemplateExpression,
            field: "template",
        })?;
        (host.node(tag), host.node(template))
    };
    let tag = host.visit_required_expression(tag)?;
    if level == ProcessLevel::LiftRestriction && !has_invalid_escape(host, template)? {
        return host.visit_each_child_required(node);
    }
    // `templateArguments[0]` is reserved for the template-object argument;
    // span expressions are visited IN the cooked/raw loop (nested tagged
    // templates therefore allocate and record their `templateObject`
    // temps before this one, matching the upstream visit order).
    let mut span_arguments: Vec<TransformNode> = Vec::new();
    let mut cooked_strings: Vec<TransformNode> = Vec::new();
    let mut raw_strings: Vec<TransformNode> = Vec::new();
    match host.arena_node(template)?.kind {
        SyntaxKind::NoSubstitutionTemplateLiteral => {
            cooked_strings.push(create_template_cooked(host, template)?);
            raw_strings.push(get_raw_literal(host, template)?);
        }
        SyntaxKind::TemplateExpression => {
            let (head, spans) = {
                let NodeData::TemplateExpression(data) = &host.arena_node(template)?.data else {
                    unreachable!("kind-checked template expression");
                };
                let head = data.head.ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TemplateExpression,
                    field: "head",
                })?;
                (host.node(head), host.array_nodes(data.template_spans)?)
            };
            cooked_strings.push(create_template_cooked(host, head)?);
            raw_strings.push(get_raw_literal(host, head)?);
            for span in spans {
                let (expression, literal) = {
                    let NodeData::TemplateSpan(data) = &host.arena_node(span)?.data else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::TemplateSpan,
                            field: "template span",
                        });
                    };
                    let expression =
                        data.expression
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::TemplateSpan,
                                field: "expression",
                            })?;
                    let literal = data.literal.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "literal",
                    })?;
                    (host.node(expression), host.node(literal))
                };
                cooked_strings.push(create_template_cooked(host, literal)?);
                raw_strings.push(get_raw_literal(host, literal)?);
                span_arguments.push(host.visit_required_expression(expression)?);
            }
        }
        _ => {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TaggedTemplateExpression,
                field: "template literal",
            });
        }
    }
    let cooked = host.create_array_literal(cooked_strings)?;
    let raw = host.create_array_literal(raw_strings)?;
    let helper_call = host.create_template_object_helper_call(cooked, raw)?;
    let template_object = if host.is_external_module_source()? {
        // `factory2.createUniqueName("templateObject")` — the eager
        // numbered arm; three identifier instances of ONE binding stand
        // for upstream's three references to one node.
        let binding = host.allocate_numbered_binding("templateObject")?;
        let declaration_name = host.create_generated_identifier(&binding)?;
        host.record_tagged_template_string(declaration_name)?;
        let or_left = host.create_generated_identifier(&binding)?;
        let assignment_left = host.create_generated_identifier(&binding)?;
        let assignment = host.create_assignment(assignment_left, helper_call)?;
        host.create_logical_or(or_left, assignment)?
    } else {
        helper_call
    };
    let mut arguments = Vec::with_capacity(1 + span_arguments.len());
    arguments.push(template_object);
    arguments.extend(span_arguments);
    host.create_call(tag, arguments)
}

/// tsc-port: createTemplateCooked @6.0.3
/// tsc-hash: 1f8f38eeb9dc74ce5fa36ea4158a351d9274f829b792c388a5a955c1c8253090
/// tsc-span: _tsc.js:94019-94021
fn create_template_cooked(
    host: &mut Es2015Visitor<'_, '_, '_>,
    template: TransformNode,
) -> Result<TransformNode, TransformError> {
    let (text, raw) = template_fragment_texts(host, template)?;
    if template_cooked_is_invalid(&raw) {
        host.create_void_zero()
    } else {
        host.create_string_literal(&text)
    }
}

/// tsc-port: getRawLiteral @6.0.3
/// tsc-hash: ed2b608e1bc5d71e6dbd771ebae0d3b917f9fe54d4c114a2520a15de62c6a854
/// tsc-span: _tsc.js:94022-94033
///
/// Parsed fragments always carry `raw_text` (the parser stores exactly
/// upstream's delimiter-stripped source slice, parser.rs:7258-7270), so
/// the upstream source-file substring fallback collapses to the stored
/// bytes; a synthesized fragment without `raw_text` is a typed error
/// (upstream asserts and slices garbage positions there — "Possibly bad
/// transform").
fn get_raw_literal(
    host: &mut Es2015Visitor<'_, '_, '_>,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    let (_, raw) = template_fragment_texts(host, node)?;
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let literal = host.create_string_literal(&text)?;
    host.set_text_range(literal, node)?;
    Ok(literal)
}

/// The fragment's `(cooked text, raw text)` pair, typed-failing on
/// non-fragment kinds and on a missing raw channel.
fn template_fragment_texts(
    host: &Es2015Visitor<'_, '_, '_>,
    node: TransformNode,
) -> Result<(String, String), TransformError> {
    let record = host.arena_node(node)?;
    let (text, raw_text) = match &record.data {
        NodeData::NoSubstitutionTemplateLiteral(data) => (&data.text, &data.raw_text),
        NodeData::TemplateHead(data) => (&data.text, &data.raw_text),
        NodeData::TemplateMiddle(data) => (&data.text, &data.raw_text),
        NodeData::TemplateTail(data) => (&data.text, &data.raw_text),
        _ => {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "template literal fragment",
            });
        }
    };
    let raw = raw_text
        .clone()
        .ok_or(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "template literal raw text",
        })?;
    Ok((text.clone(), raw))
}

/// tsc `hasInvalidEscape` over the template's fragments: any fragment
/// whose raw bytes contain an invalid escape. Reached only from the
/// `LiftRestriction` arm.
#[allow(dead_code)] // the es2018 consumer's lane; see the module doc
fn has_invalid_escape(
    host: &Es2015Visitor<'_, '_, '_>,
    template: TransformNode,
) -> Result<bool, TransformError> {
    let record = host.arena_node(template)?;
    match &record.data {
        NodeData::NoSubstitutionTemplateLiteral(_) => {
            let (_, raw) = template_fragment_texts(host, template)?;
            Ok(template_cooked_is_invalid(&raw))
        }
        NodeData::TemplateExpression(data) => {
            let head = data.head.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TemplateExpression,
                field: "head",
            })?;
            let (_, raw) = template_fragment_texts(host, host.node(head))?;
            if template_cooked_is_invalid(&raw) {
                return Ok(true);
            }
            for span in host.array_nodes(data.template_spans)? {
                let NodeData::TemplateSpan(span_data) = &host.arena_node(span)?.data else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "template span",
                    });
                };
                let literal = span_data
                    .literal
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "literal",
                    })?;
                let (_, raw) = template_fragment_texts(host, host.node(literal))?;
                if template_cooked_is_invalid(&raw) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "template literal",
        }),
    }
}

/// `templateFlags & TokenFlags.IsInvalid` recomputed from the raw
/// fragment bytes. For template literals the only reachable IsInvalid
/// member is `CONTAINS_INVALID_ESCAPE`; the walk mirrors
/// `scan_escape_sequence`'s consumption and flagging exactly
/// (scanner.rs:1114-1282):
/// octal escapes (`\0`+digit, `\1`-`\7`), `\8`/`\9`, short `\x`/`\u`
/// hex runs, and malformed or out-of-range `\u{...}` set the flag; line
/// continuations, recognized single-char escapes, arbitrary escaped
/// characters, and a lone trailing backslash (the scanner's
/// unexpected-end path) do not.
fn template_cooked_is_invalid(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut pos = 0_usize;
    while pos < bytes.len() {
        if bytes[pos] != b'\\' {
            pos += 1;
            continue;
        }
        pos += 1;
        let Some(&ch) = bytes.get(pos) else {
            return false;
        };
        pos += 1;
        match ch {
            b'0' => {
                // `\0` alone is the NUL escape; `\0` followed by a digit
                // enters `scan_octal_escape`, which flags the invalid escape
                // immediately (further consumption cannot change the answer).
                if bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                    return true;
                }
            }
            b'1'..=b'7' => return true,
            b'8' | b'9' => return true,
            b'u' => {
                if bytes.get(pos) == Some(&b'{') {
                    pos += 1;
                    let digits_start = pos;
                    while bytes.get(pos).is_some_and(u8::is_ascii_hexdigit) {
                        pos += 1;
                    }
                    let value = std::str::from_utf8(&bytes[digits_start..pos])
                        .ok()
                        .filter(|digits| !digits.is_empty())
                        .and_then(|digits| u32::from_str_radix(digits, 16).ok());
                    match value {
                        None => return true,
                        Some(value) if value > 0x10ffff => return true,
                        Some(_) => {}
                    }
                    if bytes.get(pos) == Some(&b'}') {
                        pos += 1;
                    } else {
                        return true;
                    }
                } else {
                    for _ in 0..4 {
                        if !bytes.get(pos).is_some_and(u8::is_ascii_hexdigit) {
                            return true;
                        }
                        pos += 1;
                    }
                }
            }
            b'x' => {
                for _ in 0..2 {
                    if !bytes.get(pos).is_some_and(u8::is_ascii_hexdigit) {
                        return true;
                    }
                    pos += 1;
                }
            }
            b'\r' => {
                if bytes.get(pos) == Some(&b'\n') {
                    pos += 1;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
#[path = "../../tests/unit/tagged_template/tests.rs"]
mod tagged_template_tests;
