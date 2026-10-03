//! The hosted JSDoc tags of a JavaScript file, as tsgo's reparser applies
//! them (TypeScript 7.1 parser/reparser.go reparseHosted, 346-613).
//!
//! tsgo rewrites `@type`, `@param`, `@return`, `@this`, `@template`,
//! `@satisfies`, `@implements`, `@augments` and the modifier tags into
//! ordinary syntax of the node they annotate (type annotations, type
//! parameters, a `this` parameter, question tokens, modifiers, casts and
//! heritage clauses), and the checker sees only that syntax. tsc-rs keeps the
//! tree as parsed and records the same decisions here; the binder fills the
//! table (it needs the assignment-declaration classification), and the
//! checker reads a declaration's reparsed annotations through it.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::NodeId;

/// A cast the reparser wraps around an expression: `expr as T` for a `@type`
/// on a return statement or parenthesized expression, `expr satisfies T` for
/// `@satisfies` (reparser.go makeNewCast).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostedCast {
    /// The tag's type node (the type inside its JSDoc type expression).
    pub type_node: NodeId,
    /// The `@type` or `@satisfies` tag.
    pub tag: NodeId,
    /// `as` (an assertion) rather than `satisfies`.
    pub is_assertion: bool,
}

/// The type parameters the reparser gives a function or class from the
/// `@template` tags of its JSDoc (reparser.go gatherTypeParameters).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedTypeParameters {
    /// The list's range: from the first `@template` tag's start to the last
    /// one's end.
    pub pos: u32,
    pub end: u32,
    /// The `@template` tags in order; their type parameters, in order, form
    /// the list, and a tag's constraint belongs to its first type parameter.
    pub tags: Vec<NodeId>,
}

/// tsgo's hosted reparse of one file. Every map is keyed by the node tsgo
/// mutates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsDocHosted {
    /// The reparsed `Type()` of a declaration: a variable, parameter,
    /// property declaration, property assignment, shorthand property
    /// assignment, export assignment or assignment-declaration binary
    /// expression (`@type`, `@param`), and the return type of a function-like
    /// declaration (`@return`, `@type` on a get accessor).
    pub types: FxHashMap<NodeId, NodeId>,
    /// A function-like declaration's `FullSignature`: the type of a `@type`
    /// tag that types the whole function.
    pub full_signatures: FxHashMap<NodeId, NodeId>,
    /// A function-like declaration's or class's reparsed type parameters.
    pub type_parameters: FxHashMap<NodeId, HostedTypeParameters>,
    /// A parameter's reparsed question token: the bracketed or `=`-typed
    /// `@param` tag that made it optional.
    pub question_tokens: FxHashMap<NodeId, NodeId>,
    /// The parameter a hosted `@param` tag matched (reparser.go
    /// findMatchingParameter), keyed by the tag.
    pub matched_parameters: FxHashMap<NodeId, NodeId>,
    /// A function-like declaration's reparsed `this` parameter: the `@this`
    /// tag (its type, when present, is the parameter's type).
    pub this_tags: FxHashMap<NodeId, NodeId>,
    /// The reparsed modifiers of a member, constructor or binary expression:
    /// the `@readonly`, `@private`, `@public`, `@protected` and `@override`
    /// tags in order, after any written modifiers.
    pub modifiers: FxHashMap<NodeId, Vec<NodeId>>,
    /// The cast wrapped around an expression.
    pub casts: FxHashMap<NodeId, HostedCast>,
    /// A class's reparsed `implements` types: the `@implements` tags in order.
    pub implements: FxHashMap<NodeId, Vec<NodeId>>,
    /// The `@augments` tag whose type arguments an `extends` element takes.
    pub augments: FxHashMap<NodeId, NodeId>,
}

impl JsDocHosted {
    pub fn type_of(&self, node: NodeId) -> Option<NodeId> {
        self.types.get(&node).copied()
    }

    pub fn full_signature_of(&self, node: NodeId) -> Option<NodeId> {
        self.full_signatures.get(&node).copied()
    }

    pub fn type_parameters_of(&self, node: NodeId) -> Option<&HostedTypeParameters> {
        self.type_parameters.get(&node)
    }

    pub fn question_token_of(&self, parameter: NodeId) -> Option<NodeId> {
        self.question_tokens.get(&parameter).copied()
    }

    pub fn matched_parameter_of(&self, tag: NodeId) -> Option<NodeId> {
        self.matched_parameters.get(&tag).copied()
    }

    pub fn this_tag_of(&self, node: NodeId) -> Option<NodeId> {
        self.this_tags.get(&node).copied()
    }

    pub fn modifier_tags_of(&self, node: NodeId) -> &[NodeId] {
        self.modifiers.get(&node).map_or(&[], Vec::as_slice)
    }

    pub fn cast_of(&self, expression: NodeId) -> Option<HostedCast> {
        self.casts.get(&expression).copied()
    }

    pub fn implements_tags_of(&self, class: NodeId) -> &[NodeId] {
        self.implements.get(&class).map_or(&[], Vec::as_slice)
    }

    pub fn augments_tag_of(&self, element: NodeId) -> Option<NodeId> {
        self.augments.get(&element).copied()
    }
}

/// The lazily filled table of a `SourceFile`. It is derived from the tree, so
/// a copy starts empty and every cell compares equal. The table is boxed so
/// that it adds only a pointer to every `SourceFile` value.
#[derive(Default)]
pub struct JsDocHostedCell(OnceLock<Box<JsDocHosted>>);

impl JsDocHostedCell {
    pub fn get_or_init(&self, init: impl FnOnce() -> JsDocHosted) -> &JsDocHosted {
        self.0.get_or_init(|| Box::new(init()))
    }
}

impl Clone for JsDocHostedCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for JsDocHostedCell {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for JsDocHostedCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JsDocHostedCell")
    }
}
