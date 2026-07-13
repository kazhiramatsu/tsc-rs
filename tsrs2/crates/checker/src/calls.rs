//! M4 5.7a: call resolution — the core call/new band
//! (m4-57-call-extraction.md; the tagged/import/instanceof tail is
//! 5.7b, the JSX band 5.7c).
//!
//! THE stub M6 swaps is inferTypeArguments (75938) plus the
//! inference-context construction at chooseOverload (76809-76817) and
//! inferSignatureInstantiationForOverloadFailure (76946-76954). The
//! observability rule (extraction doc §0) is the FP=0 wall: the
//! structure resolves for real, but any VALUE the stub would invent
//! escapes (`Unsupported`, class M6-stub) at the moment it would
//! become observable — generic calls without explicit type arguments
//! contain (both success results and arg-relation failures); explicit
//! type arguments, non-generic candidates, and every target-shape/
//! arity/type-argument-arity error band go fully live.
//!
//! Error rendering rides the established T2 curtain: diagnostic CODE
//! and SPAN are tsc-exact, chain tails elide, and a head whose display
//! the slice cannot render unwinds Unsupported (no diagnostic rather
//! than an unfaithful one). Elaboration-eligible argument shapes
//! (object/array literals, arrow bodies — elaborateError 63957) escape
//! on the reporting path because tsc's elaboration would move the
//! code/span into the literal.

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::{gen as diagnostics, Diagnostic, DiagnosticMessage, MessageChain, RelatedInfo};
use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, ElementFlags, ModifierFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

use crate::relate::RelationKind;

use crate::links::LinkSlot;
use crate::operators::OuterExpressionKinds;
use crate::state::{CheckerState, CheckResult2, Signature, SignatureId, Unsupported};
use crate::structural::SignatureKind;

/// The Rust stand-in for tsc's fabricated SyntheticExpression parse
/// nodes (createSyntheticExpression 76289): getEffectiveCallArguments
/// carries these instead of appending arena nodes. `pos`/`end` are the
/// byte range of the originating node (setTextRange semantics);
/// consumers are checkSyntheticExpression (73946), isSpreadArgument,
/// arity, applicability, spans, and the contextual indexOf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectiveArg {
    Node(NodeId),
    Synthetic {
        pos: u32,
        end: u32,
        ty: TypeId,
        is_spread: bool,
        tuple_name_source: Option<NodeId>,
    },
}

/// A resolved diagnostic location (file + UTF-16 start/length) — the
/// applicability walk computes spans without rendering type displays
/// so the 2769 bands stay display-free.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagSpan {
    pub(crate) file_name: String,
    pub(crate) start: u32,
    pub(crate) length: u32,
}

/// One applicability failure: the span of the diagnostic tsc would
/// create, its related rows, and (in Report mode) the fully built
/// head diagnostic.
struct ApplicabilityError {
    span: DiagSpan,
    related: Vec<RelatedInfo>,
    diagnostic: Option<Diagnostic>,
}

/// getSignatureApplicabilityError run modes: Silent is the selection
/// pass (verdicts only, errorOutputContainer.skipLogging semantics);
/// Report builds the head diagnostics (display escapes allowed);
/// Probe computes spans/related only — the overload-failure chains
/// (2769) are display-free heads whose tails elide.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ApplicabilityMode {
    Silent,
    Report,
    Probe,
}

/// resolveCall's closure state: the three error-candidate slots plus
/// the shared argCheckMode (mutated across BOTH chooseOverload passes,
/// 76590/76612/76841).
struct ResolveCallCtx {
    node: NodeId,
    args: Vec<EffectiveArg>,
    type_arguments_array: Option<NodeArrayId>,
    type_argument_nodes: Vec<NodeId>,
    arg_check_mode: CheckMode,
    candidates: Vec<SignatureId>,
    candidates_for_argument_error: Option<Vec<SignatureId>>,
    candidate_for_argument_arity_error: Option<SignatureId>,
    candidate_for_type_argument_error: Option<SignatureId>,
}


/// skipTrivia(text, pos, stopAfterLineBreak=true) followed by tsc's
/// `isLineBreak(text.charCodeAt(result - 1))` (77025-77031): true when
/// a line break separates the callee from its single argument.
fn line_break_precedes_next_token(text: &str, start: usize) -> bool {
    let mut pos = start;
    loop {
        let Some(ch) = text[pos..].chars().next() else {
            return false;
        };
        match ch {
            '\u{000A}' | '\u{000D}' | '\u{2028}' | '\u{2029}' => return true,
            c if c.is_whitespace() => pos += c.len_utf8(),
            '/' => {
                let rest = &text[pos..];
                if rest.starts_with("//") {
                    let mut cursor = pos + 2;
                    while let Some(c) = text[cursor..].chars().next() {
                        if matches!(c, '\u{000A}' | '\u{000D}' | '\u{2028}' | '\u{2029}') {
                            break;
                        }
                        cursor += c.len_utf8();
                    }
                    pos = cursor;
                } else if rest.starts_with("/*") {
                    match rest[2..].find("*/") {
                        Some(offset) => pos += 2 + offset + 2,
                        None => return false,
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

impl<'a> CheckerState<'a> {
    // ---- spans ----

    /// createDiagnosticForNode's location for `node` (error span +
    /// UTF-16 mapping — the diagnostic_for_node twin that returns the
    /// location instead of building the diagnostic).
    pub(crate) fn diag_span_of_node(&self, node: NodeId) -> DiagSpan {
        let source = self.binder.source_of_node(node);
        let (start, end) = node_util::get_error_span_for_node(source, node);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let (start, end) = (to_utf16(start), to_utf16(end));
        DiagSpan {
            file_name: source.file_name.clone(),
            start,
            length: end.saturating_sub(start),
        }
    }

    /// createDiagnosticForNodeArray / createSyntheticExpression range
    /// semantics: start = skipTrivia(text, pos), end taken verbatim.
    fn diag_span_of_byte_range(&self, node_in_file: NodeId, pos: u32, end: u32) -> DiagSpan {
        let source = self.binder.source_of_node(node_in_file);
        let start_byte = tsrs2_syntax::skip_trivia(&source.text, pos as usize);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let (start, end) = (to_utf16(start_byte), to_utf16(end as usize));
        DiagSpan {
            file_name: source.file_name.clone(),
            start,
            length: end.saturating_sub(start),
        }
    }

    fn diagnostic_at_span(&self, span: &DiagSpan, chain: MessageChain) -> Diagnostic {
        Diagnostic::new(
            Some(span.file_name.clone()),
            Some(span.start),
            Some(span.length),
            chain,
        )
    }

    /// tsc-port: getDiagnosticSpanForCallNode @6.0.3
    /// tsc-hash: 82d39cfd61d399c95d6b1cf79bd2ca8680b17feaeb0e9bfe30a2330125180c07
    /// tsc-span: _tsc.js:76376-76380
    ///
    /// tsc-port: getDiagnosticForCallNode @6.0.3
    /// tsc-hash: d0eb85649689dc58b366f87ed7622bb43fb2c6a170f50c03cd57e07d64c84601
    /// tsc-span: _tsc.js:76381-76394
    ///
    /// CallExpression → the callee NAME span (property-access callee →
    /// `.name`); every other call-like → the node's own error span.
    fn diag_span_for_call_node(&self, node: NodeId) -> DiagSpan {
        if self.kind_of(node) == SyntaxKind::CallExpression {
            let NodeData::CallExpression(data) = self.data_of(node) else {
                unreachable!("kind/data agree");
            };
            if let Some(expression) = data.expression {
                let target = match self.data_of(expression) {
                    NodeData::PropertyAccessExpression(access) => {
                        access.name.unwrap_or(expression)
                    }
                    _ => expression,
                };
                return self.diag_span_of_node(target);
            }
        }
        self.diag_span_of_node(node)
    }

    /// tsc-port: getErrorNodeForCallNode @6.0.3
    /// tsc-hash: 296f56daeae9679cbab125c26d8d8e36bd611c9158598787c53d530e3b40b169
    /// tsc-span: _tsc.js:76395-76406
    fn get_error_node_for_call_node(&self, node: NodeId) -> NodeId {
        let (expression, is_tag) = match self.data_of(node) {
            NodeData::CallExpression(data) => (data.expression, false),
            NodeData::NewExpression(data) => (data.expression, false),
            NodeData::TaggedTemplateExpression(data) => (data.tag, true),
            _ => (None, false),
        };
        let _ = is_tag;
        let Some(expression) = expression else {
            // JSX opening-likes answer tagName (5.7c); everything else
            // the node itself.
            return node;
        };
        match self.data_of(expression) {
            NodeData::PropertyAccessExpression(access) => access.name.unwrap_or(expression),
            _ => expression,
        }
    }

    // ---- untyped/error calls ----

    /// tsc-port: resolveUntypedCall @6.0.3
    /// tsc-hash: 379ef51c9ae1f6439afc5576f00e8dc816ee64ddb144f6aa41939d34a64eec13
    /// tsc-span: _tsc.js:75747-75763
    ///
    /// The deferred overload-failure pass re-enters here and walks the
    /// RAW node arguments — their contextual reads see the stashed
    /// failure candidate (§2 ordering). callLikeExpressionMayHaveType-
    /// Arguments = call/new/tagged/jsx-opening-like; the tagged/binary/
    /// jsx operand arms are 5.7b/c callers.
    pub(crate) fn resolve_untyped_call(&mut self, node: NodeId) -> CheckResult2<SignatureId> {
        match self.data_of(node) {
            NodeData::CallExpression(data) => {
                let type_arguments = data.type_arguments;
                let arguments = data.arguments;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
            }
            NodeData::NewExpression(data) => {
                let type_arguments = data.type_arguments;
                let arguments = data.arguments;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
            }
            NodeData::TaggedTemplateExpression(data) => {
                let type_arguments = data.type_arguments;
                let template = data.template;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                if let Some(template) = template {
                    self.check_expression(template, CheckMode::NORMAL)?;
                }
            }
            NodeData::BinaryExpression(data) => {
                if let Some(left) = data.left {
                    self.check_expression(left, CheckMode::NORMAL)?;
                }
            }
            _ => {
                // JSX opening-likes (attributes operand) arrive at
                // 5.7c; decorators at 5.8.
                return Err(Unsupported::new(
                    "resolveUntypedCall operand walk for JSX/decorator call-likes (5.7c/5.8)",
                ));
            }
        }
        Ok(self.any_signature)
    }

    /// tsc-port: resolveErrorCall @6.0.3
    /// tsc-hash: 6c240d4f52cedae55b64d4baf7391c105c57e9791fc744641e19d598212b953f
    /// tsc-span: _tsc.js:75764-75767
    fn resolve_error_call(&mut self, node: NodeId) -> CheckResult2<SignatureId> {
        self.resolve_untyped_call(node)?;
        Ok(self.unknown_signature)
    }

    // ---- candidate ordering ----

    /// tsc-port: getOptionalCallSignature @6.0.3
    /// tsc-hash: 72c1153c4b5f22b531edb3d2a89992c1b967d04005f6061b1db6000fa3dadb8c
    /// tsc-span: _tsc.js:57895-57910
    ///
    /// createOptionalCallSignature folded in: the per-signature 2-slot
    /// (inner, outer) cache holds the chain-flagged clones consumed by
    /// getReturnTypeOfSignature's 59816-59820 arms.
    fn get_optional_call_signature(
        &mut self,
        signature: SignatureId,
        call_chain_flags: SignatureFlags,
    ) -> SignatureId {
        let existing_flags = self.signature_of(signature).flags;
        if SignatureFlags::from_bits(existing_flags.bits() & SignatureFlags::CALL_CHAIN_FLAGS.bits())
            == call_chain_flags
        {
            return signature;
        }
        let inner = call_chain_flags == SignatureFlags::IS_INNER_CALL_CHAIN;
        debug_assert!(
            inner || call_chain_flags == SignatureFlags::IS_OUTER_CALL_CHAIN,
            "An optional call signature can either be for an inner call chain or an outer call chain, but not both."
        );
        let cache = self.signature_of(signature).optional_call_signature_cache;
        let cached = if inner { cache.0 } else { cache.1 };
        if let Some(cached) = cached {
            return cached;
        }
        let result = self.clone_signature(signature);
        let data = self.signature_mut(result);
        data.flags = SignatureFlags::from_bits(data.flags.bits() | call_chain_flags.bits());
        let cache = &mut self.signature_mut(signature).optional_call_signature_cache;
        if inner {
            cache.0 = Some(result);
        } else {
            cache.1 = Some(result);
        }
        result
    }

    /// tsc-port: reorderCandidates @6.0.3
    /// tsc-hash: 57e0a955200c0d177aea8a27b170dcec198e247503a38539e0b1f93aec0ae896
    /// tsc-span: _tsc.js:75768-75800
    ///
    /// getSymbolOfDeclaration = getMergedSymbol (the L2 bug class);
    /// specialized (literal-typed) signatures splice ahead of the
    /// cutoff, same-symbol runs keep declaration order.
    fn reorder_candidates(
        &mut self,
        signatures: &[SignatureId],
        call_chain_flags: SignatureFlags,
    ) -> CheckResult2<Vec<SignatureId>> {
        let mut result: Vec<SignatureId> = Vec::with_capacity(signatures.len());
        let mut last_parent: Option<NodeId> = None;
        let mut last_symbol: Option<SymbolId> = None;
        let mut cutoff_index = 0usize;
        let mut index = 0usize;
        let mut specialized_index: isize = -1;
        for &signature in signatures {
            let declaration = self.signature_of(signature).declaration;
            let symbol = match declaration {
                Some(declaration) => Some(self.get_symbol_of_declaration(declaration)?),
                None => None,
            };
            let parent = declaration.and_then(|declaration| self.parent_of(declaration));
            if last_symbol.is_none() || symbol == last_symbol {
                if last_parent.is_some() && parent == last_parent {
                    index += 1;
                } else {
                    last_parent = parent;
                    index = cutoff_index;
                }
            } else {
                index = result.len();
                cutoff_index = result.len();
                last_parent = parent;
            }
            last_symbol = symbol;
            let splice_index = if self
                .signature_of(signature)
                .flags
                .intersects(SignatureFlags::HAS_LITERAL_TYPES)
            {
                specialized_index += 1;
                cutoff_index += 1;
                specialized_index as usize
            } else {
                index
            };
            let inserted = if call_chain_flags != SignatureFlags::NONE {
                self.get_optional_call_signature(signature, call_chain_flags)
            } else {
                signature
            };
            result.insert(splice_index.min(result.len()), inserted);
        }
        Ok(result)
    }

    // ---- effective arguments ----

    /// tsc-port: isSpreadArgument @6.0.3
    /// tsc-hash: e8f25340d855029dda888e2c0040df70c87f8bf9ce2f2119994cea002e0cebe6
    /// tsc-span: _tsc.js:75801-75806
    ///
    /// (getSpreadArgumentIndex folded into the callers' findIndex.)
    fn is_spread_argument(&self, arg: &EffectiveArg) -> bool {
        match arg {
            EffectiveArg::Node(node) => self.kind_of(*node) == SyntaxKind::SpreadElement,
            EffectiveArg::Synthetic { is_spread, .. } => *is_spread,
        }
    }

    fn get_spread_argument_index(&self, args: &[EffectiveArg]) -> Option<usize> {
        args.iter().position(|arg| self.is_spread_argument(arg))
    }

    /// tsc-port: getEffectiveCallArguments @6.0.3
    /// tsc-hash: 67e81d21913803f705cb90293b8f58b8841797fe610040cf8b961cc8a5b6a981
    /// tsc-span: _tsc.js:76295-76339
    ///
    /// Call/new band only at 5.7a — the tagged/decorator/JSX arms own
    /// their slices. Spread expansion: flowLoopCount is 0 until M5 so
    /// the spread operand checks through checkExpressionCached; tuple
    /// spreads expand per element into Synthetics (Rest elements wrap
    /// in arrays, Variable bits mark spread-ness, labels ride
    /// tuple_name_source).
    pub(crate) fn get_effective_call_arguments(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Vec<EffectiveArg>> {
        let arguments = match self.data_of(node) {
            NodeData::CallExpression(data) => data.arguments,
            NodeData::NewExpression(data) => data.arguments,
            NodeData::TaggedTemplateExpression(_) => {
                return Err(Unsupported::new(
                    "getEffectiveCallArguments tagged-template arm (TemplateStringsArray, 5.7b)",
                ));
            }
            NodeData::BinaryExpression(data) => {
                let left = data.left.ok_or_else(|| {
                    Unsupported::new("instanceof without a left operand (parse recovery)")
                })?;
                return Ok(vec![EffectiveArg::Node(left)]);
            }
            _ => {
                return Err(Unsupported::new(
                    "getEffectiveCallArguments decorator/JSX arms (5.8/5.7c)",
                ));
            }
        };
        let args: Vec<NodeId> = self.nodes_of(arguments);
        let spread_index = args
            .iter()
            .position(|&arg| self.kind_of(arg) == SyntaxKind::SpreadElement);
        let Some(spread_index) = spread_index else {
            return Ok(args.into_iter().map(EffectiveArg::Node).collect());
        };
        let mut effective_args: Vec<EffectiveArg> =
            args[..spread_index].iter().copied().map(EffectiveArg::Node).collect();
        for &arg in &args[spread_index..] {
            let spread_type = if self.kind_of(arg) == SyntaxKind::SpreadElement {
                let NodeData::SpreadElement(data) = self.data_of(arg) else {
                    unreachable!("kind/data agree");
                };
                match data.expression {
                    // flowLoopCount == 0 until M5 → the cached arm.
                    Some(expression) => {
                        Some(self.check_expression_cached(expression, CheckMode::NORMAL)?)
                    }
                    None => None,
                }
            } else {
                None
            };
            match spread_type {
                Some(spread_type) if self.tables.is_tuple_type(spread_type) => {
                    let element_types = self.get_type_arguments(spread_type)?;
                    let target = self.tables.reference_target(spread_type);
                    let TypeData::TupleTarget(target_data) =
                        self.tables.type_of(target).data.clone()
                    else {
                        unreachable!("tuple type targets a tuple target");
                    };
                    let raw = {
                        let source = self.binder.source_of_node(arg);
                        let raw = source.arena.node(arg);
                        (raw.pos, raw.end)
                    };
                    for (i, &element) in element_types.iter().enumerate() {
                        let flags = target_data.element_flags[i];
                        let ty = if flags.intersects(ElementFlags::REST) {
                            self.create_array_type(element, /*readonly*/ false)?
                        } else {
                            element
                        };
                        let name = target_data
                            .labeled_element_declarations
                            .as_ref()
                            .and_then(|names| names.get(i).copied())
                            .flatten()
                            .map(NodeId);
                        effective_args.push(EffectiveArg::Synthetic {
                            pos: raw.0,
                            end: raw.1,
                            ty,
                            is_spread: flags.intersects(ElementFlags::VARIABLE),
                            tuple_name_source: name,
                        });
                    }
                }
                _ => effective_args.push(EffectiveArg::Node(arg)),
            }
        }
        Ok(effective_args)
    }

    /// checkSyntheticExpression (73946): spread synthetics answer the
    /// number-indexed access of their type, plain synthetics the type
    /// itself. Node args route through the real checkers.
    fn check_effective_arg(
        &mut self,
        arg: &EffectiveArg,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        match *arg {
            EffectiveArg::Node(node) => self.check_expression(node, check_mode),
            EffectiveArg::Synthetic { ty, is_spread, .. } => {
                if is_spread {
                    self.get_indexed_access_type(
                        ty,
                        self.tables.intrinsics.number,
                        tsrs2_types::AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )
                } else {
                    Ok(ty)
                }
            }
        }
    }

    fn check_effective_arg_with_contextual_type(
        &mut self,
        arg: &EffectiveArg,
        contextual_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        match *arg {
            EffectiveArg::Node(node) => self.check_expression_with_contextual_type(
                node,
                contextual_type,
                /*inference_context*/ None,
                check_mode,
            ),
            EffectiveArg::Synthetic { .. } => self.check_effective_arg(arg, check_mode),
        }
    }

    fn effective_arg_kind(&self, arg: &EffectiveArg) -> Option<SyntaxKind> {
        match arg {
            EffectiveArg::Node(node) => Some(self.kind_of(*node)),
            EffectiveArg::Synthetic { .. } => None,
        }
    }

    /// EffectiveArg span (setTextRange semantics for synthetics).
    fn diag_span_of_effective_arg(&self, node_in_file: NodeId, arg: &EffectiveArg) -> DiagSpan {
        match *arg {
            EffectiveArg::Node(node) => self.diag_span_of_node(node),
            EffectiveArg::Synthetic { pos, end, .. } => {
                self.diag_span_of_byte_range(node_in_file, pos, end)
            }
        }
    }

    // ---- arity ----

    /// tsc-port: hasCorrectArity @6.0.3
    /// tsc-hash: f974d5e1c80a39323009b4a83dbeec3fa7eb8b99275f7b5b7f20b96184e65c1f
    /// tsc-span: _tsc.js:75813-75865
    ///
    /// acceptsVoid (75807-75809) folded into the under-min filter; the
    /// JS+nonstrict acceptsVoidUndefinedUnknownOrAny variant is
    /// JS-file-gated (constant false in TS programs). Tagged/JSX arms
    /// land with 5.7b/c; the decorator arm with 5.8.
    fn has_correct_arity(
        &mut self,
        node: NodeId,
        args: &[EffectiveArg],
        signature: SignatureId,
        signature_help_trailing_comma: bool,
    ) -> CheckResult2<bool> {
        let arg_count: usize;
        let mut call_is_incomplete = false;
        // The JSX arm's clamps on these two counts arrive with 5.7c.
        let effective_parameter_count = self.get_parameter_count(signature)?;
        let effective_minimum_arguments = self.get_min_argument_count(signature)?;
        match self.kind_of(node) {
            SyntaxKind::TaggedTemplateExpression => {
                return Err(Unsupported::new(
                    "hasCorrectArity tagged-template arm (5.7b)",
                ));
            }
            SyntaxKind::Decorator => {
                unreachable!("resolveDecorator escapes until 5.8")
            }
            SyntaxKind::BinaryExpression => {
                arg_count = 1;
            }
            kind if matches!(
                kind,
                SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
            ) =>
            {
                return Err(Unsupported::new("hasCorrectArity JSX arm (5.7c)"));
            }
            _ => {
                let arguments = match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    NodeData::NewExpression(data) => data.arguments,
                    _ => None,
                };
                let Some(arguments) = arguments else {
                    // Argument-less `new C`.
                    debug_assert_eq!(self.kind_of(node), SyntaxKind::NewExpression);
                    return Ok(self.get_min_argument_count(signature)? == 0);
                };
                arg_count = if signature_help_trailing_comma {
                    args.len() + 1
                } else {
                    args.len()
                };
                // callIsIncomplete: the argument list's close paren is
                // missing (arguments.end == node.end).
                let source = self.binder.source_of_node(node);
                let arguments_end = source.arena.node_array(arguments).end;
                call_is_incomplete = arguments_end == source.arena.node(node).end;
                if let Some(spread_arg_index) = self.get_spread_argument_index(args) {
                    return Ok(spread_arg_index >= self.get_min_argument_count(signature)?
                        && (self.has_effective_rest_parameter(signature)?
                            || spread_arg_index < self.get_parameter_count(signature)?));
                }
            }
        }
        if !self.has_effective_rest_parameter(signature)? && arg_count > effective_parameter_count
        {
            return Ok(false);
        }
        if call_is_incomplete || arg_count >= effective_minimum_arguments {
            return Ok(true);
        }
        for i in arg_count..effective_minimum_arguments {
            let ty = self.get_type_at_position(signature, i)?;
            let filtered = self.tables.filter_type(ty, |tables, t| {
                tables.flags_of(t).intersects(TypeFlags::VOID)
            });
            if self.tables.flags_of(filtered).intersects(TypeFlags::NEVER) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: hasCorrectTypeArgumentArity @6.0.3
    /// tsc-hash: cb80010a22cf0b9bd684af501fd92e442b330c0bbe9ea3005074570ee292c316
    /// tsc-span: _tsc.js:75866-75870
    pub(crate) fn has_correct_type_argument_arity(
        &self,
        signature: SignatureId,
        type_arguments: &[NodeId],
    ) -> bool {
        let type_parameters = self.signature_of(signature).type_parameters.clone();
        let num_type_parameters = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
        let min_type_argument_count = self.get_min_type_argument_count(type_parameters.as_deref());
        type_arguments.is_empty()
            || (type_arguments.len() >= min_type_argument_count
                && type_arguments.len() <= num_type_parameters)
    }

    // ---- spread argument types ----

    /// tsc-port: getMutableArrayOrTupleType @6.0.3
    /// tsc-hash: 076893146c5750d0d2244745750d89fd6cd633d1aef8084b08066c1ea12ebd0a
    /// tsc-span: _tsc.js:75993-76001
    fn get_mutable_array_or_tuple_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let mapped = self.map_type(
                ty,
                &mut |state, t| state.get_mutable_array_or_tuple_type(t).map(Some),
                /*no_reductions*/ false,
            )?;
            return Ok(mapped.expect("mapper always answers"));
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(ty);
        }
        let constraint_or_self = self.get_base_constraint_of_type(ty)?.unwrap_or(ty);
        if self.is_mutable_array_or_tuple(constraint_or_self)? {
            return Ok(ty);
        }
        if self.tables.is_tuple_type(ty) {
            let element_types = self.get_type_arguments(ty)?;
            let target = self.tables.reference_target(ty);
            let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
                unreachable!("tuple type targets a tuple target");
            };
            let names = data.labeled_element_declarations.clone();
            return self.create_tuple_type_forced(
                &element_types,
                Some(&data.element_flags),
                /*readonly*/ false,
                names.as_deref(),
            );
        }
        self.create_tuple_type_forced(
            &[ty],
            Some(&[ElementFlags::VARIADIC]),
            /*readonly*/ false,
            None,
        )
    }

    /// tsc-port: getSpreadArgumentType @6.0.3
    /// tsc-hash: dfdbbb36374c6ab5201a5e1f9856e353347e1d3cace2f7a7f8d6246a95d6fbce
    /// tsc-span: _tsc.js:76002-76042
    ///
    /// The non-array-like spread arms escape on the [ITER] kit
    /// (checkIteratedTypeOrElementType — iteration protocols are 5.8
    /// machinery).
    fn get_spread_argument_type(
        &mut self,
        args: &[EffectiveArg],
        index: usize,
        arg_count: usize,
        rest_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let in_const_context = self.is_const_type_variable(Some(rest_type), 0);
        if arg_count > 0 && index >= arg_count - 1 {
            let arg = &args[arg_count - 1];
            if self.is_spread_argument(arg) {
                let spread_type = match *arg {
                    EffectiveArg::Synthetic { ty, .. } => ty,
                    EffectiveArg::Node(node) => {
                        let NodeData::SpreadElement(data) = self.data_of(node) else {
                            unreachable!("spread arguments are spread elements");
                        };
                        let expression = data.expression.ok_or_else(|| {
                            Unsupported::new("spread without operand (parse recovery)")
                        })?;
                        self.check_expression_with_contextual_type(
                            expression,
                            rest_type,
                            /*inference_context*/ None,
                            check_mode,
                        )?
                    }
                };
                if self.is_array_like_type(spread_type)? {
                    return self.get_mutable_array_or_tuple_type(spread_type);
                }
                return Err(Unsupported::new(
                    "checkIteratedTypeOrElementType for non-array spread arguments ([ITER] 5.8)",
                ));
            }
        }
        let mut types: Vec<TypeId> = Vec::new();
        let mut flags: Vec<ElementFlags> = Vec::new();
        let mut names: Vec<Option<u32>> = Vec::new();
        for i in index..arg_count {
            let arg = args[i];
            if self.is_spread_argument(&arg) {
                let spread_type = match arg {
                    EffectiveArg::Synthetic { ty, .. } => ty,
                    EffectiveArg::Node(node) => {
                        let NodeData::SpreadElement(data) = self.data_of(node) else {
                            unreachable!("spread arguments are spread elements");
                        };
                        let expression = data.expression.ok_or_else(|| {
                            Unsupported::new("spread without operand (parse recovery)")
                        })?;
                        self.check_expression(expression, CheckMode::NORMAL)?
                    }
                };
                if self.is_array_like_type(spread_type)? {
                    types.push(spread_type);
                    flags.push(ElementFlags::VARIADIC);
                } else {
                    return Err(Unsupported::new(
                        "checkIteratedTypeOrElementType for non-array spread arguments ([ITER] 5.8)",
                    ));
                }
            } else {
                let contextual_type = if self.tables.is_tuple_type(rest_type) {
                    self.get_contextual_type_for_element_expression_lengthed(
                        rest_type,
                        i - index,
                        arg_count - index,
                    )?
                    .unwrap_or(self.tables.intrinsics.unknown)
                } else {
                    let literal = self.tables.get_number_literal_type((i - index) as f64);
                    self.get_indexed_access_type(
                        rest_type,
                        literal,
                        tsrs2_types::AccessFlags::CONTEXTUAL,
                        None,
                        None,
                        None,
                    )?
                };
                let arg_type =
                    self.check_effective_arg_with_contextual_type(&arg, contextual_type, check_mode)?;
                let has_primitive_contextual_type = in_const_context
                    || self.maybe_type_of_kind(
                        contextual_type,
                        TypeFlags::from_bits(
                            TypeFlags::PRIMITIVE.bits()
                                | TypeFlags::INDEX.bits()
                                | TypeFlags::TEMPLATE_LITERAL.bits()
                                | TypeFlags::STRING_MAPPING.bits(),
                        ),
                    );
                types.push(if has_primitive_contextual_type {
                    self.tables.get_regular_type_of_literal_type(arg_type)
                } else {
                    self.get_widened_literal_type(arg_type)?
                });
                flags.push(ElementFlags::REQUIRED);
            }
            if let EffectiveArg::Synthetic {
                tuple_name_source: Some(name),
                ..
            } = arg
            {
                names.push(Some(name.0));
            } else {
                names.push(None);
            }
        }
        let readonly = in_const_context
            && !self.some_type_result(rest_type, |state, t| state.is_mutable_array_like_type(t))?;
        let named = names.iter().any(Option::is_some);
        self.create_tuple_type_forced(
            &types,
            Some(&flags),
            readonly,
            named.then_some(names.as_slice()),
        )
    }

    // ---- explicit type arguments ----

    /// tsc-port: checkTypeArguments @6.0.3
    /// tsc-hash: f903e04f64b4cdb3a2c094232953e29f212a4f803d955fa7b4c8c902869a0cd2
    /// tsc-span: _tsc.js:76043-76074
    ///
    /// Silent during selection (reportErrors=false), real on the
    /// failure ladder. The headMessage flavor belongs to instanceof/
    /// decorator resolution (5.7b/5.8) — plain calls report the bare
    /// 2344 head; reportRelationError's source shaping (literal
    /// generalization) applies like every relation head.
    pub(crate) fn check_type_arguments(
        &mut self,
        signature: SignatureId,
        type_argument_nodes: &[NodeId],
        report_errors: bool,
        head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        if head_message.is_some() {
            // The chained-head flavor (instanceof 2860 / decorator)
            // arrives with 5.7b/5.8.
            return Err(Unsupported::new(
                "checkTypeArguments under a head message (5.7b/5.8)",
            ));
        }
        let type_parameters = self
            .signature_of(signature)
            .type_parameters
            .clone()
            .expect("checkTypeArguments callers guarantee a generic signature");
        let mut mapped: Vec<TypeId> = Vec::with_capacity(type_argument_nodes.len());
        for &node in type_argument_nodes {
            mapped.push(self.get_type_from_type_node(node)?);
        }
        let min_type_argument_count = self.get_min_type_argument_count(Some(&type_parameters));
        let type_argument_types = self
            .fill_missing_type_arguments(
                Some(&mapped),
                Some(&type_parameters),
                min_type_argument_count,
                /*is_javascript*/ false,
            )?
            .expect("Some input yields Some");
        let mut mapper = None;
        for (i, &type_argument_node) in type_argument_nodes.iter().enumerate() {
            debug_assert!(
                type_parameters.get(i).is_some(),
                "Should not call checkTypeArguments with too many type arguments"
            );
            let Some(constraint) = self.get_constraint_of_type_parameter(type_parameters[i])?
            else {
                continue;
            };
            if mapper.is_none() {
                mapper = Some(self.create_type_mapper(
                    type_parameters.clone(),
                    Some(type_argument_types.clone()),
                ));
            }
            let type_argument = type_argument_types[i];
            let instantiated = self.instantiate_type(constraint, mapper)?;
            let target = self.get_type_with_this_argument(
                instantiated,
                Some(type_argument),
                /*need_apparent_type*/ false,
            )?;
            if !self.is_type_assignable_to(type_argument, target)? {
                if report_errors {
                    let span = self.diag_span_of_node(type_argument_node);
                    let diagnostic = self.build_relation_error_with_head(
                        type_argument,
                        target,
                        &span,
                        &diagnostics::Type_0_does_not_satisfy_the_constraint_1,
                    )?;
                    self.push_error_diagnostic(diagnostic);
                }
                return Ok(None);
            }
        }
        Ok(Some(type_argument_types))
    }

    /// The tuple-rest contextual read getSpreadArgumentType makes
    /// (76029): getContextualTypeForElementExpression(restType, index,
    /// length) with no spread bookkeeping.
    fn get_contextual_type_for_element_expression_lengthed(
        &mut self,
        ty: TypeId,
        index: usize,
        length: usize,
    ) -> CheckResult2<Option<TypeId>> {
        self.get_contextual_type_for_element_expression_at(ty, index, Some(length))
    }

    // ---- relation heads (the T2-curtain reporter) ----

    /// reportRelationError (65064-65115) under a PRESENT head message:
    /// the code is ALWAYS the head's (the unmatched-property override
    /// and the identically-named-types message swap are both gated on
    /// `!headMessage` — reportErrorResults 65286), the source display
    /// generalizes literals, and the chain tail elides (T2). Display
    /// failures unwind Unsupported per the house discipline.
    fn build_relation_error_with_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: &DiagSpan,
        head: &'static DiagnosticMessage,
    ) -> CheckResult2<Diagnostic> {
        if head.code == diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1.code
            && self.options.exact_optional_property_types.unwrap_or(false)
        {
            // 65111: the 2345→exactOptionalPropertyTypes head variant
            // needs getExactOptionalUnassignableProperties — escape
            // under the option rather than mis-pick the code.
            return Err(Unsupported::new(
                "exactOptionalPropertyTypes argument-head variant selection",
            ));
        }
        let source_text = self.type_to_string_slice(source)?;
        let target_text = self.type_to_string_slice(target)?;
        // 65069-65072: literal sources generalize to their base
        // primitive unless the target could accept singletons.
        let source_text = if !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
            && self.is_literal_type(source)
            && !self.type_could_have_top_level_singleton_types(target)?
        {
            let generalized = self.get_base_type_of_literal_type(source)?;
            self.type_to_string_slice(generalized)?
        } else {
            source_text
        };
        Ok(self.diagnostic_at_span(
            span,
            MessageChain::new(head, &[source_text, target_text]),
        ))
    }

    // ---- elaboration gate ----

    /// isOrHasGenericConditional (63954-63956).
    fn is_or_has_generic_conditional(&self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::CONDITIONAL) {
            return true;
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            if let TypeData::Intersection { types } = &self.tables.type_of(ty).data {
                return types
                    .to_vec()
                    .iter()
                    .any(|&t| self.is_or_has_generic_conditional(t));
            }
        }
        false
    }

    /// getSingleSignature(type, Call, allowMembers=false) (75896-75909)
    /// — the elaborateArrowFunction source gate.
    fn get_single_call_signature_no_members(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<SignatureId>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        if resolved.properties.is_empty()
            && resolved.index_infos.is_empty()
            && resolved.call_signatures.len() == 1
            && resolved.construct_signatures.is_empty()
        {
            return Ok(Some(resolved.call_signatures[0]));
        }
        Ok(None)
    }

    /// elaborateError's eligibility walk (63957-63994): Ok(None) = no
    /// elaboration (plain head), Ok(Some) = elaborateDidYouMeanToCall-
    /// OrConstruct fired (same head + span at the WALKED node, plus
    /// the did-you-mean related row — 63995-64023), Err = an
    /// elementwise elaboration would move the code/span into the
    /// literal (T2 machinery, contain).
    fn elaboration_disposition(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<Option<(NodeId, RelatedInfo)>> {
        if self.is_or_has_generic_conditional(target) {
            return Ok(None);
        }
        let mut walk = node;
        loop {
            // 63959-63967: the did-you-mean probe runs per recursion
            // level, reporting at the CURRENT node.
            for kind in [SignatureKind::Construct, SignatureKind::Call] {
                let signatures = self.get_signatures_of_type(source, kind)?;
                let mut fires = false;
                for signature in signatures {
                    let return_type = self.get_return_type_of_signature(signature)?;
                    if self
                        .tables
                        .flags_of(return_type)
                        .intersects(TypeFlags::ANY | TypeFlags::NEVER)
                    {
                        continue;
                    }
                    if self.check_type_related_to(return_type, target, relation)? {
                        fires = true;
                        break;
                    }
                }
                if fires {
                    let message = if kind == SignatureKind::Construct {
                        &diagnostics::Did_you_mean_to_use_new_with_this_expression
                    } else {
                        &diagnostics::Did_you_mean_to_call_this_expression
                    };
                    let related = self.related_info_for_node(walk, message, &[]);
                    return Ok(Some((walk, related)));
                }
            }
            match self.kind_of(walk) {
                SyntaxKind::AsExpression => {
                    let NodeData::AsExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let is_const = data
                        .r#type
                        .is_some_and(|type_node| self.is_const_type_reference_node(type_node));
                    if !is_const {
                        return Ok(None);
                    }
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(None),
                    }
                }
                SyntaxKind::JsxExpression => {
                    let NodeData::JsxExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(None),
                    }
                }
                SyntaxKind::ParenthesizedExpression => {
                    let NodeData::ParenthesizedExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(None),
                    }
                }
                SyntaxKind::BinaryExpression => {
                    let NodeData::BinaryExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let operator = data.operator_token.map(|token| self.kind_of(token));
                    match operator {
                        Some(SyntaxKind::EqualsToken | SyntaxKind::CommaToken) => {
                            match data.right {
                                Some(right) => walk = right,
                                None => return Ok(None),
                            }
                        }
                        _ => return Ok(None),
                    }
                }
                SyntaxKind::ObjectLiteralExpression => {
                    // elaborateObjectLiteral (64456): the primitive/
                    // never-target early-out falls back to the plain
                    // head — whose object-literal source display is T2
                    // anyway — so the blanket escape loses nothing.
                    return Err(Unsupported::new(
                        "elaborateObjectLiteral (elementwise elaboration, T2)",
                    ));
                }
                SyntaxKind::ArrayLiteralExpression => {
                    // elaborateArrayLiteral (64410): decide whether the
                    // elementwise walk WOULD report — if not, tsc falls
                    // back to the plain head at the literal (live).
                    if self.array_literal_elaboration_would_report(walk, source, target, relation)? {
                        return Err(Unsupported::new(
                            "elaborateArrayLiteral (elementwise elaboration, T2)",
                        ));
                    }
                    return Ok(None);
                }
                SyntaxKind::JsxAttributes => {
                    return Err(Unsupported::new(
                        "elaborateJsxComponents (elementwise elaboration, T2)",
                    ));
                }
                SyntaxKind::ArrowFunction => {
                    // elaborateArrowFunction gates (64024-64038): an
                    // expression body, no annotated parameters, a
                    // single-call-signature source, and a callable
                    // target make the elaboration recurse into the
                    // return expression.
                    let NodeData::ArrowFunction(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let body_is_block = data
                        .body
                        .is_some_and(|body| self.kind_of(body) == SyntaxKind::Block);
                    if body_is_block {
                        return Ok(None);
                    }
                    let parameters = self.nodes_of(data.parameters);
                    let has_typed_parameter = parameters.iter().any(|&parameter| {
                        matches!(self.data_of(parameter), NodeData::Parameter(p) if p.r#type.is_some())
                    });
                    if has_typed_parameter {
                        return Ok(None);
                    }
                    if self.get_single_call_signature_no_members(source)?.is_none() {
                        return Ok(None);
                    }
                    if self
                        .get_signatures_of_type(target, SignatureKind::Call)?
                        .is_empty()
                    {
                        return Ok(None);
                    }
                    return Err(Unsupported::new(
                        "elaborateArrowFunction (return-position elaboration, T2)",
                    ));
                }
                _ => return Ok(None),
            }
        }
    }

    /// The elaborateArrayLiteral decision (64410-64431 +
    /// generateLimitedTupleElements 64398 + elaborateElementwise's
    /// per-element verdicts): true when tsc's elaboration would emit
    /// inner rows instead of the plain head.
    fn array_literal_elaboration_would_report(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<bool> {
        if self
            .tables
            .flags_of(target)
            .intersects(TypeFlags::from_bits(
                TypeFlags::PRIMITIVE.bits() | TypeFlags::NEVER.bits(),
            ))
        {
            return Ok(false);
        }
        let elements = match self.data_of(node) {
            NodeData::ArrayLiteralExpression(data) => self.nodes_of(data.elements),
            _ => return Ok(false),
        };
        // Target-side pass first: an element can only produce a row
        // when the target has a matching indexed access — deciding
        // this before the forced-tuple re-check keeps no-index targets
        // out of the [ITER] contextual escapes.
        let mut candidates: Vec<(usize, TypeId)> = Vec::new();
        for (i, &element) in elements.iter().enumerate() {
            if self.is_tuple_like_type(target)?
                && self.get_property_of_type_full(target, &i.to_string())?.is_none()
            {
                continue;
            }
            if self.kind_of(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let name_type = self.tables.get_number_literal_type(i as f64);
            // getBestMatchIndexedAccessTypeOrUndefined (64103): the
            // union-target discriminant fallback is unmodeled — a
            // union target without a direct indexed access contains.
            let target_prop = self.get_indexed_access_type_or_undefined(
                target,
                name_type,
                tsrs2_types::AccessFlags::NONE,
                None,
                None,
                None,
            )?;
            let Some(target_prop) = target_prop else {
                if self.tables.flags_of(target).intersects(TypeFlags::UNION) {
                    return Err(Unsupported::new(
                        "getBestMatchingType union-target elaboration probe (T2)",
                    ));
                }
                continue;
            };
            if self
                .tables
                .flags_of(target_prop)
                .intersects(TypeFlags::INDEXED_ACCESS)
            {
                continue;
            }
            candidates.push((i, target_prop));
        }
        if candidates.is_empty() {
            return Ok(false);
        }
        let tupleized = if self.is_tuple_like_type(source)? {
            source
        } else {
            // 64416-64423: re-check as a forced tuple under the target
            // context (re-runs dedupe against the original check).
            self.push_contextual_type(node, Some(target), /*is_cache*/ false);
            let result =
                self.check_array_literal(node, CheckMode::CONTEXTUAL, /*force_tuple*/ true);
            self.pop_contextual_type();
            let tupleized = result?;
            if !self.is_tuple_like_type(tupleized)? {
                return Ok(false);
            }
            tupleized
        };
        for (i, target_prop) in candidates {
            let name_type = self.tables.get_number_literal_type(i as f64);
            let Some(source_prop) = self.get_indexed_access_type_or_undefined(
                tupleized,
                name_type,
                tsrs2_types::AccessFlags::NONE,
                None,
                None,
                None,
            )?
            else {
                continue;
            };
            if !self.check_type_related_to(source_prop, target_prop, relation)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- this arguments ----

    /// tsc-port: getThisArgumentOfCall @6.0.3
    /// tsc-hash: 13dfb639a189d005e4cb66f7c3362059101d200d03a5001d3bb534e094422b13
    /// tsc-span: _tsc.js:76277-76288
    fn get_this_argument_of_call(&self, node: NodeId) -> Option<NodeId> {
        let expression = match self.data_of(node) {
            NodeData::BinaryExpression(data) => return data.right,
            NodeData::CallExpression(data) => data.expression,
            NodeData::TaggedTemplateExpression(data) => data.tag,
            // The non-legacy decorator arm is 5.8.
            _ => None,
        }?;
        let callee = self.skip_outer_expressions(expression, OuterExpressionKinds::ALL);
        match self.data_of(callee) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        }
    }

    /// tsc-port: getThisArgumentType @6.0.3
    /// tsc-hash: b30e99a4a3ccb0345c83b20f6696d1511bfd72a263972aa44b56318304205fbe
    /// tsc-span: _tsc.js:75931-75937
    fn get_this_argument_type(
        &mut self,
        this_argument_node: Option<NodeId>,
    ) -> CheckResult2<TypeId> {
        let Some(node) = this_argument_node else {
            return Ok(self.tables.intrinsics.void);
        };
        let this_argument_type = self.check_expression(node, CheckMode::NORMAL)?;
        let is_instanceof_right = self.parent_of(node).is_some_and(|parent| {
            matches!(self.data_of(parent), NodeData::BinaryExpression(data)
                if data.right == Some(node)
                    && data.operator_token
                        .is_some_and(|token| self.kind_of(token) == SyntaxKind::InstanceOfKeyword))
        });
        if is_instanceof_right {
            return Ok(this_argument_type);
        }
        let source = self.binder.source_of_node(node);
        let parent = self.parent_of(node);
        if parent.is_some_and(|parent| node_util::is_optional_chain_root(source, parent)) {
            return self.get_non_nullable_type(this_argument_type);
        }
        if parent.is_some_and(|parent| node_util::is_optional_chain(source, parent)) {
            return Ok(self.remove_optional_type_marker(this_argument_type));
        }
        Ok(this_argument_type)
    }

    // ---- applicability ----

    /// tsc-port: getSignatureApplicabilityError @6.0.3
    /// tsc-hash: bd05784a6cdf0b44aae49b1b7135d05b6105da3b41e39e7b01d3950a29709f1b
    /// tsc-span: _tsc.js:76194-76276
    ///
    /// None = applicable; Some = the errorOutputContainer contents.
    /// Silent mode (selection) collects verdicts only; Report builds
    /// the head diagnostics; Probe computes spans/related for the
    /// display-free overload chains (2769). maybeAddMissingAwaitInfo
    /// (76265-76275) rides as related rows.
    fn get_signature_applicability_error(
        &mut self,
        node: NodeId,
        args: &[EffectiveArg],
        signature: SignatureId,
        relation: RelationKind,
        check_mode: CheckMode,
        mode: ApplicabilityMode,
    ) -> CheckResult2<Option<Vec<ApplicabilityError>>> {
        if matches!(
            self.kind_of(node),
            SyntaxKind::JsxOpeningElement
                | SyntaxKind::JsxSelfClosingElement
                | SyntaxKind::JsxOpeningFragment
        ) {
            return Err(Unsupported::new(
                "checkApplicableSignatureForJsxCallLikeElement (5.7c)",
            ));
        }
        let this_type = self.get_this_type_of_signature(signature)?;
        if let Some(this_type) = this_type {
            let is_new = self.kind_of(node) == SyntaxKind::NewExpression;
            let is_super_property_call = matches!(self.data_of(node), NodeData::CallExpression(data)
                if data.expression.is_some_and(|expression| self.is_super_property(expression)));
            if this_type != self.tables.intrinsics.void && !is_new && !is_super_property_call {
                let this_argument_node = self.get_this_argument_of_call(node);
                let this_argument_type = self.get_this_argument_type(this_argument_node)?;
                if !self.is_type_related_to(this_argument_type, this_type, relation)? {
                    if mode == ApplicabilityMode::Silent {
                        return Ok(Some(Vec::new()));
                    }
                    let span = self.diag_span_of_node(this_argument_node.unwrap_or(node));
                    let diagnostic = match mode {
                        ApplicabilityMode::Report => Some(self.build_relation_error_with_head(
                            this_argument_type,
                            this_type,
                            &span,
                            &diagnostics::The_this_context_of_type_0_is_not_assignable_to_method_s_this_of_type_1,
                        )?),
                        _ => None,
                    };
                    return Ok(Some(vec![ApplicabilityError {
                        span,
                        related: Vec::new(),
                        diagnostic,
                    }]));
                }
            }
        }
        let head = &diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1;
        let rest_type = self.get_non_array_rest_type(signature)?;
        let arg_count = if rest_type.is_some() {
            std::cmp::min(self.get_parameter_count(signature)? - 1, args.len())
        } else {
            args.len()
        };
        for i in 0..arg_count {
            let arg = args[i];
            if self.effective_arg_kind(&arg) == Some(SyntaxKind::OmittedExpression) {
                continue;
            }
            let param_type = self.get_type_at_position(signature, i)?;
            let arg_type = self.check_effective_arg_with_contextual_type(&arg, param_type, check_mode)?;
            let check_arg_type = if check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE) {
                self.get_regular_type_of_object_literal(arg_type)?
            } else {
                arg_type
            };
            if !self.is_type_related_to(check_arg_type, param_type, relation)? {
                if mode == ApplicabilityMode::Silent {
                    return Ok(Some(Vec::new()));
                }
                let effective = match arg {
                    EffectiveArg::Node(arg_node) => {
                        Some(self.get_effective_check_node(arg_node))
                    }
                    EffectiveArg::Synthetic { .. } => None,
                };
                // The elaboration gate: elementwise elaborations move
                // the code/span (Err); the did-you-mean flavor keeps
                // the head but reports at the walked node.
                let mut span = self.diag_span_of_effective_arg(node, &arg);
                let mut related: Vec<RelatedInfo> = Vec::new();
                if let Some(effective) = effective {
                    if let Some((walked, did_you_mean)) = self.elaboration_disposition(
                        effective,
                        check_arg_type,
                        param_type,
                        relation,
                    )? {
                        span = self.diag_span_of_node(walked);
                        related.push(did_you_mean);
                    }
                }
                if let Some(await_related) =
                    self.missing_await_related(&arg, check_arg_type, param_type, relation)?
                {
                    related.push(await_related);
                }
                let diagnostic = match mode {
                    ApplicabilityMode::Report => {
                        let mut diagnostic = self.build_relation_error_with_head(
                            check_arg_type,
                            param_type,
                            &span,
                            head,
                        )?;
                        diagnostic.related = related.clone();
                        Some(diagnostic)
                    }
                    _ => None,
                };
                return Ok(Some(vec![ApplicabilityError {
                    span,
                    related,
                    diagnostic,
                }]));
            }
        }
        if let Some(rest_type) = rest_type {
            let spread_type =
                self.get_spread_argument_type(args, arg_count, args.len(), rest_type, check_mode)?;
            if !self.is_type_related_to(spread_type, rest_type, relation)? {
                if mode == ApplicabilityMode::Silent {
                    return Ok(Some(Vec::new()));
                }
                let rest_arg_count = args.len() - arg_count;
                let span = if rest_arg_count == 0 {
                    self.diag_span_of_node(node)
                } else if rest_arg_count == 1 {
                    match args[arg_count] {
                        EffectiveArg::Node(arg_node) => {
                            self.diag_span_of_node(self.get_effective_check_node(arg_node))
                        }
                        arg @ EffectiveArg::Synthetic { .. } => {
                            self.diag_span_of_effective_arg(node, &arg)
                        }
                    }
                } else {
                    let pos = self.effective_arg_pos(&args[arg_count]);
                    let end = self.effective_arg_end(&args[args.len() - 1]);
                    self.diag_span_of_byte_range(node, pos, end)
                };
                let mut related: Vec<RelatedInfo> = Vec::new();
                if let Some(await_related) = self.missing_await_related_at(
                    Some(&span),
                    spread_type,
                    rest_type,
                    relation,
                )? {
                    related.push(await_related);
                }
                let diagnostic = match mode {
                    ApplicabilityMode::Report => {
                        let mut diagnostic = self.build_relation_error_with_head(
                            spread_type,
                            rest_type,
                            &span,
                            head,
                        )?;
                        diagnostic.related = related.clone();
                        Some(diagnostic)
                    }
                    _ => None,
                };
                return Ok(Some(vec![ApplicabilityError {
                    span,
                    related,
                    diagnostic,
                }]));
            }
        }
        Ok(None)
    }

    fn effective_arg_pos(&self, arg: &EffectiveArg) -> u32 {
        match *arg {
            EffectiveArg::Node(node) => {
                let source = self.binder.source_of_node(node);
                source.arena.node(node).pos
            }
            EffectiveArg::Synthetic { pos, .. } => pos,
        }
    }

    fn effective_arg_end(&self, arg: &EffectiveArg) -> u32 {
        match *arg {
            EffectiveArg::Node(node) => {
                let source = self.binder.source_of_node(node);
                source.arena.node(node).end
            }
            EffectiveArg::Synthetic { end, .. } => end,
        }
    }

    /// maybeAddMissingAwaitInfo (76265-76275): related 2773 when the
    /// awaited source relates to the target and the target itself is
    /// not promise-like.
    fn missing_await_related(
        &mut self,
        arg: &EffectiveArg,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<Option<RelatedInfo>> {
        if self.get_awaited_type_of_promise(target)?.is_some() {
            return Ok(None);
        }
        let Some(awaited_source) = self.get_awaited_type_of_promise(source)? else {
            return Ok(None);
        };
        if !self.is_type_related_to(awaited_source, target, relation)? {
            return Ok(None);
        }
        match *arg {
            EffectiveArg::Node(arg_node) => {
                let error_node = self.get_effective_check_node(arg_node);
                Ok(Some(self.related_info_for_node(
                    error_node,
                    &diagnostics::Did_you_forget_to_use_await,
                    &[],
                )))
            }
            EffectiveArg::Synthetic { .. } => Ok(None),
        }
    }

    fn missing_await_related_at(
        &mut self,
        span: Option<&DiagSpan>,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<Option<RelatedInfo>> {
        let Some(span) = span else { return Ok(None) };
        if self.get_awaited_type_of_promise(target)?.is_some() {
            return Ok(None);
        }
        let Some(awaited_source) = self.get_awaited_type_of_promise(source)? else {
            return Ok(None);
        };
        if !self.is_type_related_to(awaited_source, target, relation)? {
            return Ok(None);
        }
        Ok(Some(RelatedInfo {
            file_name: Some(span.file_name.clone()),
            start: Some(span.start),
            length: Some(span.length),
            message: MessageChain::new(&diagnostics::Did_you_forget_to_use_await, &[]),
        }))
    }

    /// isSuperProperty (16007): property/element access whose
    /// expression is `super`.
    fn is_super_property(&self, node: NodeId) -> bool {
        let expression = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        expression.is_some_and(|expression| self.kind_of(expression) == SyntaxKind::SuperKeyword)
    }

    // ---- resolveCall ----

    /// tsc-port: resolveCall @6.0.3
    /// tsc-hash: 953dbc1e549a14a2152d422085bb1026d78c85964e4e14c962d7d0711c0875cb
    /// tsc-span: _tsc.js:76579-76870
    ///
    /// candidatesOutArray / IsForSignatureHelp are LSP-only (always
    /// None/false — signatureHelpTrailingComma stays false);
    /// isInferencePartiallyBlocked is M6 state (reportErrors stays
    /// true). The chooseOverload/addImplementationSuccessElaboration
    /// closures live on ResolveCallCtx.
    pub(crate) fn resolve_call(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        check_mode: CheckMode,
        call_chain_flags: SignatureFlags,
        mut head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult2<SignatureId> {
        let node_kind = self.kind_of(node);
        let is_decorator = node_kind == SyntaxKind::Decorator;
        let is_instanceof = node_kind == SyntaxKind::BinaryExpression;
        let is_jsx_open_fragment = node_kind == SyntaxKind::JsxOpeningFragment;
        debug_assert!(!self.is_inference_partially_blocked, "M6 state leaked");
        let is_super_call = matches!(self.data_of(node), NodeData::CallExpression(data)
            if data.expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::SuperKeyword));

        // 76593-76598: type arguments — skipped entirely for
        // decorator/instanceof/super-call/jsx-fragment; each checks
        // EXCEPT on super-expression calls.
        let mut type_argument_nodes: Vec<NodeId> = Vec::new();
        let mut type_arguments_array: Option<NodeArrayId> = None;
        if !is_decorator && !is_instanceof && !is_super_call && !is_jsx_open_fragment {
            type_arguments_array = match self.data_of(node) {
                NodeData::CallExpression(data) => data.type_arguments,
                NodeData::NewExpression(data) => data.type_arguments,
                NodeData::TaggedTemplateExpression(data) => data.type_arguments,
                _ => None,
            };
            type_argument_nodes = self.nodes_of(type_arguments_array);
            for &argument in &type_argument_nodes {
                self.check_source_element(Some(argument));
            }
        }

        let candidates = self.reorder_candidates(signatures, call_chain_flags)?;
        if !is_jsx_open_fragment && candidates.is_empty() {
            let span = self.diag_span_for_call_node(node);
            let diagnostic = self.diagnostic_at_span(
                &span,
                MessageChain::new(&diagnostics::Call_target_does_not_contain_any_signatures, &[]),
            );
            self.push_error_diagnostic(diagnostic);
            return self.resolve_error_call(node);
        }

        let args = self.get_effective_call_arguments(node)?;
        let is_single_non_generic_candidate =
            candidates.len() == 1 && self.signature_of(candidates[0]).type_parameters.is_none();
        let mut arg_check_mode = CheckMode::NORMAL;
        if !is_decorator && !is_single_non_generic_candidate {
            let any_context_sensitive = args.iter().any(|arg| match arg {
                EffectiveArg::Node(node) => self.is_context_sensitive(*node),
                EffectiveArg::Synthetic { .. } => false,
            });
            if any_context_sensitive {
                arg_check_mode = CheckMode::SKIP_CONTEXT_SENSITIVE;
            }
        }

        let mut ctx = ResolveCallCtx {
            node,
            args,
            type_arguments_array,
            type_argument_nodes,
            arg_check_mode,
            candidates,
            candidates_for_argument_error: None,
            candidate_for_argument_arity_error: None,
            candidate_for_type_argument_error: None,
        };

        let mut result: Option<SignatureId> = None;
        if ctx.candidates.len() > 1 {
            result = self.choose_overload(
                &mut ctx,
                RelationKind::Subtype,
                is_single_non_generic_candidate,
            )?;
        }
        if result.is_none() {
            result = self.choose_overload(
                &mut ctx,
                RelationKind::Assignable,
                is_single_non_generic_candidate,
            )?;
        }

        // 76621-76625: a re-entrant resolution (context-sensitive arg →
        // contextual read → getResolvedSignature of the SAME node) may
        // have concretely resolved the links mid-flight.
        if let LinkSlot::Resolved(resolved) = self.links.node(node).resolved_signature {
            return Ok(resolved);
        }
        if let Some(result) = result {
            return Ok(result);
        }

        // Failure: stash the candidate BEFORE error reporting so the
        // deferred re-checks and contextual reads see its parameters
        // (76629-76630, load-bearing ordering).
        let result = self.get_candidate_for_overload_failure(node, &mut ctx, check_mode)?;
        self.links.set_node_resolved_signature_call_protocol(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(result),
        );

        if head_message.is_none() && is_instanceof {
            head_message = Some(&diagnostics::The_left_hand_side_of_an_instanceof_expression_must_be_assignable_to_the_first_argument_of_the_right_hand_side_s_Symbol_hasInstance_method);
        }
        if head_message.is_some() {
            // The instanceof (2860) and decorator flavors arrive with
            // 5.7b/5.8 — the chained-head ladder shapes are untested
            // until then.
            return Err(Unsupported::new(
                "resolveCall failure ladder under a head message (5.7b/5.8)",
            ));
        }
        self.report_call_resolution_failure(node, &mut ctx, signatures)?;
        Ok(result)
    }

    /// The reportErrors tail of resolveCall (76631-76742): the four-
    /// rung failure ladder.
    fn report_call_resolution_failure(
        &mut self,
        node: NodeId,
        ctx: &mut ResolveCallCtx,
        signatures: &[SignatureId],
    ) -> CheckResult2<()> {
        if let Some(candidates_for_argument_error) = ctx.candidates_for_argument_error.take() {
            ctx.candidates_for_argument_error = Some(candidates_for_argument_error.clone());
            if candidates_for_argument_error.len() == 1
                || candidates_for_argument_error.len() > 3
            {
                let last = *candidates_for_argument_error
                    .last()
                    .expect("non-empty by construction");
                let over_three = candidates_for_argument_error.len() > 3;
                let args = ctx.args.clone();
                if over_three {
                    // The chain heads (2769/2770) are display-free; the
                    // relation tail elides (T2) — Probe mode supplies
                    // the span tsc's inner diagnostic would carry.
                    let errors = self
                        .get_signature_applicability_error(
                            node,
                            &args,
                            last,
                            RelationKind::Assignable,
                            CheckMode::NORMAL,
                            ApplicabilityMode::Probe,
                        )?
                        .unwrap_or_else(|| {
                            panic!(
                                "No error for last overload signature @{}",
                                self.binder.source_of_node(node).file_name
                            )
                        });
                    for error in errors {
                        let chain = MessageChain::new(
                            &diagnostics::No_overload_matches_this_call,
                            &[],
                        )
                        .with_next(vec![MessageChain::new(
                            &diagnostics::The_last_overload_gave_the_following_error,
                            &[],
                        )]);
                        let mut diagnostic = self.diagnostic_at_span(&error.span, chain);
                        diagnostic.related = error.related;
                        if let Some(declaration) = self.signature_of(last).declaration {
                            diagnostic.related.push(self.related_info_for_node(
                                declaration,
                                &diagnostics::The_last_overload_is_declared_here,
                                &[],
                            ));
                        }
                        if let Some(related) =
                            self.implementation_success_elaboration(ctx, last)?
                        {
                            diagnostic.related.push(related);
                        }
                        self.push_error_diagnostic(diagnostic);
                    }
                } else {
                    let errors = self
                        .get_signature_applicability_error(
                            node,
                            &args,
                            last,
                            RelationKind::Assignable,
                            CheckMode::NORMAL,
                            ApplicabilityMode::Report,
                        )?
                        .unwrap_or_else(|| {
                            panic!(
                                "No error for last overload signature @{}",
                                self.binder.source_of_node(node).file_name
                            )
                        });
                    for error in errors {
                        let mut diagnostic =
                            error.diagnostic.expect("Report mode builds diagnostics");
                        if let Some(related) =
                            self.implementation_success_elaboration(ctx, last)?
                        {
                            diagnostic.related.push(related);
                        }
                        self.push_error_diagnostic(diagnostic);
                    }
                }
            } else {
                // 76667-76722: 2-3 failed candidates — each re-runs
                // under an `Overload N of M` chain (display-elided
                // tail); one 2769 lands at the shared span, else at
                // the callee error node.
                let args = ctx.args.clone();
                let mut all_errors: Vec<ApplicabilityError> = Vec::new();
                for &candidate in &candidates_for_argument_error {
                    let errors = self
                        .get_signature_applicability_error(
                            node,
                            &args,
                            candidate,
                            RelationKind::Assignable,
                            CheckMode::NORMAL,
                            ApplicabilityMode::Probe,
                        )?
                        .unwrap_or_else(|| {
                            panic!(
                                "No error for 3 or fewer overload signatures @{}",
                                self.binder.source_of_node(node).file_name
                            )
                        });
                    all_errors.extend(errors);
                }
                debug_assert!(
                    !all_errors.is_empty(),
                    "No errors reported for 3 or fewer overload signatures"
                );
                let chain =
                    MessageChain::new(&diagnostics::No_overload_matches_this_call, &[]);
                let shared_span = all_errors
                    .iter()
                    .all(|error| error.span == all_errors[0].span);
                let mut diagnostic = if shared_span {
                    self.diagnostic_at_span(&all_errors[0].span, chain)
                } else {
                    let error_node = self.get_error_node_for_call_node(node);
                    let span = self.diag_span_of_node(error_node);
                    self.diagnostic_at_span(&span, chain)
                };
                diagnostic.related = all_errors
                    .into_iter()
                    .flat_map(|error| error.related)
                    .collect();
                if let Some(related) = self
                    .implementation_success_elaboration(ctx, candidates_for_argument_error[0])?
                {
                    diagnostic.related.push(related);
                }
                self.push_error_diagnostic(diagnostic);
            }
        } else if let Some(candidate) = ctx.candidate_for_argument_arity_error {
            let args = ctx.args.clone();
            let diagnostic = self.get_argument_arity_error(node, &[candidate], &args)?;
            self.push_error_diagnostic(diagnostic);
        } else if let Some(candidate) = ctx.candidate_for_type_argument_error {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            self.check_type_arguments(
                candidate,
                &type_argument_nodes,
                /*report_errors*/ true,
                /*head_message*/ None,
            )?;
        } else {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            let with_correct_type_argument_arity: Vec<SignatureId> = signatures
                .iter()
                .copied()
                .filter(|&s| self.has_correct_type_argument_arity(s, &type_argument_nodes))
                .collect();
            if with_correct_type_argument_arity.is_empty() {
                let diagnostic = self.get_type_argument_arity_error(
                    node,
                    signatures,
                    ctx.type_arguments_array,
                    &type_argument_nodes,
                )?;
                self.push_error_diagnostic(diagnostic);
            } else {
                let args = ctx.args.clone();
                let diagnostic =
                    self.get_argument_arity_error(node, &with_correct_type_argument_arity, &args)?;
                self.push_error_diagnostic(diagnostic);
            }
        }
        Ok(())
    }

    /// addImplementationSuccessElaboration (76744-76762): when the
    /// failed signature's symbol has a body-bearing implementation
    /// declaration whose signature WOULD accept the call, add the 2793
    /// related row. The probe re-runs real argument checks (dedupe
    /// absorbs the duplicates); a containment inside the probe drops
    /// only the related row (attach-only, FN-safe).
    fn implementation_success_elaboration(
        &mut self,
        ctx: &mut ResolveCallCtx,
        failed: SignatureId,
    ) -> CheckResult2<Option<RelatedInfo>> {
        let save_candidates = ctx.candidates_for_argument_error.take();
        let save_arity = ctx.candidate_for_argument_arity_error.take();
        let save_type_argument = ctx.candidate_for_type_argument_error.take();
        let result = (|state: &mut Self| -> CheckResult2<Option<RelatedInfo>> {
            let Some(declaration) = state.signature_of(failed).declaration else {
                return Ok(None);
            };
            let Some(symbol) = state.node_symbol(declaration) else {
                return Ok(None);
            };
            let declarations = state.binder.symbol(symbol).declarations.clone();
            if declarations.len() <= 1 {
                return Ok(None);
            }
            let source = state.binder.source_of_node(declaration);
            let _ = source;
            let impl_decl = declarations.iter().copied().find(|&d| {
                node_util::is_function_like_declaration_kind(state.kind_of(d))
                    && node_util::body_of(state.binder.source_of_node(d), d).is_some()
            });
            let Some(impl_decl) = impl_decl else {
                return Ok(None);
            };
            let candidate = state.get_signature_from_declaration(impl_decl)?;
            let is_single_non_generic =
                state.signature_of(candidate).type_parameters.is_none();
            let mut probe_ctx = ResolveCallCtx {
                node: ctx.node,
                args: ctx.args.clone(),
                type_arguments_array: ctx.type_arguments_array,
                type_argument_nodes: ctx.type_argument_nodes.clone(),
                arg_check_mode: CheckMode::NORMAL,
                candidates: vec![candidate],
                candidates_for_argument_error: None,
                candidate_for_argument_arity_error: None,
                candidate_for_type_argument_error: None,
            };
            let chosen = state.choose_overload(
                &mut probe_ctx,
                RelationKind::Assignable,
                is_single_non_generic,
            )?;
            if chosen.is_some() {
                return Ok(Some(state.related_info_for_node(
                    impl_decl,
                    &diagnostics::The_call_would_have_succeeded_against_this_implementation_but_implementation_signatures_of_overloads_are_not_externally_visible,
                    &[],
                )));
            }
            Ok(None)
        })(self);
        ctx.candidates_for_argument_error = save_candidates;
        ctx.candidate_for_argument_arity_error = save_arity;
        ctx.candidate_for_type_argument_error = save_type_argument;
        match result {
            Ok(related) => Ok(related),
            // Attach-only probe: containment drops the related row.
            Err(_) => Ok(None),
        }
    }

    /// tsc-port: chooseOverload @6.0.3
    /// tsc-hash: f8e61f36d383d1a4c7f036ac29776b3a5e9b119fffd53de9e28d0da96168c5f2
    /// tsc-span: _tsc.js:76763-76869
    ///
    /// M6-STUB SITE #1 (extraction doc §0): a generic candidate
    /// WITHOUT explicit type arguments that survives the loop-head
    /// arity checks would instantiate with inferred types — every
    /// downstream verdict (applicability failure AND success result)
    /// is stub-poisoned, so the whole resolution escapes. tsc's real
    /// no-inference fallback is default → unknown; M6 swaps the escape
    /// for createInferenceContext + inferTypeArguments here.
    fn choose_overload(
        &mut self,
        ctx: &mut ResolveCallCtx,
        relation: RelationKind,
        is_single_non_generic_candidate: bool,
    ) -> CheckResult2<Option<SignatureId>> {
        ctx.candidates_for_argument_error = None;
        ctx.candidate_for_argument_arity_error = None;
        ctx.candidate_for_type_argument_error = None;
        let node = ctx.node;
        if is_single_non_generic_candidate {
            let candidate = ctx.candidates[0];
            let args = ctx.args.clone();
            if !ctx.type_argument_nodes.is_empty()
                || !self.has_correct_arity(node, &args, candidate, false)?
            {
                return Ok(None);
            }
            if self
                .get_signature_applicability_error(
                    node,
                    &args,
                    candidate,
                    relation,
                    CheckMode::NORMAL,
                    ApplicabilityMode::Silent,
                )?
                .is_some()
            {
                ctx.candidates_for_argument_error = Some(vec![candidate]);
                return Ok(None);
            }
            return Ok(Some(candidate));
        }
        for candidate_index in 0..ctx.candidates.len() {
            let candidate = ctx.candidates[candidate_index];
            let args = ctx.args.clone();
            if !self.has_correct_type_argument_arity(candidate, &ctx.type_argument_nodes)
                || !self.has_correct_arity(node, &args, candidate, false)?
            {
                continue;
            }
            let check_candidate: SignatureId;
            if self.signature_of(candidate).type_parameters.is_some() {
                if !ctx.type_argument_nodes.is_empty() {
                    let type_argument_nodes = ctx.type_argument_nodes.clone();
                    let type_argument_types = self.check_type_arguments(
                        candidate,
                        &type_argument_nodes,
                        /*report_errors*/ false,
                        /*head_message*/ None,
                    )?;
                    let Some(type_argument_types) = type_argument_types else {
                        ctx.candidate_for_type_argument_error = Some(candidate);
                        continue;
                    };
                    check_candidate = self.get_signature_instantiation(
                        candidate,
                        Some(&type_argument_types),
                        /*is_javascript*/ false,
                        /*inferred_type_parameters*/ None,
                    )?;
                    // 76819: the non-array-rest re-arity check reads
                    // the INSTANTIATED rest tuple.
                    if self.get_non_array_rest_type(candidate)?.is_some()
                        && !self.has_correct_arity(node, &args, check_candidate, false)?
                    {
                        ctx.candidate_for_argument_arity_error = Some(check_candidate);
                        continue;
                    }
                } else {
                    // M6-stub: inferTypeArguments (75938) + the
                    // inference context (76809-76817).
                    return Err(Unsupported::new(
                        "inferTypeArguments (M6-stub): generic call without explicit type arguments",
                    ));
                }
            } else {
                check_candidate = candidate;
            }
            if self
                .get_signature_applicability_error(
                    node,
                    &args,
                    check_candidate,
                    relation,
                    ctx.arg_check_mode,
                    ApplicabilityMode::Silent,
                )?
                .is_some()
            {
                ctx.candidates_for_argument_error
                    .get_or_insert_with(Vec::new)
                    .push(check_candidate);
                continue;
            }
            if !ctx.arg_check_mode.is_empty() {
                // 76840-76864: the context-sensitive re-run — reset to
                // Normal and re-check applicability. The generic
                // re-infer arm (76842-76848) is M6-dead (stub escapes
                // precede it).
                ctx.arg_check_mode = CheckMode::NORMAL;
                if self
                    .get_signature_applicability_error(
                        node,
                        &args,
                        check_candidate,
                        relation,
                        ctx.arg_check_mode,
                        ApplicabilityMode::Silent,
                    )?
                    .is_some()
                {
                    ctx.candidates_for_argument_error
                        .get_or_insert_with(Vec::new)
                        .push(check_candidate);
                    continue;
                }
            }
            ctx.candidates[candidate_index] = check_candidate;
            return Ok(Some(check_candidate));
        }
        Ok(None)
    }

    // ---- failure candidates ----

    /// tsc-port: getCandidateForOverloadFailure @6.0.3
    /// tsc-hash: adb5aafbe61488c803eae179a53fac1b841d113aaf7957ea811c62d1c654f234
    /// tsc-span: _tsc.js:76871-76875
    ///
    /// checkNodeDeferred ALWAYS — the deferred pass re-checks the raw
    /// arguments with the stashed candidate feeding contextual reads.
    fn get_candidate_for_overload_failure(
        &mut self,
        node: NodeId,
        ctx: &mut ResolveCallCtx,
        check_mode: CheckMode,
    ) -> CheckResult2<SignatureId> {
        debug_assert!(!ctx.candidates.is_empty());
        self.check_node_deferred(node);
        let _ = check_mode;
        let any_generic = ctx
            .candidates
            .iter()
            .any(|&c| self.signature_of(c).type_parameters.is_some());
        if ctx.candidates.len() == 1 || any_generic {
            self.pick_longest_candidate_signature(node, ctx)
        } else {
            self.create_union_of_signatures_for_overload_failure(&ctx.candidates)
        }
    }

    /// tsc-port: pickLongestCandidateSignature @6.0.3
    /// tsc-hash: 4fc7d0044870d548ebaedcda33e0f43d9cb442e80bc9903676738e800d523164
    /// tsc-span: _tsc.js:76924-76935
    ///
    /// M6-STUB SITE #2 (inferSignatureInstantiationForOverloadFailure
    /// 76946-76954): the stub fills default → constraint → unknown per
    /// parameter with NO argument walk. The stash feeds error
    /// selection and the deferred contextual reads only; stub values
    /// that would become observable escape — context-sensitive raw
    /// arguments (their parameters would take stub-typed contextual
    /// assignments) and tuple-rest arity reads (stub-typed counts).
    /// tsc's real fallback is default → unknown; the constraint step
    /// is an M4-only enrichment M6 MUST REMOVE.
    fn pick_longest_candidate_signature(
        &mut self,
        _node: NodeId,
        ctx: &mut ResolveCallCtx,
    ) -> CheckResult2<SignatureId> {
        let args_count = self.apparent_argument_count.unwrap_or(ctx.args.len());
        let best_index = self.get_longest_candidate_index(&ctx.candidates, args_count)?;
        let candidate = ctx.candidates[best_index];
        let Some(type_parameters) = self.signature_of(candidate).type_parameters.clone() else {
            return Ok(candidate);
        };
        let instantiated = if !ctx.type_argument_nodes.is_empty() {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            let type_arguments =
                self.get_type_arguments_from_nodes(&type_argument_nodes, &type_parameters)?;
            self.create_signature_instantiation(candidate, Some(&type_arguments))?
        } else {
            let any_context_sensitive = ctx.args.iter().any(|arg| match arg {
                EffectiveArg::Node(arg_node) => self.is_context_sensitive(*arg_node),
                EffectiveArg::Synthetic { .. } => false,
            });
            if any_context_sensitive {
                return Err(Unsupported::new(
                    "inferSignatureInstantiationForOverloadFailure (M6-stub): context-sensitive arguments would observe stub parameter types",
                ));
            }
            if self.get_non_array_rest_type(candidate)?.is_some() {
                return Err(Unsupported::new(
                    "inferSignatureInstantiationForOverloadFailure (M6-stub): arity selection reads a stub-instantiated rest tuple",
                ));
            }
            let mut stub_types: Vec<TypeId> = Vec::with_capacity(type_parameters.len());
            for &type_parameter in &type_parameters {
                // M6-stub fill: default → constraint → unknownType.
                let ty = match self.get_default_from_type_parameter(type_parameter)? {
                    Some(default) => default,
                    None => self
                        .get_constraint_of_type_parameter(type_parameter)?
                        .unwrap_or(self.tables.intrinsics.unknown),
                };
                stub_types.push(ty);
            }
            self.create_signature_instantiation(candidate, Some(&stub_types))?
        };
        ctx.candidates[best_index] = instantiated;
        Ok(instantiated)
    }

    /// tsc-port: getTypeArgumentsFromNodes @6.0.3
    /// tsc-hash: e42b94a48cb077bb4c85ccc9efd4acbf62ac12a8db1efbdf320901a5d0437865
    /// tsc-span: _tsc.js:76936-76945
    ///
    /// The default → constraint → unknown fill here is tsc's REAL code
    /// for explicit-typearg failure candidates (not the M6 stub);
    /// getDefaultTypeArgumentType(isJs=false) = unknownType.
    fn get_type_arguments_from_nodes(
        &mut self,
        type_argument_nodes: &[NodeId],
        type_parameters: &[TypeId],
    ) -> CheckResult2<Vec<TypeId>> {
        let mut type_arguments: Vec<TypeId> = Vec::with_capacity(type_argument_nodes.len());
        for &node in type_argument_nodes {
            type_arguments.push(self.get_type_from_type_node(node)?);
        }
        while type_arguments.len() > type_parameters.len() {
            type_arguments.pop();
        }
        while type_arguments.len() < type_parameters.len() {
            let type_parameter = type_parameters[type_arguments.len()];
            let ty = match self.get_default_from_type_parameter(type_parameter)? {
                Some(default) => default,
                None => self
                    .get_constraint_of_type_parameter(type_parameter)?
                    .unwrap_or(self.tables.intrinsics.unknown),
            };
            type_arguments.push(ty);
        }
        Ok(type_arguments)
    }

    /// tsc-port: getLongestCandidateIndex @6.0.3
    /// tsc-hash: 6cc04912575b8b07783bb427f4dc10ead8dbe05e659a452d2dfda194d22c6efb
    /// tsc-span: _tsc.js:76956-76971
    fn get_longest_candidate_index(
        &mut self,
        candidates: &[SignatureId],
        args_count: usize,
    ) -> CheckResult2<usize> {
        let mut max_params_index: usize = 0;
        let mut max_params: isize = -1;
        for (i, &candidate) in candidates.iter().enumerate() {
            let param_count = self.get_parameter_count(candidate)?;
            if self.has_effective_rest_parameter(candidate)? || param_count >= args_count {
                return Ok(i);
            }
            if param_count as isize > max_params {
                max_params = param_count as isize;
                max_params_index = i;
            }
        }
        Ok(max_params_index)
    }

    /// tsc-port: createUnionOfSignaturesForOverloadFailure @6.0.3
    /// tsc-hash: 1fe0405cb7d3f33b2f768c339467f0b7db03798810ca3329b03ecb9a780d1c20
    /// tsc-span: _tsc.js:76876-76913
    ///
    /// (getNumNonRestParameters 76914-76917 and the combined-symbol
    /// helpers 76918-76923 folded in.)
    fn create_union_of_signatures_for_overload_failure(
        &mut self,
        candidates: &[SignatureId],
    ) -> CheckResult2<SignatureId> {
        let this_parameters: Vec<SymbolId> = candidates
            .iter()
            .filter_map(|&c| self.signature_of(c).this_parameter)
            .collect();
        let mut this_parameter: Option<SymbolId> = None;
        if !this_parameters.is_empty() {
            let mut types = Vec::with_capacity(this_parameters.len());
            for &parameter in &this_parameters {
                types.push(self.get_type_of_parameter(parameter)?);
            }
            let unioned = self.get_union_type_ex(&types, UnionReduction::Subtype)?;
            this_parameter = Some(self.create_symbol_with_type(this_parameters[0], unioned));
        }
        let num_non_rest: Vec<usize> = candidates
            .iter()
            .map(|&c| {
                let data = self.signature_of(c);
                data.parameters.len()
                    - usize::from(data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER))
            })
            .collect();
        let min_argument_count = candidates
            .iter()
            .map(|&c| self.signature_of(c).min_argument_count)
            .min()
            .expect("non-empty candidates");
        let max_non_rest_param = num_non_rest.iter().copied().max().expect("non-empty");
        let mut parameters: Vec<SymbolId> = Vec::with_capacity(max_non_rest_param);
        for i in 0..max_non_rest_param {
            let symbols: Vec<SymbolId> = candidates
                .iter()
                .filter_map(|&s| {
                    let data = self.signature_of(s);
                    if data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
                        if i < data.parameters.len() - 1 {
                            Some(data.parameters[i])
                        } else {
                            data.parameters.last().copied()
                        }
                    } else if i < data.parameters.len() {
                        Some(data.parameters[i])
                    } else {
                        None
                    }
                })
                .collect();
            debug_assert!(!symbols.is_empty());
            let mut types: Vec<TypeId> = Vec::new();
            for &candidate in candidates {
                if let Some(ty) = self.try_get_type_at_position(candidate, i)? {
                    types.push(ty);
                }
            }
            let unioned = self.get_union_type_ex(&types, UnionReduction::Subtype)?;
            parameters.push(self.create_symbol_with_type(symbols[0], unioned));
        }
        let rest_parameter_symbols: Vec<SymbolId> = candidates
            .iter()
            .filter_map(|&c| {
                let data = self.signature_of(c);
                if data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
                    data.parameters.last().copied()
                } else {
                    None
                }
            })
            .collect();
        let mut flags = SignatureFlags::IS_SIGNATURE_CANDIDATE_FOR_OVERLOAD_FAILURE;
        if !rest_parameter_symbols.is_empty() {
            let mut rest_types: Vec<TypeId> = Vec::new();
            for &candidate in candidates {
                if let Some(rest) = self.try_get_rest_type_of_signature(candidate)? {
                    rest_types.push(rest);
                }
            }
            let unioned = self.get_union_type_ex(&rest_types, UnionReduction::Subtype)?;
            let array = self.create_array_type(unioned, /*readonly*/ false)?;
            parameters.push(self.create_symbol_with_type(rest_parameter_symbols[0], array));
            flags = SignatureFlags::from_bits(
                flags.bits() | SignatureFlags::HAS_REST_PARAMETER.bits(),
            );
        }
        if candidates
            .iter()
            .any(|&c| self.signature_of(c).flags.intersects(SignatureFlags::HAS_LITERAL_TYPES))
        {
            flags = SignatureFlags::from_bits(
                flags.bits() | SignatureFlags::HAS_LITERAL_TYPES.bits(),
            );
        }
        let mut return_types: Vec<TypeId> = Vec::with_capacity(candidates.len());
        for &candidate in candidates {
            return_types.push(self.get_return_type_of_signature(candidate)?);
        }
        let return_type =
            self.get_intersection_type(&return_types, tsrs2_types::IntersectionFlags::NONE)?;
        let first = self.signature_of(candidates[0]).clone();
        Ok(self.alloc_signature(Signature {
            declaration: first.declaration,
            flags,
            type_parameters: None,
            parameters,
            this_parameter,
            min_argument_count,
            resolved_return_type: LinkSlot::Resolved(return_type),
            from_method: first.from_method,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
        }))
    }

    /// tsc-port: tryGetRestTypeOfSignature @6.0.3
    /// tsc-hash: 0be56e511e900fd0aa622d918e53b2c5e132254bf3b61e8ad25be72950ff7728
    /// tsc-span: _tsc.js:59878-59885
    fn try_get_rest_type_of_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypeId>> {
        let data = self.signature_of(signature);
        if !data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
            return Ok(None);
        }
        let rest_parameter = *data
            .parameters
            .last()
            .expect("rest-parameter signatures have parameters");
        let sig_rest_type = self.get_type_of_symbol(rest_parameter)?;
        let rest_type = if self.tables.is_tuple_type(sig_rest_type) {
            match self.get_rest_type_of_tuple_type(sig_rest_type)? {
                Some(rest) => rest,
                None => return Ok(None),
            }
        } else {
            sig_rest_type
        };
        self.get_index_type_of_type(rest_type, self.tables.intrinsics.number)
    }

    // ---- arity errors ----

    /// tsc-port: isPromiseResolveArityError @6.0.3
    /// tsc-hash: 6bba0fd86e72d239399c30338e816833a9ed67ed3cff895e44386acae0c8d48e
    /// tsc-span: _tsc.js:76407-76433
    ///
    /// The callee resolves to a parameter of a function-expression
    /// directly under `new <globalPromiseSymbol>`; getSymbolAtLocation
    /// on the constructor identifier reduces to the same resolveName
    /// probe for the identifier-callee shape this predicate demands.
    fn is_promise_resolve_arity_error(&mut self, node: NodeId) -> CheckResult2<bool> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(callee) = data.expression else {
            return Ok(false);
        };
        if self.kind_of(callee) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let callee_text = match self.identifier_text_of(callee) {
            Some(text) => text.to_owned(),
            None => return Ok(false),
        };
        let symbol = self.resolve_name(
            Some(callee),
            &callee_text,
            SymbolFlags::VALUE,
            /*name_not_found_message*/ None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
        );
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        let Some(decl) = self.binder.symbol(symbol).value_declaration else {
            return Ok(false);
        };
        if self.kind_of(decl) != SyntaxKind::Parameter {
            return Ok(false);
        }
        let Some(func) = self.parent_of(decl) else {
            return Ok(false);
        };
        if !matches!(
            self.kind_of(func),
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
        ) {
            return Ok(false);
        }
        let Some(new_expr) = self.parent_of(func) else {
            return Ok(false);
        };
        let NodeData::NewExpression(new_data) = self.data_of(new_expr) else {
            return Ok(false);
        };
        let Some(ctor) = new_data.expression else {
            return Ok(false);
        };
        if self.kind_of(ctor) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let Some(global_promise) = self.get_global_promise_constructor_symbol(false)? else {
            return Ok(false);
        };
        let ctor_text = match self.identifier_text_of(ctor) {
            Some(text) => text.to_owned(),
            None => return Ok(false),
        };
        let ctor_symbol = self.resolve_name(
            Some(ctor),
            &ctor_text,
            SymbolFlags::VALUE,
            None,
            false,
            false,
        );
        Ok(ctor_symbol == Some(global_promise))
    }

    /// tsc-port: getArgumentArityError @6.0.3
    /// tsc-hash: 7584a2739d127f4143461c80dcc282d90a4308c3b7effba7add87f93001a2007
    /// tsc-span: _tsc.js:76434-76520
    ///
    /// Count-only payloads — the whole band is display-free. Decorator
    /// flavors are 5.8; headMessage chains 5.7b (both unreachable
    /// here). The JS Promise-hint flavor is JS-file-gated.
    fn get_argument_arity_error(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[EffectiveArg],
    ) -> CheckResult2<Diagnostic> {
        if let Some(spread_index) = self.get_spread_argument_index(args) {
            let span = self.diag_span_of_effective_arg(node, &args[spread_index]);
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::A_spread_argument_must_either_have_a_tuple_type_or_be_passed_to_a_rest_parameter,
                    &[],
                ),
            ));
        }
        let mut min = usize::MAX;
        let mut max: usize = 0;
        let mut max_below: Option<usize> = None;
        let mut min_above: Option<usize> = None;
        let mut closest_signature: Option<SignatureId> = None;
        for &signature in signatures {
            let min_parameter = self.get_min_argument_count(signature)?;
            let max_parameter = self.get_parameter_count(signature)?;
            if min_parameter < min {
                min = min_parameter;
                closest_signature = Some(signature);
            }
            max = std::cmp::max(max, max_parameter);
            if min_parameter < args.len()
                && max_below.map_or(true, |below| min_parameter > below)
            {
                max_below = Some(min_parameter);
            }
            if args.len() < max_parameter
                && min_above.map_or(true, |above| max_parameter < above)
            {
                min_above = Some(max_parameter);
            }
        }
        let mut has_rest_parameter = false;
        for &signature in signatures {
            if self.has_effective_rest_parameter(signature)? {
                has_rest_parameter = true;
                break;
            }
        }
        let parameter_range = if has_rest_parameter {
            min.to_string()
        } else if min < max {
            format!("{min}-{max}")
        } else {
            min.to_string()
        };
        let is_void_promise_error = !has_rest_parameter
            && parameter_range == "1"
            && args.is_empty()
            && self.is_promise_resolve_arity_error(node)?;
        let error_message: &'static DiagnosticMessage = if has_rest_parameter {
            &diagnostics::Expected_at_least_0_arguments_but_got_1
        } else if is_void_promise_error {
            &diagnostics::Expected_0_arguments_but_got_1_Did_you_forget_to_include_void_in_your_type_argument_to_Promise
        } else {
            &diagnostics::Expected_0_arguments_but_got_1
        };
        let arg_count_text = args.len().to_string();
        if min < args.len() && args.len() < max {
            // 76463-76476: between the overload boundaries.
            let span = self.diag_span_for_call_node(node);
            let max_below = max_below.expect("between-range implies a below bound");
            let min_above = min_above.expect("between-range implies an above bound");
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::No_overload_expects_0_arguments_but_overloads_do_exist_that_expect_either_1_or_2_arguments,
                    &[
                        arg_count_text,
                        max_below.to_string(),
                        min_above.to_string(),
                    ],
                ),
            ));
        }
        if args.len() < min {
            let span = self.diag_span_for_call_node(node);
            let mut diagnostic = self.diagnostic_at_span(
                &span,
                MessageChain::new(error_message, &[parameter_range, arg_count_text]),
            );
            // 76492-76497: the "argument not provided" related row on
            // the closest signature's missing parameter.
            if let Some(declaration) =
                closest_signature.and_then(|s| self.signature_of(s).declaration)
            {
                let has_this = closest_signature
                    .is_some_and(|s| self.signature_of(s).this_parameter.is_some());
                let parameter_index = if has_this { args.len() + 1 } else { args.len() };
                let parameters = match self.data_of(declaration) {
                    NodeData::FunctionDeclaration(data) => data.parameters,
                    NodeData::FunctionExpression(data) => data.parameters,
                    NodeData::ArrowFunction(data) => data.parameters,
                    NodeData::MethodDeclaration(data) => data.parameters,
                    NodeData::MethodSignature(data) => data.parameters,
                    NodeData::CallSignature(data) => data.parameters,
                    NodeData::ConstructSignature(data) => data.parameters,
                    NodeData::FunctionType(data) => data.parameters,
                    NodeData::ConstructorType(data) => data.parameters,
                    NodeData::Constructor(data) => data.parameters,
                    _ => None,
                };
                let parameter = self.nodes_of(parameters).get(parameter_index).copied();
                if let Some(parameter) = parameter {
                    let related = self.argument_not_provided_related(parameter)?;
                    diagnostic.related.push(related);
                }
            }
            return Ok(diagnostic);
        }
        // 76499-76519: over max — the excess-args range (end==pos bump).
        let pos = self.effective_arg_pos(&args[max]);
        let mut end = self.effective_arg_end(&args[args.len() - 1]);
        if end == pos {
            end += 1;
        }
        let span = self.diag_span_of_byte_range(node, pos, end);
        Ok(self.diagnostic_at_span(
            &span,
            MessageChain::new(error_message, &[parameter_range, arg_count_text]),
        ))
    }

    /// The 76494 related-row selection: binding pattern / rest / named.
    fn argument_not_provided_related(&mut self, parameter: NodeId) -> CheckResult2<RelatedInfo> {
        let NodeData::Parameter(data) = self.data_of(parameter) else {
            unreachable!("signature declarations carry parameter nodes");
        };
        let name = data.name;
        let is_rest = data.dot_dot_dot_token.is_some();
        let name_kind = name.map(|name| self.kind_of(name));
        if matches!(
            name_kind,
            Some(SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern)
        ) {
            return Ok(self.related_info_for_node(
                parameter,
                &diagnostics::An_argument_matching_this_binding_pattern_was_not_provided,
                &[],
            ));
        }
        let name_text = name
            .and_then(|name| self.identifier_text_of(name))
            .map(str::to_owned);
        if is_rest {
            let text = name_text.unwrap_or_default();
            return Ok(self.related_info_for_node(
                parameter,
                &diagnostics::Arguments_for_the_rest_parameter_0_were_not_provided,
                &[&text],
            ));
        }
        let text = name_text.unwrap_or_default();
        Ok(self.related_info_for_node(
            parameter,
            &diagnostics::An_argument_for_0_was_not_provided,
            &[&text],
        ))
    }

    /// tsc-port: getTypeArgumentArityError @6.0.3
    /// tsc-hash: 6ed32b61094692b28f2f33ddd7c2c03c8a86d35230e4008c87ac294ac74100a8
    /// tsc-span: _tsc.js:76521-76578
    ///
    /// headMessage chains are 5.7b/5.8 (unreachable here). The span is
    /// the typeArguments node-array range in every arm.
    fn get_type_argument_arity_error(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        type_arguments_array: Option<NodeArrayId>,
        type_argument_nodes: &[NodeId],
    ) -> CheckResult2<Diagnostic> {
        let arg_count = type_argument_nodes.len();
        let span = match type_arguments_array {
            Some(array) => {
                let source = self.binder.source_of_node(node);
                let array = source.arena.node_array(array);
                self.diag_span_of_byte_range(node, array.pos, array.end)
            }
            None => self.diag_span_of_node(node),
        };
        if signatures.len() == 1 {
            let signature = signatures[0];
            let type_parameters = self.signature_of(signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(type_parameters.as_deref());
            let max = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
            let range = if min < max {
                format!("{min}-{max}")
            } else {
                min.to_string()
            };
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::Expected_0_type_arguments_but_got_1,
                    &[range, arg_count.to_string()],
                ),
            ));
        }
        let mut below_arg_count: Option<usize> = None;
        let mut above_arg_count: Option<usize> = None;
        for &signature in signatures {
            let type_parameters = self.signature_of(signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(type_parameters.as_deref());
            let max = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
            if min > arg_count {
                above_arg_count = Some(above_arg_count.map_or(min, |above| above.min(min)));
            } else if max < arg_count {
                below_arg_count = Some(below_arg_count.map_or(max, |below| below.max(max)));
            }
        }
        if let (Some(below), Some(above)) = (below_arg_count, above_arg_count) {
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::No_overload_expects_0_type_arguments_but_overloads_do_exist_that_expect_either_1_or_2_type_arguments,
                    &[arg_count.to_string(), below.to_string(), above.to_string()],
                ),
            ));
        }
        let boundary = below_arg_count.or(above_arg_count).unwrap_or(0);
        Ok(self.diagnostic_at_span(
            &span,
            MessageChain::new(
                &diagnostics::Expected_0_type_arguments_but_got_1,
                &[boundary.to_string(), arg_count.to_string()],
            ),
        ))
    }

    // ---- per-kind resolvers ----

    /// tsc-port: isUntypedFunctionCall @6.0.3
    /// tsc-hash: 2353c2c317bde5a830b031cc38da3caee3d391b53a8e95bae70d601f17a12321
    /// tsc-span: _tsc.js:77052-77054
    fn is_untyped_function_call(
        &mut self,
        func_type: TypeId,
        apparent_func_type: TypeId,
        num_call_signatures: usize,
        num_construct_signatures: usize,
    ) -> CheckResult2<bool> {
        if self.tables.flags_of(func_type).intersects(TypeFlags::ANY) {
            return Ok(true);
        }
        if self
            .tables
            .flags_of(apparent_func_type)
            .intersects(TypeFlags::ANY)
            && self
                .tables
                .flags_of(func_type)
                .intersects(TypeFlags::TYPE_PARAMETER)
        {
            return Ok(true);
        }
        if num_call_signatures != 0 || num_construct_signatures != 0 {
            return Ok(false);
        }
        if self
            .tables
            .flags_of(apparent_func_type)
            .intersects(TypeFlags::UNION)
        {
            return Ok(false);
        }
        let reduced = self.get_reduced_type(apparent_func_type)?;
        if self.tables.flags_of(reduced).intersects(TypeFlags::NEVER) {
            return Ok(false);
        }
        let global_function = self.global_function_type()?;
        self.is_type_assignable_to(func_type, global_function)
    }

    /// tsc-port: invocationError @6.0.3 (invocationErrorDetails folded in)
    /// tsc-hash: f2d2133394f805817e33a6c4b1534917ab876d99027c097b8c1f6d172778d90b
    /// tsc-span: _tsc.js:77167-77247
    ///
    /// The union constituent rows and the typeToString chain details
    /// all elide (T2 curtain) — the emitted HEAD (2349/2351, or the
    /// 6234 get-accessor flavor) is display-free, so the band never
    /// escapes. Related rows: the await hint (2773); the
    /// invocationErrorRecovery 7038 rides the unmodeled
    /// originatingImport link (absent = attach-only, safe).
    fn invocation_error(
        &mut self,
        error_target: NodeId,
        apparent_type: TypeId,
        kind: SignatureKind,
        related_information: Option<RelatedInfo>,
    ) -> CheckResult2<()> {
        let is_call = kind == SignatureKind::Call;
        let awaited = self.get_awaited_type_probe(apparent_type)?;
        let maybe_missing_await = match awaited {
            Some(awaited) => !self.get_signatures_of_type(awaited, kind)?.is_empty(),
            None => false,
        };
        // 77222-77228: the zero-arg get-accessor head flavor.
        let mut head: &'static DiagnosticMessage = if is_call {
            &diagnostics::This_expression_is_not_callable
        } else {
            &diagnostics::This_expression_is_not_constructable
        };
        let parent = self.parent_of(error_target);
        let parent_call_args = parent.and_then(|parent| match self.data_of(parent) {
            NodeData::CallExpression(data) => Some(self.nodes_of(data.arguments).len()),
            _ => None,
        });
        if parent_call_args == Some(0) {
            if let LinkSlot::Resolved(resolved_symbol) =
                self.links.node(error_target).resolved_symbol
            {
                if self
                    .binder
                    .symbol(resolved_symbol)
                    .flags
                    .intersects(SymbolFlags::GET_ACCESSOR)
                {
                    head = &diagnostics::This_expression_is_not_callable_because_it_is_a_get_accessor_Did_you_mean_to_use_it_without;
                }
            }
        }
        // 77240-77244: the span override inside call parents.
        let span = if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::CallExpression)
        {
            self.diag_span_for_call_node(parent.expect("checked above"))
        } else {
            self.diag_span_of_node(error_target)
        };
        let mut diagnostic = self.diagnostic_at_span(&span, MessageChain::new(head, &[]));
        if maybe_missing_await {
            diagnostic.related.push(self.related_info_for_node(
                error_target,
                &diagnostics::Did_you_forget_to_use_await,
                &[],
            ));
        }
        if let Some(related) = related_information {
            diagnostic.related.push(related);
        }
        self.push_error_diagnostic(diagnostic);
        Ok(())
    }

    /// tsc-port: resolveCallExpression @6.0.3
    /// tsc-hash: 80e582aa9064a2e37878a85900269bfeb17fa3b61b12cb8e5697a910d13c0b73
    /// tsc-span: _tsc.js:76972-77048
    fn resolve_call_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<SignatureId> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.ok_or_else(|| {
            Unsupported::new("call without a callee (parse recovery)")
        })?;
        let type_arguments = data.type_arguments;
        if self.kind_of(expression) == SyntaxKind::SuperKeyword {
            // 76973-76989: the super() arm.
            let super_type = self.check_super_expression(expression)?;
            if self.tables.flags_of(super_type).intersects(TypeFlags::ANY) {
                let arguments = match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    _ => None,
                };
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
                return Ok(self.any_signature);
            }
            if super_type != self.tables.intrinsics.error {
                // getEffectiveBaseTypeNode + getInstantiatedConstructors
                // ForTypeArguments needs constructor-body forcing.
                return Err(Unsupported::new("super base constructors (5.8)"));
            }
            return self.resolve_untyped_call(node);
        }
        let mut func_type = self.check_expression(expression, CheckMode::NORMAL)?;
        // 76990-76998: call-chain flags.
        let source = self.binder.source_of_node(node);
        let call_chain_flags = if node_util::is_optional_chain(source, node) {
            let non_optional_type = self.get_optional_expression_type(func_type, expression)?;
            if non_optional_type == func_type {
                SignatureFlags::NONE
            } else {
                let flags = if node_util::is_outermost_optional_chain(
                    self.binder.source_of_node(node),
                    node,
                ) {
                    SignatureFlags::IS_OUTER_CALL_CHAIN
                } else {
                    SignatureFlags::IS_INNER_CALL_CHAIN
                };
                func_type = non_optional_type;
                flags
            }
        } else {
            SignatureFlags::NONE
        };
        let func_type = self.check_non_null_type_with_reporter(
            func_type,
            expression,
            Self::report_cannot_invoke_possibly_null_or_undefined_error,
        )?;
        if func_type == self.tables.intrinsics.silent_never {
            return Ok(self.silent_never_signature);
        }
        let apparent_type = self.get_apparent_type(func_type)?;
        if apparent_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        let call_signatures = self.get_signatures_of_type(apparent_type, SignatureKind::Call)?;
        let num_construct_signatures = self
            .get_signatures_of_type(apparent_type, SignatureKind::Construct)?
            .len();
        if self.is_untyped_function_call(
            func_type,
            apparent_type,
            call_signatures.len(),
            num_construct_signatures,
        )? {
            // 77014-77016: 2347 on non-error targets with typeArguments.
            if func_type != self.tables.intrinsics.error && type_arguments.is_some() {
                self.error_at(
                    Some(node),
                    &diagnostics::Untyped_function_calls_may_not_accept_type_arguments,
                    &[],
                );
            }
            return self.resolve_untyped_call(node);
        }
        if call_signatures.is_empty() {
            if num_construct_signatures != 0 {
                let display = self.type_to_string_slice(func_type)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Value_of_type_0_is_not_callable_Did_you_mean_to_include_new,
                    &[&display],
                );
            } else {
                // 77023-77034: the missing-semicolon hint on a
                // single-argument call whose argument opens on a new
                // line.
                let mut related_information: Option<RelatedInfo> = None;
                let arguments = self.nodes_of(match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    _ => None,
                });
                if arguments.len() == 1 {
                    let source = self.binder.source_of_node(node);
                    let callee_end = source.arena.node(expression).end as usize;
                    if line_break_precedes_next_token(&source.text, callee_end) {
                        related_information = Some(self.related_info_for_node(
                            expression,
                            &diagnostics::Are_you_missing_a_semicolon,
                            &[],
                        ));
                    }
                }
                self.invocation_error(
                    expression,
                    apparent_type,
                    SignatureKind::Call,
                    related_information,
                )?;
            }
            return self.resolve_error_call(node);
        }
        // 77039-77042: the SkipGenericFunctions arm is M6-dead (the
        // CheckMode audit proves no producer until inference lands).
        if check_mode.intersects(CheckMode::SKIP_GENERIC_FUNCTIONS) {
            unreachable!("SkipGenericFunctions has no producer until M6 (CheckMode audit)");
        }
        // 77043-77046: the JSDoc @class arm is JS-file-gated.
        self.resolve_call(node, &call_signatures, check_mode, call_chain_flags, None)
    }

    /// tsc-port: resolveNewExpression @6.0.3
    /// tsc-hash: 1d3882b681eb1a6defdf1901381e33d6091c5bfc756487595475c65db0511b41
    /// tsc-span: _tsc.js:77055-77101
    ///
    /// The 2350/2679 tail is dead under the strict default but live
    /// under noImplicitAny:false directives.
    fn resolve_new_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<SignatureId> {
        let NodeData::NewExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.ok_or_else(|| {
            Unsupported::new("new without a callee (parse recovery)")
        })?;
        let type_arguments = data.type_arguments;
        let expression_type = self.check_non_null_expression(expression)?;
        if expression_type == self.tables.intrinsics.silent_never {
            return Ok(self.silent_never_signature);
        }
        let expression_type = self.get_apparent_type(expression_type)?;
        if expression_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        if self
            .tables
            .flags_of(expression_type)
            .intersects(TypeFlags::ANY)
        {
            if type_arguments.is_some() {
                self.error_at(
                    Some(node),
                    &diagnostics::Untyped_function_calls_may_not_accept_type_arguments,
                    &[],
                );
            }
            return self.resolve_untyped_call(node);
        }
        let construct_signatures =
            self.get_signatures_of_type(expression_type, SignatureKind::Construct)?;
        if !construct_signatures.is_empty() {
            if !self.is_constructor_accessible(node, construct_signatures[0])? {
                return self.resolve_error_call(node);
            }
            // 77075-77083: abstract construct signatures and abstract
            // class modifiers.
            let mut some_abstract = false;
            for &signature in &construct_signatures {
                if self.some_signature_has_flags(signature, SignatureFlags::ABSTRACT) {
                    some_abstract = true;
                    break;
                }
            }
            if some_abstract {
                self.error_at(
                    Some(node),
                    &diagnostics::Cannot_create_an_instance_of_an_abstract_class,
                    &[],
                );
                return self.resolve_error_call(node);
            }
            let value_decl = self
                .tables
                .type_of(expression_type)
                .symbol
                .and_then(|symbol| self.get_class_like_declaration_of_symbol(symbol));
            if let Some(value_decl) = value_decl {
                let source = self.binder.source_of_node(value_decl);
                if node_util::has_syntactic_modifier(source, value_decl, ModifierFlags::ABSTRACT) {
                    self.error_at(
                        Some(node),
                        &diagnostics::Cannot_create_an_instance_of_an_abstract_class,
                        &[],
                    );
                    return self.resolve_error_call(node);
                }
            }
            return self.resolve_call(
                node,
                &construct_signatures,
                check_mode,
                SignatureFlags::NONE,
                None,
            );
        }
        let call_signatures = self.get_signatures_of_type(expression_type, SignatureKind::Call)?;
        if !call_signatures.is_empty() {
            let signature = self.resolve_call(
                node,
                &call_signatures,
                check_mode,
                SignatureFlags::NONE,
                None,
            )?;
            if !self
                .options
                .strict_option_value(self.options.no_implicit_any)
            {
                let declaration = self.signature_of(signature).declaration;
                if let Some(declaration) = declaration {
                    if self.is_in_js_file(declaration) {
                        // isJSConstructor consults JS expando members —
                        // the JS band; guessing either way mis-codes
                        // (conformance FP: salsa inferring fixtures).
                        return Err(Unsupported::new(
                            "isJSConstructor probe on a JS declaration (JS band)",
                        ));
                    }
                    let return_type = self.get_return_type_of_signature(signature)?;
                    if return_type == self.tables.intrinsics.never {
                        // [FLOW M5] functionHasImplicitReturn is the
                        // stub-false face: a no-return body computes
                        // `never` where tsc's reachability gives
                        // `void` — the 2350 verdict hinges on it
                        // (conformance FP: inferringClassMembers-
                        // FromAssignments8).
                        return Err(Unsupported::new(
                            "[FLOW M5] functionHasImplicitReturn stub (never-vs-void return under the 2350 gate)",
                        ));
                    }
                    if return_type != self.tables.intrinsics.void {
                        self.error_at(
                            Some(node),
                            &diagnostics::Only_a_void_function_can_be_called_with_the_new_keyword,
                            &[],
                        );
                    }
                }
                if self.get_this_type_of_signature(signature)?
                    == Some(self.tables.intrinsics.void)
                {
                    self.error_at(
                        Some(node),
                        &diagnostics::A_function_that_is_called_with_the_new_keyword_cannot_have_a_this_type_that_is_void,
                        &[],
                    );
                }
            }
            return Ok(signature);
        }
        self.invocation_error(expression, expression_type, SignatureKind::Construct, None)?;
        self.resolve_error_call(node)
    }

    /// tsc-port: someSignature @6.0.3
    /// tsc-hash: a879e6e70ac9beeb2da83e2e0dbc48b5ec7df38c5f6acaab206b517a080294c7
    /// tsc-span: _tsc.js:77102-77107
    fn some_signature_has_flags(&self, signature: SignatureId, flags: SignatureFlags) -> bool {
        let data = self.signature_of(signature);
        if data.composite_kind == Some(TypeFlags::UNION) {
            if let Some(composite) = data.composite_signatures.clone() {
                return composite
                    .iter()
                    .any(|&member| self.some_signature_has_flags(member, flags));
            }
        }
        data.flags.intersects(flags)
    }

    /// tsc-port: typeHasProtectedAccessibleBase @6.0.3
    /// tsc-hash: 16b2b29a9deaee99d4aada788ecf55b7eecdd3c3d5a814fd4268b565c6291703
    /// tsc-span: _tsc.js:77108-77137
    fn type_has_protected_accessible_base(
        &mut self,
        target: SymbolId,
        ty: TypeId,
    ) -> CheckResult2<bool> {
        let base_types = self.get_base_types(ty)?;
        if base_types.is_empty() {
            return Ok(false);
        }
        let first_base = base_types[0];
        if self
            .tables
            .flags_of(first_base)
            .intersects(TypeFlags::INTERSECTION)
        {
            let types = match &self.tables.type_of(first_base).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies payload"),
            };
            let mixin_flags = self.find_mixins(&types)?;
            for (i, &member) in types.iter().enumerate() {
                if mixin_flags[i] {
                    continue;
                }
                if self
                    .tables
                    .object_flags_of(member)
                    .intersects(tsrs2_types::ObjectFlags::CLASS | tsrs2_types::ObjectFlags::INTERFACE)
                {
                    if self.tables.type_of(member).symbol == Some(target) {
                        return Ok(true);
                    }
                    if self.type_has_protected_accessible_base(target, member)? {
                        return Ok(true);
                    }
                }
            }
            return Ok(false);
        }
        if self.tables.type_of(first_base).symbol == Some(target) {
            return Ok(true);
        }
        self.type_has_protected_accessible_base(target, first_base)
    }

    /// tsc-port: isConstructorAccessible @6.0.3
    /// tsc-hash: e7b60027f1bf535adc73a98a8f9e83b7cab35f1ca3a39b39b602b55c6db52baf
    /// tsc-span: _tsc.js:77138-77166
    fn is_constructor_accessible(
        &mut self,
        node: NodeId,
        signature: SignatureId,
    ) -> CheckResult2<bool> {
        let Some(declaration) = self.signature_of(signature).declaration else {
            return Ok(true);
        };
        let source = self.binder.source_of_node(declaration);
        let modifiers = ModifierFlags::from_bits(
            node_util::get_combined_modifier_flags(source, declaration).bits()
                & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits(),
        );
        if modifiers == ModifierFlags::NONE
            || self.kind_of(declaration) != SyntaxKind::Constructor
        {
            return Ok(true);
        }
        let class_declaration = self
            .parent_of(declaration)
            .ok_or_else(|| Unsupported::new("constructor without a class (parse recovery)"))?;
        let class_symbol = self.get_symbol_of_declaration(class_declaration)?;
        let declaring_class_declaration = self.get_class_like_declaration_of_symbol(class_symbol);
        let declaring_class = self.get_declared_type_of_class_or_interface(class_symbol)?;
        if !self.is_node_within_class(node, declaring_class_declaration) {
            let containing_class = self.get_containing_class_of(node);
            if let Some(containing_class) = containing_class {
                if modifiers.intersects(ModifierFlags::PROTECTED) {
                    let containing_symbol = self.get_symbol_of_declaration(containing_class)?;
                    let containing_type =
                        self.get_declared_type_of_class_or_interface(containing_symbol)?;
                    if self.type_has_protected_accessible_base(class_symbol, containing_type)? {
                        return Ok(true);
                    }
                }
            }
            if modifiers.intersects(ModifierFlags::PRIVATE) {
                let display = self.type_to_string_slice(declaring_class)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Constructor_of_class_0_is_private_and_only_accessible_within_the_class_declaration,
                    &[&display],
                );
            }
            if modifiers.intersects(ModifierFlags::PROTECTED) {
                let display = self.type_to_string_slice(declaring_class)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Constructor_of_class_0_is_protected_and_only_accessible_within_the_class_declaration,
                    &[&display],
                );
            }
            return Ok(false);
        }
        Ok(true)
    }

    // ---- dispatch + links protocol ----

    /// tsc-port: resolveSignature @6.0.3
    /// tsc-hash: 76619800b60dc3d6783ffd65d95f10e7eb835be6e3f4ea709139adedbf508a9a
    /// tsc-span: _tsc.js:77472-77490
    fn resolve_signature_dispatch(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<SignatureId> {
        match self.kind_of(node) {
            SyntaxKind::CallExpression => self.resolve_call_expression(node, check_mode),
            SyntaxKind::NewExpression => self.resolve_new_expression(node, check_mode),
            SyntaxKind::TaggedTemplateExpression => Err(Unsupported::new(
                "resolveTaggedTemplateExpression (5.7b)",
            )),
            SyntaxKind::Decorator => Err(Unsupported::new("resolveDecorator (5.8)")),
            SyntaxKind::JsxOpeningFragment
            | SyntaxKind::JsxOpeningElement
            | SyntaxKind::JsxSelfClosingElement => Err(Unsupported::new(
                "resolveJsxOpeningLikeElement (5.7c)",
            )),
            SyntaxKind::BinaryExpression => Err(Unsupported::new(
                "resolveInstanceofExpression (5.7b)",
            )),
            _ => unreachable!("Branch in 'resolveSignature' should be unreachable."),
        }
    }

    /// tsc-port: getResolvedSignature @6.0.3
    /// tsc-hash: 6a0c3093b217f129ec9c4778d89b3a819996877ac666147b4eb6521ad514fd66
    /// tsc-span: _tsc.js:77491-77508
    ///
    /// candidatesOutArray is LSP-only (always None): the cached
    /// early-return needs no re-run arm. flowLoopStart == flowLoopCount
    /// (both 0) until M5 — the final write always takes the result
    /// [FLOW M5]. An Unsupported unwind reverts the sentinel THIS
    /// frame wrote so later queries re-resolve (tsc has no failure
    /// channel here); Resolved stashes survive as real memos.
    pub(crate) fn get_resolved_signature(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<SignatureId> {
        let cached = self.links.node(node).resolved_signature.clone();
        if let LinkSlot::Resolved(cached) = cached {
            return Ok(cached);
        }
        let save_resolution_start = self.resolution_start;
        let wrote_sentinel = matches!(cached, LinkSlot::Vacant);
        if wrote_sentinel {
            self.resolution_start = self.resolution_targets.len();
        }
        self.links.set_node_resolved_signature_call_protocol(
            self.speculation_depth,
            node,
            LinkSlot::Resolving,
        );
        let result = self.resolve_signature_dispatch(node, check_mode);
        self.resolution_start = save_resolution_start;
        match result {
            Ok(result) => {
                debug_assert_eq!(self.flow_loop_start, self.flow_loop_count, "[FLOW M5]");
                self.links.set_node_resolved_signature_call_protocol(
                    self.speculation_depth,
                    node,
                    LinkSlot::Resolved(result),
                );
                Ok(result)
            }
            Err(err) => {
                if wrote_sentinel {
                    self.links.revert_node_resolved_signature_call(node);
                }
                Err(err)
            }
        }
    }

    // ---- the checkCallExpression worker ----

    /// tsc-port: checkCallExpression @6.0.3
    /// tsc-hash: 3459b258ce93da62aaf8212b10d3765e2f130715cb86f663d60d438cecfb09a1
    /// tsc-span: _tsc.js:77607-77660
    ///
    /// Serves Call AND New (tsc dispatches both here).
    /// checkDeprecatedSignature is a no-op: the Deprecated node flag
    /// only ever comes from JSDoc `@deprecated` parsing (unmodeled).
    /// The void-return type-predicate assertion band (2775/2776) is
    /// provably dead — signature type predicates are unmodeled at M4
    /// (predicate-shaped annotations already unwind in the return-type
    /// resolver); the arm ports with M5's getEffectsSignature work.
    /// JS arms (require, expando) are JS-file-gated.
    pub(crate) fn check_call_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let (type_arguments, expression) = match self.data_of(node) {
            NodeData::CallExpression(data) => (data.type_arguments, data.expression),
            NodeData::NewExpression(data) => (data.type_arguments, data.expression),
            _ => unreachable!("checkCallExpression serves call/new"),
        };
        self.check_grammar_type_arguments(node, type_arguments);
        let signature = self.get_resolved_signature(node, check_mode)?;
        if signature == self.resolving_signature {
            // 77616-77618: M6-dead (the SkipGenericFunctions arm is the
            // only producer).
            return Ok(self.tables.intrinsics.silent_never);
        }
        if expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::SuperKeyword) {
            return Ok(self.tables.intrinsics.void);
        }
        if self.kind_of(node) == SyntaxKind::NewExpression {
            // 77623-77631: a `new` that resolved through call
            // signatures — 7009 under noImplicitAny, anyType result.
            let declaration = self.signature_of(signature).declaration;
            if let Some(declaration) = declaration {
                if !matches!(
                    self.kind_of(declaration),
                    SyntaxKind::Constructor
                        | SyntaxKind::ConstructSignature
                        | SyntaxKind::ConstructorType
                ) {
                    if self.is_in_js_file(declaration) {
                        // The 77625 isJSConstructor/JSDoc arms decide
                        // this band for JS declarations (JS band).
                        return Err(Unsupported::new(
                            "isJSConstructor probe on a JS declaration (JS band)",
                        ));
                    }
                    if self
                        .options
                        .strict_option_value(self.options.no_implicit_any)
                    {
                        self.error_at(
                            Some(node),
                            &diagnostics::new_expression_whose_target_lacks_a_construct_signature_implicitly_has_an_any_type,
                            &[],
                        );
                    }
                    return Ok(self.tables.intrinsics.any);
                }
            }
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        if self
            .tables
            .flags_of(return_type)
            .intersects(TypeFlags::ES_SYMBOL_LIKE)
            && self.is_symbol_or_symbol_for_call(node)?
        {
            // getESSymbolLikeTypeForNode — without it `const s: unique
            // symbol = Symbol()` would render a wrong 2322 (doc §6).
            return Err(Unsupported::new(
                "getESSymbolLikeTypeForNode (unique symbol call results, 5.7b)",
            ));
        }
        Ok(return_type)
    }

    /// tsc-port: isSymbolOrSymbolForCall @6.0.3
    /// tsc-hash: 7f795d82739f8c0d3e0537b4833ca9e15fe55c71dd23938052fff87798ea1dfc
    /// tsc-span: _tsc.js:77692-77717
    fn is_symbol_or_symbol_for_call(&mut self, node: NodeId) -> CheckResult2<bool> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(mut left) = data.expression else {
            return Ok(false);
        };
        if let NodeData::PropertyAccessExpression(access) = self.data_of(left) {
            let is_for = access
                .name
                .and_then(|name| self.identifier_text_of(name))
                .is_some_and(|text| text == "for");
            if is_for {
                if let Some(inner) = access.expression {
                    left = inner;
                }
            }
        }
        if self.kind_of(left) != SyntaxKind::Identifier
            || self.identifier_text_of(left) != Some("Symbol")
        {
            return Ok(false);
        }
        // getGlobalESSymbolConstructorSymbol(reportErrors=false)
        // (77701): the silent global-value probe; the deferredGlobal*
        // memo elides (deterministic, no suggestion-budget burn).
        let Some(global_es_symbol) =
            self.get_global_symbol("Symbol", SymbolFlags::VALUE, None)
        else {
            return Ok(false);
        };
        let resolved = self.resolve_name(
            Some(left),
            "Symbol",
            SymbolFlags::VALUE,
            None,
            false,
            false,
        );
        Ok(resolved == Some(global_es_symbol))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Driver-level fixture check (operators.rs idiom): oracle-pinned
    /// rows (tsc 6.0.3, noLib, options {} unless stated) — scratchpad
    /// pins/{p,q,r}*.ts probes, 2026-07-13.
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        checked_rows_with(text, &CompilerOptions::default())
    }

    fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], options, |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    }

    // ---- M6-stub observability (risk #1) ----

    #[test]
    fn generic_call_without_typeargs_contains() {
        // Oracle: clean — the stub result (unknown where tsc infers
        // number) would poison downstream types, so the statement
        // contains (honest FN of nothing here).
        assert_eq!(
            checked_rows("declare function identity<T>(x: T): T;\nidentity(1);\n"),
            []
        );
    }

    #[test]
    fn generic_callback_contravariance_contains_not_2345() {
        // Oracle: 6133-only (M7 unused). A stub-instantiated
        // applicability verdict would fabricate a 2345 tsc never
        // reports — the whole resolution escapes instead.
        assert_eq!(
            checked_rows(
                "declare function g<T>(f: (x: T) => void): void;\ng((x: number) => {});\n"
            ),
            []
        );
    }

    #[test]
    fn explicit_typearg_argument_mismatch_reports_2345() {
        assert_eq!(
            checked_rows("declare function f<T>(x: T): T;\nf<number>(\"x\");\n"),
            [(2345, 42, 3)]
        );
    }

    #[test]
    fn generic_arity_error_stays_live_under_the_stub() {
        // Arity verdicts read declared parameter counts — stub-free.
        assert_eq!(
            checked_rows("declare function f<T>(a: T, b: T): void;\nf(1);\n"),
            [(2554, 41, 1)]
        );
    }

    // ---- deferred re-check (risk #2) ----

    #[test]
    fn arity_failed_call_rechecks_args_with_candidate_types() {
        // The stashed failure candidate types `x` as string; the
        // deferred plain re-check then reports the noLib 2339 on
        // `length` — NOT a 7006 implicit-any (oracle-pinned pair).
        assert_eq!(
            checked_rows(
                "declare function f(cb: (x: string) => void, b: number): void;\nf((x) => x.length);\n"
            ),
            [(2554, 62, 1), (2339, 73, 6)]
        );
    }

    // ---- code identity pairs (risk #3) ----

    #[test]
    fn construct_only_target_reports_2348_with_display() {
        assert_eq!(
            checked_rows(
                "interface Ctor { new (x: number): object }\ndeclare const c: Ctor;\nc(1);\n"
            ),
            [(2348, 66, 4)]
        );
        // The anonymous-typed flavor contains on display (T2 curtain;
        // oracle: 2348 with the inline shape rendered).
        assert_eq!(
            checked_rows("declare const c: { new (x: number): object };\nc(1);\n"),
            []
        );
    }

    #[test]
    fn rest_under_min_reports_2555_not_2554() {
        assert_eq!(
            checked_rows(
                "declare function r(a: number, ...rest: string[]): void;\nr();\n"
            ),
            [(2555, 56, 1)]
        );
    }

    #[test]
    fn between_overload_bounds_reports_2575() {
        assert_eq!(
            checked_rows(
                "declare function m(a: number): void;\ndeclare function m(a: number, b: string, c: boolean): void;\nm(1, \"x\");\n"
            ),
            [(2575, 97, 1)]
        );
    }

    #[test]
    fn single_signature_typearg_arity_reports_2558_on_the_range() {
        assert_eq!(
            checked_rows("declare function t<T, U>(x: T): void;\nt<number>(1);\n"),
            [(2558, 40, 6)]
        );
    }

    #[test]
    fn overload_typearg_brackets_report_2743() {
        assert_eq!(
            checked_rows(
                "declare function ta<T>(x: T): void;\ndeclare function ta<T, U, V>(x: T): void;\nta<string, number>(\"a\");\n"
            ),
            [(2743, 81, 14)]
        );
    }

    #[test]
    fn boundary_only_multi_signature_typeargs_report_2558() {
        assert_eq!(
            checked_rows(
                "declare function tt<T, U>(x: T, y: U): void;\ndeclare function tt<T>(x: T): void;\ntt<string, number, boolean>(\"a\", 1);\n"
            ),
            [(2558, 84, 23)]
        );
    }

    #[test]
    fn new_over_call_signatures_reports_7009_under_strict_default() {
        assert_eq!(
            checked_rows(
                "declare function nvv(): void;\ndeclare function nvo(): number;\nnew nvv();\nnew nvo();\n"
            ),
            [(7009, 62, 9), (7009, 73, 9)]
        );
    }

    #[test]
    fn no_implicit_any_off_swaps_7009_for_the_2350_band() {
        // Void-returning new-over-call is CLEAN; non-void reports
        // 2350; 7009 is gone (oracle-pinned option flip).
        let options = CompilerOptions {
            no_implicit_any: Some(false),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_rows_with(
                "declare function nvv(): void;\ndeclare function nvo(): number;\nnew nvv();\nnew nvo();\n",
                &options
            ),
            [(2350, 73, 9)]
        );
    }

    #[test]
    fn untyped_call_with_typeargs_reports_2347_at_the_call() {
        assert_eq!(
            checked_rows("declare const anyv: any;\nanyv<number>(1);\n"),
            [(2347, 25, 15)]
        );
    }

    // ---- span discipline (risk #4) ----

    #[test]
    fn plain_argument_mismatch_reports_2345_at_the_arg() {
        assert_eq!(
            checked_rows("declare function s(a: number): void;\ns(\"x\");\n"),
            [(2345, 39, 3)]
        );
    }

    #[test]
    fn under_arity_reports_at_the_callee_name_span() {
        assert_eq!(
            checked_rows("declare const obj: { m(a: number): void };\nobj.m();\n"),
            [(2554, 47, 1)]
        );
    }

    #[test]
    fn over_arity_reports_at_the_excess_args_range() {
        assert_eq!(
            checked_rows("declare function v(a: number): void;\nv(1, 2, 3);\n"),
            [(2554, 42, 4)]
        );
    }

    #[test]
    fn overload_over_arity_uses_the_union_failure_candidate() {
        assert_eq!(
            checked_rows(
                "declare function two(a: number): void;\ndeclare function two(a: string): void;\ntwo(1, 2);\n"
            ),
            [(2554, 85, 1)]
        );
    }

    #[test]
    fn untupled_spread_reports_2556_at_the_spread_arg() {
        assert_eq!(
            checked_rows(
                "declare function sp(a: number, b: number): void;\ndeclare const xs: number[];\nsp(...xs);\n"
            ),
            [(2556, 80, 5)]
        );
    }

    // ---- overload failure chains (2769 band) ----

    #[test]
    fn two_failed_overloads_report_2769_at_the_shared_span() {
        assert_eq!(
            checked_rows(
                "declare function o(a: number): void;\ndeclare function o(a: string): void;\no(true);\n"
            ),
            [(2769, 76, 4)]
        );
    }

    #[test]
    fn many_failed_overloads_report_2769_at_the_last_failure() {
        assert_eq!(
            checked_rows(
                "declare function w(a: number): void;\ndeclare function w(a: string): void;\ndeclare function w(a: boolean): void;\ndeclare function w(a: object): void;\nw(null);\n"
            ),
            [(2769, 151, 4)]
        );
    }

    // ---- invocation errors ----

    #[test]
    fn union_with_uncallable_constituent_reports_one_2349_row() {
        assert_eq!(
            checked_rows("declare const u: { (): void } | { n: number };\nu();\n"),
            [(2349, 47, 1)]
        );
    }

    // ---- the Invoke non-null reporter ----

    #[test]
    fn nullable_narrowable_callee_contains_until_flow() {
        // Oracle: 2721 at `nf` — the receiver is a narrowable
        // identifier, so the [FLOW M5] gate contains (tsc reports on
        // the FLOW type; the declared-type stand-in would FP once
        // narrowing exists).
        assert_eq!(
            checked_rows("declare const nf: (() => void) | null;\nnf();\n"),
            []
        );
    }

    #[test]
    fn nullable_unnarrowable_callee_reports_2721() {
        assert_eq!(
            checked_rows("((null as unknown) as (() => void) | null)();\n"),
            [(2721, 0, 42)]
        );
    }

    // ---- this arguments ----

    #[test]
    fn this_parameter_mismatch_reports_2684_at_the_call() {
        assert_eq!(
            checked_rows(
                "interface N { n: number; }\ndeclare function th(this: N, a: number): void;\nth(1);\n"
            ),
            [(2684, 74, 5)]
        );
    }

    // ---- optional-chain results (risk #9) ----

    #[test]
    fn outer_chain_call_result_takes_the_optional_union() {
        // The IsOuterCallChain return arm adds `undefined` — dropping
        // it would leave a plain `number` RHS and kill this 2322
        // (the assignment shape rides 5.5e; const statements are 5.8).
        assert_eq!(
            checked_rows(
                "declare const oc: { b(): number } | undefined;\ndeclare let sink: number;\nsink = oc?.b();\n"
            ),
            [(2322, 73, 4)]
        );
    }

    // ---- elaboration gate ----

    #[test]
    fn array_literal_args_against_non_array_params_contain() {
        // Oracle: plain 2345 at the literal (elaboration finds no
        // rows). The ARG CHECK contains first: the array literal's
        // element contextual read against a non-array/non-tuple
        // contextual type rides the 5.5c [ITER → 5.8] escape
        // (getIteratedTypeOrElementType) — recorded FN until the
        // iteration protocol lands.
        assert_eq!(
            checked_rows(
                "interface I { p: string }\ndeclare function el(a: I): void;\nel([1]);\n"
            ),
            []
        );
        // Tuple targets check the elements fine; the plain head then
        // stays behind the tuple-display curtain (oracle: 2345 with
        // the tuple rendered).
        assert_eq!(
            checked_rows("declare function tup(a: [number]): void;\ntup([1, \"x\"]);\n"),
            []
        );
    }

    #[test]
    fn array_literal_arg_with_elementwise_rows_contains() {
        // Oracle: 2322 at the element — the elementwise elaboration
        // (T2) owns that row; the plain head would be a wrong-payload
        // FP, so the statement contains.
        assert_eq!(
            checked_rows("declare function tup(a: [number]): void;\ntup([\"x\"]);\n"),
            []
        );
    }

    #[test]
    fn function_valued_argument_success_is_clean() {
        assert_eq!(
            checked_rows(
                "declare function cb(f: () => number): void;\ndeclare function mk(): number;\ncb(mk);\n"
            ),
            []
        );
    }

    #[test]
    fn optional_method_chain_call_contains_until_flow() {
        // Oracle: 2722 at `om.m` — property-access receivers are
        // narrowable ([FLOW M5] gate).
        assert_eq!(
            checked_rows("declare const om: { m?(): void };\nom.m();\n"),
            []
        );
    }

}
