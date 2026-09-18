#!/usr/bin/env python3
"""EF6-JSDOC-LINK: JSDoc comment text keeps {@link}/{@linkcode}/{@linkplain} parts (getTextOfJSDocComment + formatJSDocLink)
at every node-builder site, not only the typedef alias comment."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/checker/src/node_builder/type_nodes.rs", [
("""/// tsc-port: preserveCommentsOn @6.0.3
/// tsc-hash: 151533253991304ca0ea7538723109604fab53e462040768f2ee1a7cc26a0bda
/// tsc-span: _tsc.js:52384-52396
fn preserve_comments_on(
""", """/// tsc-port: getTextOfJSDocComment / formatJSDocLink @6.0.3
/// tsc-span: _tsc.js:11773-11781
///
/// A parsed JSDoc comment is either one string or a list of text and link
/// parts; every link part is re-spelled as `{@link name text}` (with tsc's
/// space rule), so `@property` and typedef comments keep their links.
pub(super) fn js_doc_comment_text(
    checker: &CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    comment: Option<&tsc_syntax::nodes::JSDocComment>,
) -> BuildResult<Option<String>> {
    match comment {
        None => Ok(None),
        Some(tsc_syntax::nodes::JSDocComment::Text(text)) => Ok(Some(text.clone())),
        Some(tsc_syntax::nodes::JSDocComment::Nodes(nodes)) => {
            let mut text = String::new();
            for node in checker.nodes_of(Some(*nodes)) {
                let (kind, name, link_text) = match checker.data_of(node).clone() {
                    NodeData::JSDocText(data) => {
                        text.push_str(&data.text);
                        continue;
                    }
                    NodeData::JSDocLink(data) => ("link", data.name, data.text),
                    NodeData::JSDocLinkCode(data) => ("linkcode", data.name, data.text),
                    NodeData::JSDocLinkPlain(data) => ("linkplain", data.name, data.text),
                    _ => continue,
                };
                let entity = name
                    .map(|name| checker.entity_name_to_string(name))
                    .transpose()
                    .map_err(|abort| checker_abort_error(checker, context, abort))?
                    .unwrap_or_default();
                let space = if name.is_some() && (link_text.is_empty() || link_text.starts_with("://")) {
                    ""
                } else {
                    " "
                };
                text.push_str(&format!("{{@{kind} {entity}{space}{link_text}}}"));
            }
            Ok(Some(text))
        }
    }
}

/// tsc-port: preserveCommentsOn @6.0.3
/// tsc-hash: 151533253991304ca0ea7538723109604fab53e462040768f2ee1a7cc26a0bda
/// tsc-span: _tsc.js:52384-52396
fn preserve_comments_on(
"""),
("""            let comment_text = match data.comment.as_ref() {
                Some(tsc_syntax::nodes::JSDocComment::Text(text)) => Some(text.clone()),
                Some(tsc_syntax::nodes::JSDocComment::Nodes(nodes)) => {
                    let text = checker
                        .nodes_of(Some(*nodes))
                        .into_iter()
                        .filter_map(|node| match checker.data_of(node) {
                            NodeData::JSDocText(data) => Some(data.text.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    (!text.is_empty()).then_some(text)
                }
                None => None,
            };
""", """            let comment_text = js_doc_comment_text(checker, context, data.comment.as_ref())?
                .filter(|text| !text.is_empty());
"""),
])
print("EF6-JSDOC-LINK patch applied")
