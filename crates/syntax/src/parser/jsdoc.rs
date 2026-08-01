use super::{token_is_identifier_or_keyword, Parser};
use crate::nodes::{
    ExpressionWithTypeArgumentsData, IdentifierData, JSDocAugmentsTagData, JSDocAuthorTagData,
    JSDocCallbackTagData, JSDocClassTagData, JSDocComment, JSDocData, JSDocDeprecatedTagData,
    JSDocEnumTagData, JSDocImplementsTagData, JSDocImportTagData, JSDocLinkCodeData, JSDocLinkData,
    JSDocLinkPlainData, JSDocMemberNameData, JSDocNameReferenceData, JSDocOverloadTagData,
    JSDocOverrideTagData, JSDocParameterTagData, JSDocPrivateTagData, JSDocPropertyTagData,
    JSDocProtectedTagData, JSDocPublicTagData, JSDocReadonlyTagData, JSDocReturnTagData,
    JSDocSatisfiesTagData, JSDocSeeTagData, JSDocSignatureData, JSDocTagData, JSDocTemplateTagData,
    JSDocTextData, JSDocThisTagData, JSDocThrowsTagData, JSDocTypeExpressionData,
    JSDocTypeLiteralData, JSDocTypeTagData, JSDocTypedefTagData, ModuleDeclarationData, NodeData,
    NodeId, PropertyAccessExpressionData, QualifiedNameData, TypeParameterData,
};
use crate::SyntaxKind;
use tsc_diagnostics::{gen, DiagnosticMessage, RelatedInfo};
use tsc_types::NodeFlags;

const TARGET_PROPERTY: u8 = 1;
const TARGET_PARAMETER: u8 = 2;
const TARGET_CALLBACK_PARAMETER: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommentState {
    BeginningOfLine,
    SawAsterisk,
    SavingComments,
    SavingBackticks,
}

pub(super) struct ParsedJSDoc {
    pub(super) node: NodeId,
    pub(super) deprecated: bool,
}

struct JSDocParser<'parser, 'text> {
    parser: &'parser mut Parser<'text>,
    tags: Vec<NodeId>,
    tags_pos: Option<usize>,
    tags_end: Option<usize>,
    has_deprecated_tag: bool,
}

impl<'parser, 'text> JSDocParser<'parser, 'text> {
    fn new(parser: &'parser mut Parser<'text>) -> Self {
        Self {
            parser,
            tags: Vec::new(),
            tags_pos: None,
            tags_end: None,
            has_deprecated_tag: false,
        }
    }

    fn token(&self) -> SyntaxKind {
        self.parser.token()
    }

    fn node_pos(&self) -> usize {
        self.parser.scanner.full_start_pos()
    }

    fn token_start(&self) -> usize {
        self.parser.scanner.token_start()
    }

    fn token_end(&self) -> usize {
        self.parser.scanner.pos()
    }

    fn token_text(&self) -> String {
        self.parser.scanner.token_text().to_owned()
    }

    fn token_value(&self) -> String {
        if self.parser.scanner.token_value().is_empty() {
            self.token_text()
        } else {
            self.parser.scanner.token_value().to_owned()
        }
    }

    fn text_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    fn trim_end(text: &str) -> &str {
        text.trim_end_matches(crate::scanner::is_js_whitespace)
    }

    /// JavaScript String#slice start semantics. JavaScript indexes strings in
    /// UTF-16 code units, including for non-ASCII indentation whitespace.
    fn slice_indent(text: &str, start: isize) -> String {
        let len = Self::text_len(text);
        let start = if start < 0 {
            len.saturating_sub(start.unsigned_abs())
        } else {
            (start as usize).min(len)
        };
        if start == 0 {
            return text.to_owned();
        }
        if start == len {
            return String::new();
        }

        let mut utf16_pos = 0;
        for (byte_pos, ch) in text.char_indices() {
            if utf16_pos == start {
                return text[byte_pos..].to_owned();
            }
            let next = utf16_pos + ch.len_utf16();
            if start < next {
                // Indentation tokens only contain BMP whitespace, so this is
                // unreachable for parser callers. Keep the helper total if it
                // is ever reused with a supplementary-plane character.
                return String::from_utf16_lossy(
                    &text.encode_utf16().skip(start).collect::<Vec<_>>(),
                );
            }
            utf16_pos = next;
        }
        String::new()
    }

    fn next_token_jsdoc(&mut self) -> SyntaxKind {
        let token = self.parser.scanner.scan_jsdoc_token();
        self.parser.drain_scanner_errors();
        token
    }

    fn next_comment_text_token(&mut self, in_backticks: bool) -> SyntaxKind {
        let token = self
            .parser
            .scanner
            .scan_jsdoc_comment_text_token(in_backticks);
        self.parser.drain_scanner_errors();
        token
    }

    fn parse_optional(&mut self, kind: SyntaxKind) -> bool {
        if self.token() == kind {
            self.next_token_jsdoc();
            true
        } else {
            false
        }
    }

    /// tsc-port: parseExpectedJSDoc @6.0.3
    /// tsc-hash: 5752be124f97a11a7e2f7e6af19208ddc36ddd9a127d2921c9018a0111f6a185
    /// tsc-span: _tsc.js:29678-29686
    /// d2: d2:f734756314d2535619516fd4d1feef44e92506df26798d86fbaa59b0f8ae9c09
    fn parse_expected(&mut self, kind: SyntaxKind) -> bool {
        if self.parse_optional(kind) {
            true
        } else {
            self.parser
                .parse_error_at_current_token(&gen::_0_expected, &[&super::token_to_string(kind)]);
            false
        }
    }

    fn finish(&mut self, data: NodeData, pos: usize, end: usize) -> NodeId {
        self.finish_with_flags(data, pos, end, NodeFlags::NONE)
    }

    fn finish_with_flags(
        &mut self,
        data: NodeData,
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> NodeId {
        let id = self
            .parser
            .arena
            .alloc_node(data, pos, end, self.parser.jsdoc_flags() | flags);
        self.parser.finish_node_at(id, pos, end)
    }

    fn finish_current(&mut self, data: NodeData, pos: usize) -> NodeId {
        self.finish(data, pos, self.node_pos())
    }

    fn alloc_array(&mut self, nodes: Vec<NodeId>, pos: usize, end: usize) -> crate::NodeArrayId {
        self.parser.arena.alloc_array(nodes, pos, end, false)
    }

    /// tsc-port: parseJSDocIdentifierName @6.0.3
    /// tsc-hash: 78215969ec15ba75b198cf8c0bb5d05364314b74a24edc2ab3673fe6da3f454f
    /// tsc-span: _tsc.js:35782-35799
    /// d2: d2:e9f169197af45c251fa2e5f2e93467061c25ff6cfeba65771e6fda87350e6d4f
    fn parse_identifier_name(&mut self, message: Option<&'static DiagnosticMessage>) -> NodeId {
        if !token_is_identifier_or_keyword(self.token()) {
            return self.parser.create_missing_node(
                SyntaxKind::Identifier,
                message.is_none(),
                Some(message.unwrap_or(&gen::Identifier_expected)),
                &[],
            );
        }
        // parseJSDocIdentifierName uses getTokenStart, not getNodePos.
        // The distinction is observable when an ordinary-parser operation
        // immediately preceded this JSDoc identifier and consumed trivia.
        let pos = self.token_start();
        let end = self.token_end();
        let text = self.token_value();
        let id = self.finish(
            NodeData::Identifier(IdentifierData {
                escaped_text: crate::escape_leading_underscores(&text),
                text,
            }),
            pos,
            end,
        );
        self.next_token_jsdoc();
        id
    }

    fn identifier_text(&self, id: NodeId) -> Option<&str> {
        match &self.parser.arena.node(id).data {
            NodeData::Identifier(data) => Some(&data.text),
            _ => None,
        }
    }

    fn skip_whitespace(&mut self) {
        if matches!(
            self.token(),
            SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
        ) {
            let state = self.parser.scanner.save();
            loop {
                self.next_token_jsdoc();
                if self.token() == SyntaxKind::EndOfFileToken {
                    self.parser.scanner.restore(state);
                    return;
                }
                if !matches!(
                    self.token(),
                    SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
                ) {
                    self.parser.scanner.restore(state);
                    break;
                }
            }
        }
        while matches!(
            self.token(),
            SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
        ) {
            self.next_token_jsdoc();
        }
    }

    fn skip_whitespace_or_asterisk(&mut self) -> String {
        if matches!(
            self.token(),
            SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
        ) {
            let state = self.parser.scanner.save();
            loop {
                self.next_token_jsdoc();
                if self.token() == SyntaxKind::EndOfFileToken {
                    self.parser.scanner.restore(state);
                    return String::new();
                }
                if !matches!(
                    self.token(),
                    SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
                ) {
                    self.parser.scanner.restore(state);
                    break;
                }
            }
        }
        let mut preceding_line_break = self.parser.scanner.has_preceding_line_break();
        let mut seen_line_break = false;
        let mut indent_text = String::new();
        while preceding_line_break && self.token() == SyntaxKind::AsteriskToken
            || matches!(
                self.token(),
                SyntaxKind::WhitespaceTrivia | SyntaxKind::NewLineTrivia
            )
        {
            indent_text.push_str(&self.token_text());
            match self.token() {
                SyntaxKind::NewLineTrivia => {
                    preceding_line_break = true;
                    seen_line_break = true;
                    indent_text.clear();
                }
                SyntaxKind::AsteriskToken => preceding_line_break = false,
                _ => {}
            }
            self.next_token_jsdoc();
        }
        if seen_line_break {
            indent_text
        } else {
            String::new()
        }
    }

    fn remove_leading_newlines(comments: &mut Vec<String>) {
        while comments
            .first()
            .is_some_and(|text| text == "\n" || text == "\r")
        {
            comments.remove(0);
        }
    }

    fn remove_trailing_whitespace(comments: &mut Vec<String>) {
        while let Some(last) = comments.last_mut() {
            let trimmed = Self::trim_end(last).to_owned();
            if trimmed.is_empty() {
                comments.pop();
            } else if trimmed.len() != last.len() {
                *last = trimmed;
                break;
            } else {
                break;
            }
        }
    }

    fn comment_value(
        &mut self,
        parts: Vec<NodeId>,
        text: String,
        pos: usize,
        end: usize,
    ) -> Option<JSDocComment> {
        if !parts.is_empty() {
            Some(JSDocComment::Nodes(self.alloc_array(parts, pos, end)))
        } else if !text.is_empty() {
            Some(JSDocComment::Text(text))
        } else {
            None
        }
    }

    fn add_tag(&mut self, tag: NodeId) {
        if self.tags.is_empty() {
            self.tags_pos = Some(self.parser.arena.node(tag).pos as usize);
        }
        self.tags_end = Some(self.parser.arena.node(tag).end as usize);
        self.tags.push(tag);
    }

    fn parse_comment_worker(&mut self, start: usize, end: usize) -> NodeId {
        let mut state = CommentState::SawAsterisk;
        let mut margin: Option<usize> = None;
        let line_start = self.parser.source_text[..start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let mut indent = Self::text_len(&self.parser.source_text[line_start..start]) + 4;
        let mut comments = Vec::<String>::new();
        let mut parts = Vec::<NodeId>::new();
        let mut link_end: Option<usize> = None;
        let mut comments_pos: Option<usize> = None;

        self.next_token_jsdoc();
        while self.parse_optional(SyntaxKind::WhitespaceTrivia) {}
        if self.parse_optional(SyntaxKind::NewLineTrivia) {
            state = CommentState::BeginningOfLine;
            indent = 0;
        }

        loop {
            match self.token() {
                SyntaxKind::AtToken => {
                    Self::remove_trailing_whitespace(&mut comments);
                    comments_pos.get_or_insert(self.node_pos());
                    let tag = self.parse_tag(indent);
                    self.add_tag(tag);
                    state = CommentState::BeginningOfLine;
                    margin = None;
                }
                SyntaxKind::NewLineTrivia => {
                    comments.push(self.token_text());
                    state = CommentState::BeginningOfLine;
                    indent = 0;
                }
                SyntaxKind::AsteriskToken => {
                    let asterisk = self.token_text();
                    if state == CommentState::SawAsterisk {
                        state = CommentState::SavingComments;
                        if margin.is_none() || margin == Some(0) {
                            margin = Some(indent);
                        }
                        comments.push(asterisk.clone());
                        indent += Self::text_len(&asterisk);
                    } else {
                        state = CommentState::SawAsterisk;
                        indent += Self::text_len(&asterisk);
                    }
                }
                SyntaxKind::WhitespaceTrivia => {
                    let whitespace = self.token_text();
                    if let Some(margin) = margin {
                        let whitespace_len = Self::text_len(&whitespace);
                        if indent + whitespace_len > margin {
                            comments.push(Self::slice_indent(
                                &whitespace,
                                margin as isize - indent as isize,
                            ));
                        }
                    }
                    indent += Self::text_len(&whitespace);
                }
                SyntaxKind::EndOfFileToken => break,
                SyntaxKind::JSDocCommentTextToken => {
                    state = CommentState::SavingComments;
                    let value = self.token_value();
                    if margin.is_none() || margin == Some(0) {
                        margin = Some(indent);
                    }
                    indent += Self::text_len(&value);
                    comments.push(value);
                }
                SyntaxKind::OpenBraceToken => {
                    state = CommentState::SavingComments;
                    let comment_end = self.parser.scanner.full_start_pos();
                    let link_start = self.token_end().saturating_sub(1);
                    if let Some(link) = self.try_parse_jsdoc_link(link_start) {
                        if link_end.is_none() {
                            Self::remove_leading_newlines(&mut comments);
                        }
                        let text = comments.join("");
                        parts.push(self.finish(
                            NodeData::JSDocText(JSDocTextData { text }),
                            link_end.unwrap_or(start),
                            comment_end,
                        ));
                        parts.push(link);
                        comments.clear();
                        link_end = Some(self.token_end());
                    } else {
                        comments.push(self.token_text());
                    }
                }
                _ => {
                    state = CommentState::SavingComments;
                    let text = self.token_text();
                    if margin.is_none() || margin == Some(0) {
                        margin = Some(indent);
                    }
                    indent += Self::text_len(&text);
                    comments.push(text);
                }
            }
            if state == CommentState::SavingComments {
                self.next_comment_text_token(false);
            } else {
                self.next_token_jsdoc();
            }
        }

        let scan_end = self.node_pos();
        let joined_comments = comments.join("");
        let trimmed_comments = Self::trim_end(&joined_comments).to_owned();
        if !parts.is_empty() && !trimmed_comments.is_empty() {
            parts.push(self.finish(
                NodeData::JSDocText(JSDocTextData {
                    text: trimmed_comments.clone(),
                }),
                link_end.unwrap_or(start),
                comments_pos.unwrap_or(scan_end),
            ));
        }
        let comment = self.comment_value(
            parts,
            trimmed_comments,
            start,
            comments_pos.unwrap_or(scan_end),
        );
        let tags = if self.tags.is_empty() {
            None
        } else {
            let tags = std::mem::take(&mut self.tags);
            Some(self.alloc_array(
                tags,
                self.tags_pos.unwrap_or(start),
                self.tags_end.unwrap_or(end),
            ))
        };
        self.finish(NodeData::JSDoc(JSDocData { tags, comment }), start, end)
    }

    fn parse_tag_comments(
        &mut self,
        mut indent: usize,
        initial_margin: Option<String>,
    ) -> Option<JSDocComment> {
        let comments_pos = self.node_pos();
        let mut comments = Vec::<String>::new();
        let mut parts = Vec::<NodeId>::new();
        let mut link_end = None;
        let mut state = CommentState::BeginningOfLine;
        let mut margin: Option<usize> = None;
        if let Some(initial) = initial_margin {
            if !initial.is_empty() {
                margin = Some(indent);
                indent += Self::text_len(&initial);
                comments.push(initial);
            }
            state = CommentState::SawAsterisk;
        }

        loop {
            match self.token() {
                SyntaxKind::NewLineTrivia => {
                    state = CommentState::BeginningOfLine;
                    comments.push(self.token_text());
                    indent = 0;
                }
                SyntaxKind::AtToken => {
                    self.parser
                        .scanner
                        .reset_token_state(self.token_end().saturating_sub(1));
                    break;
                }
                SyntaxKind::EndOfFileToken => break,
                SyntaxKind::WhitespaceTrivia => {
                    let whitespace = self.token_text();
                    if let Some(margin) = margin {
                        let whitespace_len = Self::text_len(&whitespace);
                        if indent + whitespace_len > margin {
                            comments.push(Self::slice_indent(
                                &whitespace,
                                margin as isize - indent as isize,
                            ));
                            state = CommentState::SavingComments;
                        }
                    }
                    indent += Self::text_len(&whitespace);
                }
                SyntaxKind::OpenBraceToken => {
                    state = CommentState::SavingComments;
                    let comment_end = self.node_pos();
                    let link_start = self.token_end().saturating_sub(1);
                    if let Some(link) = self.try_parse_jsdoc_link(link_start) {
                        parts.push(self.finish(
                            NodeData::JSDocText(JSDocTextData {
                                text: comments.join(""),
                            }),
                            link_end.unwrap_or(comments_pos),
                            comment_end,
                        ));
                        parts.push(link);
                        comments.clear();
                        link_end = Some(self.token_end());
                    } else {
                        comments.push(self.token_text());
                        if margin.is_none() || margin == Some(0) {
                            margin = Some(indent);
                        }
                        indent += Self::text_len(&self.token_text());
                    }
                }
                SyntaxKind::BacktickToken => {
                    state = if state == CommentState::SavingBackticks {
                        CommentState::SavingComments
                    } else {
                        CommentState::SavingBackticks
                    };
                    comments.push(self.token_text());
                    if margin.is_none() || margin == Some(0) {
                        margin = Some(indent);
                    }
                    indent += Self::text_len(&self.token_text());
                }
                SyntaxKind::JSDocCommentTextToken => {
                    if state != CommentState::SavingBackticks {
                        state = CommentState::SavingComments;
                    }
                    let value = self.token_value();
                    if margin.is_none() || margin == Some(0) {
                        margin = Some(indent);
                    }
                    indent += Self::text_len(&value);
                    comments.push(value);
                }
                SyntaxKind::AsteriskToken if state == CommentState::BeginningOfLine => {
                    state = CommentState::SawAsterisk;
                    indent += 1;
                }
                _ => {
                    if state != CommentState::SavingBackticks {
                        state = CommentState::SavingComments;
                    }
                    let text = self.token_text();
                    if margin.is_none() || margin == Some(0) {
                        margin = Some(indent);
                    }
                    indent += Self::text_len(&text);
                    comments.push(text);
                }
            }
            if matches!(
                state,
                CommentState::SavingComments | CommentState::SavingBackticks
            ) {
                self.next_comment_text_token(state == CommentState::SavingBackticks);
            } else {
                self.next_token_jsdoc();
            }
        }

        Self::remove_leading_newlines(&mut comments);
        let joined_comments = comments.join("");
        let trimmed = Self::trim_end(&joined_comments).to_owned();
        if !parts.is_empty() && !trimmed.is_empty() {
            parts.push(self.finish(
                NodeData::JSDocText(JSDocTextData {
                    text: trimmed.clone(),
                }),
                link_end.unwrap_or(comments_pos),
                self.node_pos(),
            ));
        }
        self.comment_value(parts, trimmed, comments_pos, self.token_end())
    }

    fn parse_trailing_tag_comments(
        &mut self,
        pos: usize,
        end: usize,
        margin: usize,
        indent_text: String,
    ) -> Option<JSDocComment> {
        let adjusted = if indent_text.is_empty() {
            margin
                + self
                    .parser
                    .source_text
                    .get(pos..end)
                    .map_or_else(|| end.saturating_sub(pos), Self::text_len)
        } else {
            margin
        };
        let initial = Self::slice_indent(&indent_text, adjusted as isize);
        self.parse_tag_comments(adjusted, Some(initial))
    }

    fn try_parse_jsdoc_link(&mut self, start: usize) -> Option<NodeId> {
        let scanner_state = self.parser.scanner.save();
        let diagnostics_len = self.parser.parse_diagnostics.len();
        let parse_error = self.parser.parse_error_before_next_finished_node;
        let result = self.parse_jsdoc_link(start);
        if result.is_none() {
            self.parser.scanner.restore(scanner_state);
            self.parser.parse_diagnostics.truncate(diagnostics_len);
            self.parser.parse_error_before_next_finished_node = parse_error;
        }
        result
    }

    fn parse_jsdoc_link(&mut self, start: usize) -> Option<NodeId> {
        let link_type = self.parse_jsdoc_link_prefix()?;
        self.next_token_jsdoc();
        self.skip_whitespace();
        let name = self.parse_jsdoc_link_name();
        let mut text = String::new();
        while !matches!(
            self.token(),
            SyntaxKind::CloseBraceToken | SyntaxKind::NewLineTrivia | SyntaxKind::EndOfFileToken
        ) {
            text.push_str(&self.token_text());
            self.next_token_jsdoc();
        }
        let end = self.token_end();
        let data = match link_type.as_str() {
            "link" => NodeData::JSDocLink(JSDocLinkData { name, text }),
            "linkcode" => NodeData::JSDocLinkCode(JSDocLinkCodeData { name, text }),
            _ => NodeData::JSDocLinkPlain(JSDocLinkPlainData { name, text }),
        };
        Some(self.finish(data, start, end))
    }

    fn parse_jsdoc_link_prefix(&mut self) -> Option<String> {
        self.skip_whitespace_or_asterisk();
        if self.token() != SyntaxKind::OpenBraceToken {
            return None;
        }
        self.next_token_jsdoc();
        if self.token() != SyntaxKind::AtToken {
            return None;
        }
        self.next_token_jsdoc();
        if !token_is_identifier_or_keyword(self.token()) {
            return None;
        }
        let kind = self.token_value();
        matches!(kind.as_str(), "link" | "linkcode" | "linkplain").then_some(kind)
    }

    fn parse_jsdoc_link_name(&mut self) -> Option<NodeId> {
        if !token_is_identifier_or_keyword(self.token()) {
            return None;
        }
        let pos = self.node_pos();
        let mut name = self.parser.parse_identifier_name(None);
        while self.parser.parse_optional(SyntaxKind::DotToken) {
            let right = if self.token() == SyntaxKind::PrivateIdentifier {
                self.parser
                    .create_missing_node(SyntaxKind::Identifier, false, None, &[])
            } else {
                self.parser.parse_identifier_name(None)
            };
            name = self.finish_current(
                NodeData::QualifiedName(QualifiedNameData {
                    left: Some(name),
                    right: Some(right),
                }),
                pos,
            );
        }
        while self.token() == SyntaxKind::PrivateIdentifier {
            self.parser.scanner.re_scan_hash_token();
            self.next_token_jsdoc();
            let right = self.parser.parse_identifier();
            name = self.finish_current(
                NodeData::JSDocMemberName(JSDocMemberNameData {
                    left: Some(name),
                    right: Some(right),
                }),
                pos,
            );
        }
        Some(name)
    }

    fn parse_jsdoc_entity_name(&mut self) -> NodeId {
        let mut entity = self.parse_identifier_name(None);
        let pos = self.parser.arena.node(entity).pos as usize;
        if self.parser.parse_optional(SyntaxKind::OpenBracketToken) {
            self.parser
                .parse_expected(SyntaxKind::CloseBracketToken, None);
        }
        while self.parser.parse_optional(SyntaxKind::DotToken) {
            let right = self.parse_identifier_name(None);
            if self.parser.parse_optional(SyntaxKind::OpenBracketToken) {
                self.parser
                    .parse_expected(SyntaxKind::CloseBracketToken, None);
            }
            entity = self.finish_current(
                NodeData::QualifiedName(QualifiedNameData {
                    left: Some(entity),
                    right: Some(right),
                }),
                pos,
            );
        }
        entity
    }

    fn parse_jsdoc_name_reference(&mut self) -> NodeId {
        let pos = self.node_pos();
        let has_brace = self.parser.parse_optional(SyntaxKind::OpenBraceToken);
        let member_pos = self.node_pos();
        let mut entity = self.parser.parse_entity_name(false, None);
        while self.token() == SyntaxKind::PrivateIdentifier {
            self.parser.scanner.re_scan_hash_token();
            self.next_token_jsdoc();
            let right = self.parser.parse_identifier();
            entity = self.finish_current(
                NodeData::JSDocMemberName(JSDocMemberNameData {
                    left: Some(entity),
                    right: Some(right),
                }),
                member_pos,
            );
        }
        if has_brace {
            self.parse_expected(SyntaxKind::CloseBraceToken);
        }
        self.finish_current(
            NodeData::JSDocNameReference(JSDocNameReferenceData { name: Some(entity) }),
            pos,
        )
    }

    fn parse_tag(&mut self, margin: usize) -> NodeId {
        debug_assert_eq!(self.token(), SyntaxKind::AtToken);
        let start = self.token_start();
        self.next_token_jsdoc();
        let tag_name = self.parse_identifier_name(None);
        let name = self
            .identifier_text(tag_name)
            .unwrap_or_default()
            .to_owned();
        let indent_text = self.skip_whitespace_or_asterisk();
        match name.as_str() {
            "author" => self.parse_author_tag(start, tag_name, margin, indent_text),
            "implements" => self.parse_implements_tag(start, tag_name, margin, indent_text),
            "augments" | "extends" => self.parse_augments_tag(start, tag_name, margin, indent_text),
            "class" | "constructor" => {
                let comment =
                    self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
                self.finish_current(
                    NodeData::JSDocClassTag(JSDocClassTagData {
                        tag_name: Some(tag_name),
                        comment,
                    }),
                    start,
                )
            }
            "public" => self.parse_simple_tag(start, tag_name, margin, indent_text, 0),
            "private" => self.parse_simple_tag(start, tag_name, margin, indent_text, 1),
            "protected" => self.parse_simple_tag(start, tag_name, margin, indent_text, 2),
            "readonly" => self.parse_simple_tag(start, tag_name, margin, indent_text, 3),
            "override" => self.parse_simple_tag(start, tag_name, margin, indent_text, 4),
            "deprecated" => {
                self.has_deprecated_tag = true;
                self.parse_simple_tag(start, tag_name, margin, indent_text, 5)
            }
            "this" => self.parse_this_tag(start, tag_name, margin, indent_text),
            "enum" => self.parse_enum_tag(start, tag_name, margin, indent_text),
            "arg" | "argument" | "param" => {
                self.parse_parameter_or_property_tag(start, tag_name, TARGET_PARAMETER, margin)
            }
            "return" | "returns" => self.parse_return_tag(start, tag_name, margin, indent_text),
            "template" => self.parse_template_tag(start, tag_name, margin, indent_text),
            "type" => self.parse_type_tag(start, tag_name, Some((margin, indent_text))),
            "typedef" => self.parse_typedef_tag(start, tag_name, margin, indent_text),
            "callback" => self.parse_callback_tag(start, tag_name, margin, indent_text),
            "overload" => self.parse_overload_tag(start, tag_name, margin, indent_text),
            "satisfies" => self.parse_satisfies_tag(start, tag_name, margin, indent_text),
            "see" => self.parse_see_tag(start, tag_name, margin, indent_text),
            "exception" | "throws" => self.parse_throws_tag(start, tag_name, margin, indent_text),
            "import" => self.parse_import_tag(start, tag_name, margin, indent_text),
            _ => {
                let comment =
                    self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
                self.finish_current(
                    NodeData::JSDocTag(JSDocTagData {
                        tag_name: Some(tag_name),
                        comment,
                    }),
                    start,
                )
            }
        }
    }

    fn parse_simple_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
        kind: u8,
    ) -> NodeId {
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        let data = match kind {
            0 => NodeData::JSDocPublicTag(JSDocPublicTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            1 => NodeData::JSDocPrivateTag(JSDocPrivateTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            2 => NodeData::JSDocProtectedTag(JSDocProtectedTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            3 => NodeData::JSDocReadonlyTag(JSDocReadonlyTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            4 => NodeData::JSDocOverrideTag(JSDocOverrideTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            _ => NodeData::JSDocDeprecatedTag(JSDocDeprecatedTagData {
                tag_name: Some(tag_name),
                comment,
            }),
        };
        self.finish_current(data, start)
    }

    fn try_parse_type_expression(&mut self) -> Option<NodeId> {
        self.skip_whitespace_or_asterisk();
        (self.token() == SyntaxKind::OpenBraceToken)
            .then(|| self.parse_jsdoc_type_expression(false))
    }

    /// tsc-port: parseJSDocTypeExpression @6.0.3
    /// tsc-hash: 325ea8484a74cb8af2c59724306cb66852667232a2d61cb1de369487261e7a6e
    /// tsc-span: _tsc.js:34787-34797
    /// d2: d2:cf43460ba6610f9063b48070240cc02225ad0587f4de3225328c61cb7e610c79
    fn parse_jsdoc_type_expression(&mut self, may_omit_braces: bool) -> NodeId {
        let pos = self.node_pos();
        let has_brace = self.token() == SyntaxKind::OpenBraceToken;
        if has_brace {
            // parseExpected in tsc switches from the JSDoc scanner to the
            // ordinary scanner for the type grammar.
            self.parser.next_token();
        } else if !may_omit_braces {
            self.parser.parse_error_at_current_token(
                &gen::_0_expected,
                &[&super::token_to_string(SyntaxKind::OpenBraceToken)],
            );
        }
        let r#type = self.parser.parse_jsdoc_type();
        if !may_omit_braces || has_brace {
            if self.token() == SyntaxKind::CloseBraceToken {
                self.next_token_jsdoc();
            } else {
                self.parser.parse_error_at_current_token(
                    &gen::_0_expected,
                    &[&super::token_to_string(SyntaxKind::CloseBraceToken)],
                );
            }
        }
        self.finish_current(
            NodeData::JSDocTypeExpression(JSDocTypeExpressionData {
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn parse_bracket_name_in_property_and_param_tag(&mut self) -> (NodeId, bool) {
        let is_bracketed = self.parse_optional(SyntaxKind::OpenBracketToken);
        if is_bracketed {
            self.skip_whitespace();
        }
        let is_backquoted = self.parse_optional(SyntaxKind::BacktickToken);
        let name = self.parse_jsdoc_entity_name();
        if is_backquoted && !self.parse_optional(SyntaxKind::BacktickToken) {
            let _ = self.parser.create_missing_node(
                SyntaxKind::BacktickToken,
                false,
                Some(&gen::_0_expected),
                &[&super::token_to_string(SyntaxKind::BacktickToken)],
            );
        }
        if is_bracketed {
            self.skip_whitespace();
            if self.token() == SyntaxKind::EqualsToken {
                // parseOptionalToken uses the ordinary scanner.
                self.parser.next_token();
                self.parser.parse_expression();
            }
            if self.token() == SyntaxKind::CloseBracketToken {
                // tsc deliberately uses parseExpected (the ordinary scanner)
                // here, rather than parseExpectedTokenJSDoc. Besides consuming
                // the bracket, this preserves the start of any trailing trivia
                // as the full start of the next token.
                self.parser.next_token();
            } else {
                self.parser.parse_error_at_current_token(
                    &gen::_0_expected,
                    &[&super::token_to_string(SyntaxKind::CloseBracketToken)],
                );
            }
        }
        (name, is_bracketed)
    }

    fn look_ahead_jsdoc_link_prefix(&mut self) -> bool {
        let state = self.parser.scanner.save();
        let diagnostics = self.parser.parse_diagnostics.len();
        let parse_error = self.parser.parse_error_before_next_finished_node;
        let result = self.parse_jsdoc_link_prefix().is_some();
        self.parser.scanner.restore(state);
        self.parser.parse_diagnostics.truncate(diagnostics);
        self.parser.parse_error_before_next_finished_node = parse_error;
        result
    }

    fn parse_parameter_or_property_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        target: u8,
        indent: usize,
    ) -> NodeId {
        let mut type_expression = self.try_parse_type_expression();
        let mut is_name_first = type_expression.is_none();
        self.skip_whitespace_or_asterisk();
        let (name, is_bracketed) = self.parse_bracket_name_in_property_and_param_tag();
        let indent_text = self.skip_whitespace_or_asterisk();
        if is_name_first && !self.look_ahead_jsdoc_link_prefix() {
            type_expression = self.try_parse_type_expression();
        }
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text);
        if let Some(nested) = self.parse_nested_type_literal(type_expression, name, target, indent)
        {
            type_expression = Some(nested);
            is_name_first = true;
        }
        let data = if target == TARGET_PROPERTY {
            NodeData::JSDocPropertyTag(JSDocPropertyTagData {
                tag_name: Some(tag_name),
                comment,
                name: Some(name),
                type_expression,
                is_name_first,
                is_bracketed,
            })
        } else {
            NodeData::JSDocParameterTag(JSDocParameterTagData {
                tag_name: Some(tag_name),
                comment,
                name: Some(name),
                type_expression,
                is_name_first,
                is_bracketed,
            })
        };
        self.finish_current(data, start)
    }

    fn jsdoc_type_expression_type(&self, expression: NodeId) -> Option<NodeId> {
        match &self.parser.arena.node(expression).data {
            NodeData::JSDocTypeExpression(data) => data.r#type,
            _ => None,
        }
    }

    fn is_object_or_object_array_type_reference(&self, node: NodeId) -> bool {
        match &self.parser.arena.node(node).data {
            NodeData::ArrayType(data) => data
                .element_type
                .is_some_and(|node| self.is_object_or_object_array_type_reference(node)),
            NodeData::TypeReference(data) => {
                data.type_arguments.is_none()
                    && data
                        .type_name
                        .and_then(|name| self.identifier_text(name))
                        .is_some_and(|name| name == "Object")
            }
            _ => self.parser.arena.node(node).kind == SyntaxKind::ObjectKeyword,
        }
    }

    fn parse_nested_type_literal(
        &mut self,
        type_expression: Option<NodeId>,
        name: NodeId,
        target: u8,
        indent: usize,
    ) -> Option<NodeId> {
        let expression = type_expression?;
        let r#type = self.jsdoc_type_expression_type(expression)?;
        if !self.is_object_or_object_array_type_reference(r#type) {
            return None;
        }
        let pos = self.node_pos();
        let mut children = Vec::new();
        while let Some(child) =
            self.parse_child_parameter_or_property_tag(target, indent, Some(name))
        {
            match self.parser.arena.node(child).kind {
                SyntaxKind::JSDocParameterTag | SyntaxKind::JSDocPropertyTag => {
                    children.push(child)
                }
                SyntaxKind::JSDocTemplateTag => self.parser.parse_error_at_range(
                    self.tag_name_of(child).unwrap_or(child),
                    &gen::A_JSDoc_template_tag_may_not_follow_a_typedef_callback_or_overload_tag,
                    &[],
                ),
                _ => {}
            }
        }
        if children.is_empty() {
            return None;
        }
        let end = self.node_pos();
        let array = self.parser.arena.alloc_synthetic_array(children);
        let literal = self.finish(
            NodeData::JSDocTypeLiteral(JSDocTypeLiteralData {
                js_doc_property_tags: Some(array),
                is_array_type: self.parser.arena.node(r#type).kind == SyntaxKind::ArrayType,
            }),
            pos,
            end,
        );
        Some(self.finish(
            NodeData::JSDocTypeExpression(JSDocTypeExpressionData {
                r#type: Some(literal),
            }),
            pos,
            end,
        ))
    }

    fn tag_name_of(&self, node: NodeId) -> Option<NodeId> {
        match &self.parser.arena.node(node).data {
            NodeData::JSDocParameterTag(data) => data.tag_name,
            NodeData::JSDocPropertyTag(data) => data.tag_name,
            NodeData::JSDocTemplateTag(data) => data.tag_name,
            NodeData::JSDocThisTag(data) => data.tag_name,
            NodeData::JSDocTypeTag(data) => data.tag_name,
            NodeData::JSDocReturnTag(data) => data.tag_name,
            _ => None,
        }
    }

    fn entity_names_equal(&self, left: NodeId, right: NodeId) -> bool {
        match (
            &self.parser.arena.node(left).data,
            &self.parser.arena.node(right).data,
        ) {
            (NodeData::Identifier(left), NodeData::Identifier(right)) => {
                left.escaped_text == right.escaped_text
            }
            (NodeData::QualifiedName(left), NodeData::QualifiedName(right)) => {
                left.left
                    .zip(right.left)
                    .is_some_and(|(left, right)| self.entity_names_equal(left, right))
                    && left
                        .right
                        .zip(right.right)
                        .is_some_and(|(left, right)| self.entity_names_equal(left, right))
            }
            _ => false,
        }
    }

    fn parse_child_parameter_or_property_tag(
        &mut self,
        target: u8,
        indent: usize,
        parent_name: Option<NodeId>,
    ) -> Option<NodeId> {
        let scanner_state = self.parser.scanner.save();
        let diagnostics_len = self.parser.parse_diagnostics.len();
        let parse_error = self.parser.parse_error_before_next_finished_node;
        let mut can_parse_tag = true;
        let mut seen_asterisk = false;
        let child = loop {
            match self.next_token_jsdoc() {
                SyntaxKind::AtToken if can_parse_tag => {
                    let start = self.node_pos();
                    self.next_token_jsdoc();
                    let tag_name = self.parse_identifier_name(None);
                    let name_text = self
                        .identifier_text(tag_name)
                        .unwrap_or_default()
                        .to_owned();
                    let indent_text = self.skip_whitespace_or_asterisk();
                    let child = match name_text.as_str() {
                        "type" if target == TARGET_PROPERTY => {
                            Some(self.parse_type_tag(start, tag_name, None))
                        }
                        "prop" | "property" if target & TARGET_PROPERTY != 0 => Some(
                            self.parse_parameter_or_property_tag(start, tag_name, target, indent),
                        ),
                        "arg" | "argument" | "param"
                            if target & (TARGET_PARAMETER | TARGET_CALLBACK_PARAMETER) != 0 =>
                        {
                            Some(
                                self.parse_parameter_or_property_tag(
                                    start, tag_name, target, indent,
                                ),
                            )
                        }
                        "template" => {
                            Some(self.parse_template_tag(start, tag_name, indent, indent_text))
                        }
                        "this" => Some(self.parse_this_tag(start, tag_name, indent, indent_text)),
                        _ => None,
                    };
                    break child;
                }
                SyntaxKind::AtToken => {
                    seen_asterisk = false;
                }
                SyntaxKind::NewLineTrivia => {
                    can_parse_tag = true;
                    seen_asterisk = false;
                }
                SyntaxKind::AsteriskToken => {
                    if seen_asterisk {
                        can_parse_tag = false;
                    }
                    seen_asterisk = true;
                }
                SyntaxKind::Identifier => can_parse_tag = false,
                SyntaxKind::EndOfFileToken => break None,
                _ => {}
            }
        };
        let Some(child) = child else {
            self.parser.scanner.restore(scanner_state);
            self.parser.parse_diagnostics.truncate(diagnostics_len);
            self.parser.parse_error_before_next_finished_node = parse_error;
            return None;
        };
        if let Some(parent_name) = parent_name {
            if matches!(
                self.parser.arena.node(child).kind,
                SyntaxKind::JSDocParameterTag | SyntaxKind::JSDocPropertyTag
            ) {
                let child_name = match &self.parser.arena.node(child).data {
                    NodeData::JSDocParameterTag(data) => data.name,
                    NodeData::JSDocPropertyTag(data) => data.name,
                    _ => None,
                };
                if let Some(child_name) = child_name {
                    let valid_nested_name = match &self.parser.arena.node(child_name).data {
                        NodeData::QualifiedName(data) => data
                            .left
                            .is_some_and(|left| self.entity_names_equal(parent_name, left)),
                        _ => false,
                    };
                    if !valid_nested_name {
                        self.parser.scanner.restore(scanner_state);
                        self.parser.parse_diagnostics.truncate(diagnostics_len);
                        self.parser.parse_error_before_next_finished_node = parse_error;
                        return None;
                    }
                }
            }
        }
        Some(child)
    }

    fn has_root_tag_kind(&self, kind: SyntaxKind) -> bool {
        self.tags
            .iter()
            .any(|tag| self.parser.arena.node(*tag).kind == kind)
    }

    fn parse_return_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        indent: usize,
        indent_text: String,
    ) -> NodeId {
        if self.has_root_tag_kind(SyntaxKind::JSDocReturnTag) {
            let name = self
                .identifier_text(tag_name)
                .unwrap_or_default()
                .to_owned();
            self.parser.parse_error_at(
                self.parser.arena.node(tag_name).pos as usize,
                self.token_start(),
                &gen::_0_tag_already_specified,
                &[&name],
            );
        }
        let type_expression = self.try_parse_type_expression();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text);
        self.finish_current(
            NodeData::JSDocReturnTag(JSDocReturnTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_type_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        trailing: Option<(usize, String)>,
    ) -> NodeId {
        if self.has_root_tag_kind(SyntaxKind::JSDocTypeTag) {
            let name = self
                .identifier_text(tag_name)
                .unwrap_or_default()
                .to_owned();
            self.parser.parse_error_at(
                self.parser.arena.node(tag_name).pos as usize,
                self.token_start(),
                &gen::_0_tag_already_specified,
                &[&name],
            );
        }
        let type_expression = Some(self.parse_jsdoc_type_expression(true));
        let comment = trailing.and_then(|(indent, indent_text)| {
            self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text)
        });
        self.finish_current(
            NodeData::JSDocTypeTag(JSDocTypeTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_this_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let type_expression = Some(self.parse_jsdoc_type_expression(true));
        self.skip_whitespace();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocThisTag(JSDocThisTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_enum_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let type_expression = Some(self.parse_jsdoc_type_expression(true));
        self.skip_whitespace();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocEnumTag(JSDocEnumTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_satisfies_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let type_expression = Some(self.parse_jsdoc_type_expression(false));
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocSatisfiesTag(JSDocSatisfiesTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_throws_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let type_expression = self.try_parse_type_expression();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocThrowsTag(JSDocThrowsTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression,
            }),
            start,
        )
    }

    fn parse_see_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let is_markdown_or_link =
            self.token() == SyntaxKind::OpenBracketToken || self.look_ahead_jsdoc_link_prefix();
        let name = (!is_markdown_or_link).then(|| self.parse_jsdoc_name_reference());
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocSeeTag(JSDocSeeTagData {
                tag_name: Some(tag_name),
                comment,
                name,
            }),
            start,
        )
    }

    fn parse_author_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let comment_start = self.node_pos();
        let mut text = String::new();
        let mut in_email = false;
        while !matches!(
            self.token(),
            SyntaxKind::EndOfFileToken | SyntaxKind::NewLineTrivia
        ) {
            match self.token() {
                SyntaxKind::LessThanToken => in_email = true,
                SyntaxKind::AtToken if !in_email => break,
                SyntaxKind::GreaterThanToken if in_email => {
                    text.push_str(&self.token_text());
                    let end = self.token_end();
                    self.parser.scanner.reset_token_state(end);
                    break;
                }
                _ => {}
            }
            text.push_str(&self.token_text());
            self.next_token_jsdoc();
        }
        let mut text_end = self.parser.scanner.full_start_pos();
        let trailing = self.parse_trailing_tag_comments(start, text_end, margin, indent_text);
        let comment = match trailing {
            Some(JSDocComment::Text(trailing)) => {
                Some(JSDocComment::Text(format!("{text}{trailing}")))
            }
            Some(JSDocComment::Nodes(nodes)) => {
                let mut all = vec![self.finish(
                    NodeData::JSDocText(JSDocTextData { text }),
                    comment_start,
                    text_end,
                )];
                all.extend(self.parser.arena.node_array(nodes).nodes.iter().copied());
                let array_end = self.node_pos();
                Some(JSDocComment::Nodes(self.alloc_array(
                    all,
                    comment_start,
                    array_end,
                )))
            }
            None => {
                text_end = self.node_pos();
                let text = self.finish(
                    NodeData::JSDocText(JSDocTextData { text }),
                    comment_start,
                    text_end,
                );
                let array_end = self.node_pos();
                Some(JSDocComment::Nodes(self.alloc_array(
                    vec![text],
                    comment_start,
                    array_end,
                )))
            }
        };
        self.finish_current(
            NodeData::JSDocAuthorTag(JSDocAuthorTagData {
                tag_name: Some(tag_name),
                comment,
            }),
            start,
        )
    }

    fn parse_property_access_entity_name_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let mut expression = self.parse_identifier_name(None);
        while self.parser.parse_optional(SyntaxKind::DotToken) {
            let name = self.parse_identifier_name(None);
            expression = self.finish_current(
                NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                    expression: Some(expression),
                    question_dot_token: None,
                    name: Some(name),
                }),
                pos,
            );
        }
        expression
    }

    fn parse_expression_with_type_arguments_for_augments(&mut self) -> NodeId {
        let used_brace = self.parser.parse_optional(SyntaxKind::OpenBraceToken);
        let pos = self.node_pos();
        let expression = self.parse_property_access_entity_name_expression();
        self.parser.scanner.set_skip_jsdoc_leading_asterisks(true);
        let type_arguments = self.parser.try_parse_type_arguments();
        self.parser.scanner.set_skip_jsdoc_leading_asterisks(false);
        let result = self.finish_current(
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                expression: Some(expression),
                type_arguments,
            }),
            pos,
        );
        if used_brace {
            self.skip_whitespace();
            if self.token() == SyntaxKind::CloseBraceToken {
                // parseExpected in tsc resumes the ordinary scanner here.
                self.parser.next_token();
            } else {
                self.parser.parse_error_at_current_token(
                    &gen::_0_expected,
                    &[&super::token_to_string(SyntaxKind::CloseBraceToken)],
                );
            }
        }
        result
    }

    fn parse_augments_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let class = self.parse_expression_with_type_arguments_for_augments();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocAugmentsTag(JSDocAugmentsTagData {
                tag_name: Some(tag_name),
                comment,
                class: Some(class),
            }),
            start,
        )
    }

    fn parse_implements_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let class = self.parse_expression_with_type_arguments_for_augments();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocImplementsTag(JSDocImplementsTagData {
                tag_name: Some(tag_name),
                comment,
                class: Some(class),
            }),
            start,
        )
    }

    fn parse_jsdoc_type_name_with_namespace(&mut self, nested: bool) -> Option<NodeId> {
        let start = self.token_start();
        if !token_is_identifier_or_keyword(self.token()) {
            return None;
        }
        let name = self.parse_identifier_name(None);
        if self.parser.parse_optional(SyntaxKind::DotToken) {
            let body = self.parse_jsdoc_type_name_with_namespace(true);
            let flags = if nested {
                NodeFlags::NESTED_NAMESPACE
            } else {
                NodeFlags::NONE
            };
            return Some(self.finish_with_flags(
                NodeData::ModuleDeclaration(ModuleDeclarationData {
                    modifiers: None,
                    name: Some(name),
                    body,
                }),
                start,
                self.node_pos(),
                flags,
            ));
        }
        if nested {
            self.parser.arena.node_mut(name).flags |=
                NodeFlags::IDENTIFIER_IS_IN_JS_DOC_NAMESPACE.bits();
        }
        Some(name)
    }

    fn jsdoc_alias_name(&self, full_name: Option<NodeId>) -> Option<NodeId> {
        let mut current = full_name?;
        loop {
            match &self.parser.arena.node(current).data {
                NodeData::ModuleDeclaration(data) => current = data.body?,
                NodeData::Identifier(_) => return Some(current),
                _ => return None,
            }
        }
    }

    fn parse_typedef_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        indent: usize,
        indent_text: String,
    ) -> NodeId {
        let mut type_expression = self.try_parse_type_expression();
        self.skip_whitespace_or_asterisk();
        let full_name = self.parse_jsdoc_type_name_with_namespace(false);
        let name = self.jsdoc_alias_name(full_name);
        self.skip_whitespace();
        let mut comment = self.parse_tag_comments(indent, None);
        let mut explicit_end = None;

        let may_have_children = type_expression
            .and_then(|expression| self.jsdoc_type_expression_type(expression))
            .is_none_or(|r#type| self.is_object_or_object_array_type_reference(r#type));
        if may_have_children {
            let mut child_type_tag: Option<NodeId> = None;
            let mut property_tags = Vec::new();
            let mut has_children = false;
            while let Some(child) =
                self.parse_child_parameter_or_property_tag(TARGET_PROPERTY, indent, None)
            {
                if self.parser.arena.node(child).kind == SyntaxKind::JSDocTemplateTag {
                    break;
                }
                has_children = true;
                if self.parser.arena.node(child).kind == SyntaxKind::JSDocTypeTag {
                    if child_type_tag.is_some() {
                        if let Some(index) = self.parser.parse_error_at_current_token_with_index(
                            &gen::A_JSDoc_typedef_comment_may_not_contain_multiple_type_tags,
                            &[],
                        ) {
                            self.parser.parse_diagnostics[index]
                                .related
                                .push(RelatedInfo {
                                    file_name: Some(self.parser.file_name.clone()),
                                    start: Some(0),
                                    length: Some(0),
                                    message: tsc_diagnostics::MessageChain::new(
                                        &gen::The_tag_was_first_specified_here,
                                        &[],
                                    ),
                                });
                        }
                        break;
                    }
                    child_type_tag = Some(child);
                } else {
                    property_tags.push(child);
                }
            }
            if has_children {
                let original_type = type_expression
                    .and_then(|expression| self.jsdoc_type_expression_type(expression));
                let is_array_type = original_type.is_some_and(|r#type| {
                    self.parser.arena.node(r#type).kind == SyntaxKind::ArrayType
                });
                let replacement = child_type_tag
                    .and_then(|tag| match &self.parser.arena.node(tag).data {
                        NodeData::JSDocTypeTag(data) => data.type_expression,
                        _ => None,
                    })
                    .filter(|expression| {
                        self.jsdoc_type_expression_type(*expression)
                            .is_some_and(|r#type| {
                                !self.is_object_or_object_array_type_reference(r#type)
                            })
                    });
                type_expression = if replacement.is_some() {
                    replacement
                } else {
                    let end = self.node_pos();
                    let properties = if property_tags.is_empty() {
                        None
                    } else {
                        Some(self.parser.arena.alloc_synthetic_array(property_tags))
                    };
                    Some(self.finish(
                        NodeData::JSDocTypeLiteral(JSDocTypeLiteralData {
                            js_doc_property_tags: properties,
                            is_array_type,
                        }),
                        start,
                        end,
                    ))
                };
                explicit_end =
                    type_expression.map(|node| self.parser.arena.node(node).end as usize);
            }
        }

        let end = if explicit_end.is_some_and(|end| end != 0) || comment.is_some() {
            self.node_pos()
        } else {
            full_name
                .or(type_expression)
                .unwrap_or(tag_name)
                .pipe(|node| self.parser.arena.node(node).end as usize)
        };
        if comment.is_none() {
            comment = self.parse_trailing_tag_comments(start, end, indent, indent_text);
        }
        self.finish(
            NodeData::JSDocTypedefTag(JSDocTypedefTagData {
                tag_name: Some(tag_name),
                comment,
                name,
                full_name,
                type_expression,
            }),
            start,
            end,
        )
    }

    fn parse_jsdoc_signature(&mut self, start: usize, indent: usize) -> NodeId {
        let parameters_pos = self.node_pos();
        let mut parameters = Vec::new();
        while let Some(child) =
            self.parse_child_parameter_or_property_tag(TARGET_CALLBACK_PARAMETER, indent, None)
        {
            if self.parser.arena.node(child).kind == SyntaxKind::JSDocTemplateTag {
                self.parser.parse_error_at_range(
                    self.tag_name_of(child).unwrap_or(child),
                    &gen::A_JSDoc_template_tag_may_not_follow_a_typedef_callback_or_overload_tag,
                    &[],
                );
                break;
            }
            parameters.push(child);
        }
        let parameters = self.alloc_array(parameters, parameters_pos, self.node_pos());
        let state = self.parser.scanner.save();
        let diagnostics = self.parser.parse_diagnostics.len();
        let parse_error = self.parser.parse_error_before_next_finished_node;
        if self.token() == SyntaxKind::Unknown {
            self.next_token_jsdoc();
        }
        let return_tag = if self.token() == SyntaxKind::AtToken {
            let tag = self.parse_tag(indent);
            (self.parser.arena.node(tag).kind == SyntaxKind::JSDocReturnTag).then_some(tag)
        } else {
            None
        };
        if return_tag.is_none() {
            self.parser.scanner.restore(state);
            self.parser.parse_diagnostics.truncate(diagnostics);
            self.parser.parse_error_before_next_finished_node = parse_error;
        }
        self.finish_current(
            NodeData::JSDocSignature(JSDocSignatureData {
                type_parameters: None,
                parameters: Some(parameters),
                r#type: return_tag,
            }),
            start,
        )
    }

    fn parse_callback_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        indent: usize,
        indent_text: String,
    ) -> NodeId {
        let full_name = self.parse_jsdoc_type_name_with_namespace(false);
        let name = self.jsdoc_alias_name(full_name);
        self.skip_whitespace();
        let mut comment = self.parse_tag_comments(indent, None);
        let type_expression = self.parse_jsdoc_signature(start, indent);
        if comment.is_none() {
            comment = self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text);
        }
        let end = if comment.is_some() {
            self.node_pos()
        } else {
            self.parser.arena.node(type_expression).end as usize
        };
        self.finish(
            NodeData::JSDocCallbackTag(JSDocCallbackTagData {
                tag_name: Some(tag_name),
                comment,
                name,
                full_name,
                type_expression: Some(type_expression),
            }),
            start,
            end,
        )
    }

    fn parse_overload_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        indent: usize,
        indent_text: String,
    ) -> NodeId {
        self.skip_whitespace();
        let mut comment = self.parse_tag_comments(indent, None);
        let type_expression = self.parse_jsdoc_signature(start, indent);
        if comment.is_none() {
            comment = self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text);
        }
        let end = if comment.is_some() {
            self.node_pos()
        } else {
            self.parser.arena.node(type_expression).end as usize
        };
        self.finish(
            NodeData::JSDocOverloadTag(JSDocOverloadTagData {
                tag_name: Some(tag_name),
                comment,
                type_expression: Some(type_expression),
            }),
            start,
            end,
        )
    }

    /// tsc-port: parseTemplateTagTypeParameter @6.0.3
    /// tsc-hash: 82c21472fa8bd61beebc2c4a8bd5db6d41831197899023a26eee489f11c8f3d2
    /// tsc-span: _tsc.js:35712-35742
    /// d2: d2:624f6ae3e504b80c275345b2fe8f9e150962ade1f6a4932a070a86909f5d8051
    fn parse_template_type_parameter(&mut self) -> Option<NodeId> {
        let pos = self.node_pos();
        let is_bracketed = self.parse_optional(SyntaxKind::OpenBracketToken);
        if is_bracketed {
            self.skip_whitespace();
        }
        let modifiers = self.parser.parse_modifiers(false, true, false);
        let has_name = token_is_identifier_or_keyword(self.token());
        let name = self.parse_identifier_name(Some(
            &gen::Unexpected_token_A_type_parameter_name_was_expected_without_curly_braces,
        ));
        let default_type = if is_bracketed {
            self.skip_whitespace();
            self.parser.parse_expected(SyntaxKind::EqualsToken, None);
            let r#type = self.parser.parse_jsdoc_type();
            self.parser
                .parse_expected(SyntaxKind::CloseBracketToken, None);
            Some(r#type)
        } else {
            None
        };
        if !has_name {
            return None;
        }
        Some(self.finish_current(
            NodeData::TypeParameter(TypeParameterData {
                modifiers,
                name: Some(name),
                constraint: None,
                r#default: default_type,
                expression: None,
            }),
            pos,
        ))
    }

    fn parse_template_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        indent: usize,
        indent_text: String,
    ) -> NodeId {
        let constraint = (self.token() == SyntaxKind::OpenBraceToken)
            .then(|| self.parse_jsdoc_type_expression(false));
        let parameters_pos = self.node_pos();
        let mut type_parameters = Vec::new();
        loop {
            self.skip_whitespace();
            if let Some(parameter) = self.parse_template_type_parameter() {
                type_parameters.push(parameter);
            }
            self.skip_whitespace_or_asterisk();
            if !self.parse_optional(SyntaxKind::CommaToken) {
                break;
            }
        }
        let type_parameters = self.alloc_array(type_parameters, parameters_pos, self.node_pos());
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), indent, indent_text);
        self.finish_current(
            NodeData::JSDocTemplateTag(JSDocTemplateTagData {
                tag_name: Some(tag_name),
                comment,
                constraint,
                type_parameters: Some(type_parameters),
            }),
            start,
        )
    }

    fn parse_import_tag(
        &mut self,
        start: usize,
        tag_name: NodeId,
        margin: usize,
        indent_text: String,
    ) -> NodeId {
        let after_import_tag_pos = self.parser.scanner.full_start_pos();
        let identifier = if self.parser.is_identifier() {
            Some(self.parser.parse_identifier())
        } else {
            None
        };
        let import_clause = self.parser.try_parse_import_clause(
            identifier,
            after_import_tag_pos,
            Some(SyntaxKind::TypeKeyword),
            true,
        );
        let module_specifier = self.parser.parse_module_specifier();
        let attributes = self.parser.try_parse_import_attributes();
        let comment = self.parse_trailing_tag_comments(start, self.node_pos(), margin, indent_text);
        self.finish_current(
            NodeData::JSDocImportTag(JSDocImportTagData {
                tag_name: Some(tag_name),
                comment,
                import_clause,
                module_specifier: Some(module_specifier),
                attributes,
            }),
            start,
        )
    }
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}

pub(super) fn parse_jsdoc_comment(
    parser: &mut Parser<'_>,
    start: usize,
    end: usize,
    host_flags: NodeFlags,
) -> ParsedJSDoc {
    let scanner_state = parser.scanner.save();
    let diagnostics_start = parser.parse_diagnostics.len();
    let saved_parse_error = parser.parse_error_before_next_finished_node;
    let saved_context = parser.context_flags;
    let saved_parsing_context = parser.parsing_context;

    parser.context_flags = (host_flags & NodeFlags::CONTEXT_FLAGS) | NodeFlags::JS_DOC;
    parser.parsing_context |= super::ParsingContext::JSDocComment.bit();
    parser
        .scanner
        .reset_range(start.saturating_add(3), end.saturating_sub(2));
    let mut worker = JSDocParser::new(parser);
    let node = worker.parse_comment_worker(start, end);
    let deprecated = worker.has_deprecated_tag;
    drop(worker);

    if parser.javascript_file {
        parser.js_doc_diagnostics.extend(
            parser.parse_diagnostics[diagnostics_start..]
                .iter()
                .cloned(),
        );
    }
    parser.parse_diagnostics.truncate(diagnostics_start);
    parser.parse_error_before_next_finished_node = saved_parse_error;
    parser.context_flags = saved_context;
    parser.parsing_context = saved_parsing_context;
    parser.scanner.restore(scanner_state);
    ParsedJSDoc { node, deprecated }
}
