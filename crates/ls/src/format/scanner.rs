//! tsgo `format/scanner.go`: the formatting scanner reads the tokens of a
//! span with their leading and trailing trivia, and reads a token again
//! the way its node needs (`>>`, a regular expression, a template, JSX).

use tsc_syntax::{SyntaxKind, TokenScanner};

use super::file::{is_keyword_kind, is_token_kind, is_trivia, File, Node};

/// tsgo `TextRangeWithKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextRangeWithKind {
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) kind: SyntaxKind,
}

impl TextRangeWithKind {
    /// tsgo `NewTextRangeWithKind(0, 0, 0)`.
    pub(crate) const EMPTY: Self = Self::new(0, 0, SyntaxKind::Unknown);

    pub(crate) const fn new(pos: usize, end: usize, kind: SyntaxKind) -> Self {
        Self { pos, end, kind }
    }

    pub(crate) fn contained_by(self, pos: usize, end: usize) -> bool {
        pos <= self.pos && self.end <= end
    }
}

/// tsgo `tokenInfo`.
#[derive(Clone, Debug)]
pub(crate) struct TokenInfo {
    pub(crate) leading_trivia: Vec<TextRangeWithKind>,
    pub(crate) token: TextRangeWithKind,
    pub(crate) trailing_trivia: Vec<TextRangeWithKind>,
}

/// tsgo `scanAction`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanAction {
    Scan,
    RescanGreaterThanToken,
    RescanSlashToken,
    RescanTemplateToken,
    RescanJsxIdentifier,
    RescanJsxText,
    RescanJsxAttributeValue,
}

/// tsgo `formattingScanner`.
pub(crate) struct FormattingScanner<'a> {
    s: TokenScanner<'a>,
    file: &'a File<'a>,
    start_pos: usize,
    end_pos: usize,
    saved_pos: usize,
    last_token_info: Option<TokenInfo>,
    last_scan_action: ScanAction,
    leading_trivia: Vec<TextRangeWithKind>,
    trailing_trivia: Vec<TextRangeWithKind>,
    was_new_line: bool,
}

impl<'a> FormattingScanner<'a> {
    /// tsgo `newFormattingScanner`'s scanner.
    pub(crate) fn new(file: &'a File<'a>, start_pos: usize, end_pos: usize) -> Self {
        Self {
            s: TokenScanner::with_trivia(file.source, start_pos),
            file,
            start_pos,
            end_pos,
            saved_pos: 0,
            last_token_info: None,
            last_scan_action: ScanAction::Scan,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
            was_new_line: true,
        }
    }

    pub(crate) fn advance(&mut self) {
        self.last_token_info = None;
        let is_started = self.s.token_full_start() != self.start_pos;

        if is_started {
            self.was_new_line = self
                .trailing_trivia
                .last()
                .is_some_and(|trivia| trivia.kind == SyntaxKind::NewLineTrivia);
        } else {
            self.s.scan();
        }

        self.leading_trivia.clear();
        self.trailing_trivia.clear();

        let mut pos = self.s.token_full_start();

        // Read leading trivia and token
        while pos < self.end_pos {
            let t = self.s.token();
            if !is_trivia(t) {
                break;
            }

            // consume leading trivia
            self.s.scan();
            let item = TextRangeWithKind::new(pos, self.s.token_full_start(), t);

            pos = self.s.token_full_start();

            self.leading_trivia.push(item);
        }

        self.saved_pos = self.s.token_full_start();
    }

    pub(crate) fn read_token_info(&mut self, n: Node) -> TokenInfo {
        debug_assert!(self.is_on_token());

        // normally scanner returns the smallest available token
        // check the kind of context node to determine if scanner should have more greedy behavior and consume more text.
        let expected_scan_action = if should_rescan_greater_than_token(self.file, n) {
            ScanAction::RescanGreaterThanToken
        } else if should_rescan_slash_token(self.file, n) {
            ScanAction::RescanSlashToken
        } else if should_rescan_template_token(self.file, n) {
            ScanAction::RescanTemplateToken
        } else if should_rescan_jsx_identifier(self.file, n) {
            ScanAction::RescanJsxIdentifier
        } else if self.should_rescan_jsx_text(n) {
            ScanAction::RescanJsxText
        } else if should_rescan_jsx_attribute_value(self.file, n) {
            ScanAction::RescanJsxAttributeValue
        } else {
            ScanAction::Scan
        };

        if let Some(last) = &self.last_token_info {
            if expected_scan_action == self.last_scan_action {
                // readTokenInfo was called before with the same expected scan action.
                // No need to re-scan text, return existing 'lastTokenInfo'
                // it is ok to call fixTokenKind here since it does not affect
                // what portion of text is consumed. In contrast rescanning can change it,
                // i.e. for '>=' when originally scanner eats just one character
                // and rescanning forces it to consume more.
                let fixed = fix_token_kind(self.file, last.clone(), n);
                self.last_token_info = Some(fixed.clone());
                return fixed;
            }
        }

        if self.s.token_full_start() != self.saved_pos {
            // readTokenInfo was called before but scan action differs - rescan text
            self.s.reset_token_state(self.saved_pos);
            self.s.scan();
        }

        let mut current_token = self.get_next_token(n, expected_scan_action);

        let token =
            TextRangeWithKind::new(self.s.token_full_start(), self.s.token_end(), current_token);

        // consume trailing trivia
        self.trailing_trivia.clear();
        while self.s.token_full_start() < self.end_pos {
            current_token = self.s.scan();
            if !is_trivia(current_token) {
                break;
            }
            let trivia = TextRangeWithKind::new(
                self.s.token_full_start(),
                self.s.token_end(),
                current_token,
            );

            self.trailing_trivia.push(trivia);

            if current_token == SyntaxKind::NewLineTrivia {
                // move past new line
                self.s.scan();
                break;
            }
        }

        let info = fix_token_kind(
            self.file,
            TokenInfo {
                leading_trivia: self.leading_trivia.clone(),
                token,
                trailing_trivia: self.trailing_trivia.clone(),
            },
            n,
        );
        self.last_token_info = Some(info.clone());
        info
    }

    fn get_next_token(&mut self, n: Node, expected_scan_action: ScanAction) -> SyntaxKind {
        let token = self.s.token();
        self.last_scan_action = ScanAction::Scan;
        match expected_scan_action {
            ScanAction::RescanGreaterThanToken => {
                if token == SyntaxKind::GreaterThanToken {
                    self.last_scan_action = ScanAction::RescanGreaterThanToken;
                    let new_token = self.s.re_scan_greater_token();
                    debug_assert_eq!(self.file.kind(n), new_token);
                    return new_token;
                }
            }
            ScanAction::RescanSlashToken => {
                if starts_with_slash_token(token) {
                    self.last_scan_action = ScanAction::RescanSlashToken;
                    let new_token = self.s.re_scan_slash_token();
                    debug_assert_eq!(self.file.kind(n), new_token);
                    return new_token;
                }
            }
            ScanAction::RescanTemplateToken => {
                if token == SyntaxKind::CloseBraceToken {
                    self.last_scan_action = ScanAction::RescanTemplateToken;
                    return self.s.re_scan_template_token(false);
                }
            }
            ScanAction::RescanJsxIdentifier => {
                self.last_scan_action = ScanAction::RescanJsxIdentifier;
                return self.s.scan_jsx_identifier();
            }
            ScanAction::RescanJsxText => {
                self.last_scan_action = ScanAction::RescanJsxText;
                return self.s.re_scan_jsx_token(false);
            }
            ScanAction::RescanJsxAttributeValue => {
                self.last_scan_action = ScanAction::RescanJsxAttributeValue;
                return self.s.re_scan_jsx_attribute_value();
            }
            // no rescan needed; the token was already produced by the normal scan
            ScanAction::Scan => {}
        }
        token
    }

    pub(crate) fn read_eof_token_range(&self) -> TextRangeWithKind {
        debug_assert!(self.is_on_eof());
        TextRangeWithKind::new(
            self.s.token_full_start(),
            self.s.token_end(),
            SyntaxKind::EndOfFileToken,
        )
    }

    fn current_kind(&self) -> SyntaxKind {
        self.last_token_info
            .as_ref()
            .map_or_else(|| self.s.token(), |info| info.token.kind)
    }

    pub(crate) fn is_on_token(&self) -> bool {
        let current = self.current_kind();
        current != SyntaxKind::EndOfFileToken && !is_trivia(current)
    }

    pub(crate) fn is_on_eof(&self) -> bool {
        self.current_kind() == SyntaxKind::EndOfFileToken
    }

    pub(crate) fn skip_to_end_of(&mut self, end: usize) {
        self.skip_to(end);
    }

    pub(crate) fn skip_to_start_of(&mut self, pos: usize) {
        self.skip_to(pos);
    }

    fn skip_to(&mut self, pos: usize) {
        self.s.reset_token_state(pos);
        self.saved_pos = self.s.token_full_start();
        self.last_scan_action = ScanAction::Scan;
        self.last_token_info = None;
        self.was_new_line = false;
        self.leading_trivia.clear();
        self.trailing_trivia.clear();
    }

    pub(crate) fn get_current_leading_trivia(&self) -> &[TextRangeWithKind] {
        &self.leading_trivia
    }

    pub(crate) fn last_trailing_trivia_was_new_line(&self) -> bool {
        self.was_new_line
    }

    pub(crate) fn get_token_full_start(&self) -> usize {
        self.last_token_info
            .as_ref()
            .map_or_else(|| self.s.token_full_start(), |info| info.token.pos)
    }

    fn should_rescan_jsx_text(&self, node: Node) -> bool {
        match self.file.kind(node) {
            SyntaxKind::JsxText => true,
            SyntaxKind::JsxElement => self
                .last_token_info
                .as_ref()
                .is_some_and(|info| info.token.kind == SyntaxKind::JsxText),
            _ => false,
        }
    }
}

fn should_rescan_greater_than_token(file: &File<'_>, node: Node) -> bool {
    matches!(
        file.kind(node),
        SyntaxKind::GreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanToken
            | SyntaxKind::GreaterThanGreaterThanToken
    )
}

fn should_rescan_jsx_identifier(file: &File<'_>, node: Node) -> bool {
    let Some(parent) = file.parent(node) else {
        return false;
    };
    let kind = file.kind(node);
    match file.kind(parent) {
        SyntaxKind::JsxAttribute
        | SyntaxKind::JsxOpeningElement
        | SyntaxKind::JsxClosingElement
        | SyntaxKind::JsxSelfClosingElement
        | SyntaxKind::JsxNamespacedName => {
            // May parse an identifier like `module-layout`; that will be scanned as a keyword at first, but we should parse the whole thing to get an identifier.
            is_keyword_kind(kind) || kind == SyntaxKind::Identifier
        }
        SyntaxKind::PropertyAccessExpression => {
            // The leftmost name of a dotted JSX tag name (e.g. `a-b` in `<a-b.c>`) may contain hyphens, so rescan it as a JSX identifier.
            (is_keyword_kind(kind) || kind == SyntaxKind::Identifier)
                && is_leftmost_jsx_tag_name(file, node)
        }
        _ => false,
    }
}

/// Whether `node` starts a JSX tag name: the tag name itself, or the
/// expression a property access of the tag name starts with.
fn is_leftmost_jsx_tag_name(file: &File<'_>, node: Node) -> bool {
    let mut n = node;
    loop {
        let Some(parent) = file.parent(n) else {
            return false;
        };
        if is_jsx_tag_name(file, n) {
            return true;
        }
        if file.kind(parent) == SyntaxKind::PropertyAccessExpression
            && file.expression(parent) == Some(n)
        {
            n = parent;
            continue;
        }
        return false;
    }
}

/// tsgo `IsJsxTagName`.
fn is_jsx_tag_name(file: &File<'_>, node: Node) -> bool {
    file.parent(node).is_some_and(|parent| {
        matches!(
            file.kind(parent),
            SyntaxKind::JsxOpeningElement
                | SyntaxKind::JsxClosingElement
                | SyntaxKind::JsxSelfClosingElement
        ) && file.tag_name(parent) == Some(node)
    })
}

fn should_rescan_slash_token(file: &File<'_>, container: Node) -> bool {
    file.kind(container) == SyntaxKind::RegularExpressionLiteral
}

fn should_rescan_template_token(file: &File<'_>, container: Node) -> bool {
    matches!(
        file.kind(container),
        SyntaxKind::TemplateMiddle | SyntaxKind::TemplateTail
    )
}

fn should_rescan_jsx_attribute_value(file: &File<'_>, node: Node) -> bool {
    file.parent(node).is_some_and(|parent| {
        file.kind(parent) == SyntaxKind::JsxAttribute && file.initializer(parent) == Some(node)
    })
}

fn starts_with_slash_token(t: SyntaxKind) -> bool {
    t == SyntaxKind::SlashToken || t == SyntaxKind::SlashEqualsToken
}

/// tsgo `fixTokenKind`: a token read in a token node takes the node's kind.
fn fix_token_kind(file: &File<'_>, mut token_info: TokenInfo, container: Node) -> TokenInfo {
    let kind = file.kind(container);
    if is_token_kind(kind) && token_info.token.kind != kind {
        token_info.token.kind = kind;
    }
    token_info
}
