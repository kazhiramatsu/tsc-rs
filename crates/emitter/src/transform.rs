use std::error::Error;
use std::fmt;

use tsc_diagnostics::{Diagnostic, DiagnosticList};
use tsc_syntax::SyntaxKind;

use crate::{
    EmitFlags, EmitResolverError, NodeFactory, TransformArena, TransformNode, TransformNodeArray,
    TransformSourceId, UnsupportedEmitFeature,
};

/// TypeScript transform-feature bits retained outside persistent syntax nodes.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TransformFlags(i32);

impl TransformFlags {
    pub const NONE: Self = Self(0);
    pub const CONTAINS_TYPE_SCRIPT: Self = Self(1);
    pub const CONTAINS_JSX: Self = Self(2);
    pub const CONTAINS_ES_NEXT: Self = Self(4);
    pub const CONTAINS_ES_2022: Self = Self(8);
    pub const CONTAINS_ES_2021: Self = Self(16);
    pub const CONTAINS_ES_2020: Self = Self(32);
    pub const CONTAINS_ES_2019: Self = Self(64);
    pub const CONTAINS_ES_2018: Self = Self(128);
    pub const CONTAINS_ES_2017: Self = Self(256);
    pub const CONTAINS_ES_2016: Self = Self(512);
    pub const CONTAINS_ES_2015: Self = Self(1024);
    pub const CONTAINS_GENERATOR: Self = Self(2048);
    pub const CONTAINS_DESTRUCTURING_ASSIGNMENT: Self = Self(4096);
    pub const CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX: Self = Self(8192);
    pub const CONTAINS_LEXICAL_THIS: Self = Self(16384);
    pub const CONTAINS_REST_OR_SPREAD: Self = Self(32768);
    pub const CONTAINS_OBJECT_REST_OR_SPREAD: Self = Self(65536);
    pub const CONTAINS_COMPUTED_PROPERTY_NAME: Self = Self(131072);
    pub const CONTAINS_BLOCK_SCOPED_BINDING: Self = Self(262144);
    pub const CONTAINS_BINDING_PATTERN: Self = Self(524288);
    pub const CONTAINS_YIELD: Self = Self(1048576);
    pub const CONTAINS_AWAIT: Self = Self(2097152);
    pub const CONTAINS_HOISTED_DECLARATION_OR_COMPLETION: Self = Self(4194304);
    pub const CONTAINS_DYNAMIC_IMPORT: Self = Self(8388608);
    pub const CONTAINS_CLASS_FIELDS: Self = Self(16777216);
    pub const CONTAINS_DECORATORS: Self = Self(33554432);
    pub const CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT: Self = Self(67108864);
    pub const CONTAINS_LEXICAL_SUPER: Self = Self(134217728);
    pub const CONTAINS_UPDATE_EXPRESSION_FOR_IDENTIFIER: Self = Self(268435456);
    pub const CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION: Self = Self(536870912);
    pub const HAS_COMPUTED_FLAGS: Self = Self(i32::MIN);

    pub const OUTER_EXPRESSION_EXCLUDES: Self = Self(i32::MIN);
    pub const PROPERTY_ACCESS_EXCLUDES: Self = Self(i32::MIN);
    pub const NODE_EXCLUDES: Self = Self(i32::MIN);
    pub const ARROW_FUNCTION_EXCLUDES: Self = Self(-2072174592);
    pub const FUNCTION_EXCLUDES: Self = Self(-1937940480);
    pub const CONSTRUCTOR_EXCLUDES: Self = Self(-1937948672);
    pub const METHOD_OR_ACCESSOR_EXCLUDES: Self = Self(-2005057536);
    pub const PROPERTY_EXCLUDES: Self = Self(-2013249536);
    pub const CLASS_EXCLUDES: Self = Self(-2147344384);
    pub const MODULE_EXCLUDES: Self = Self(-1941676032);
    pub const TYPE_EXCLUDES: Self = Self(-2);
    pub const OBJECT_LITERAL_EXCLUDES: Self = Self(-2147278848);
    pub const ARRAY_LITERAL_OR_CALL_OR_NEW_EXCLUDES: Self = Self(-2147450880);
    pub const VARIABLE_DECLARATION_LIST_EXCLUDES: Self = Self(-2146893824);
    pub const PARAMETER_EXCLUDES: Self = Self(i32::MIN);
    pub const CATCH_CLAUSE_EXCLUDES: Self = Self(-2147418112);
    pub const BINDING_PATTERN_EXCLUDES: Self = Self(-2147450880);
    pub const CONTAINS_LEXICAL_THIS_OR_SUPER: Self = Self(134234112);
    pub const PROPERTY_NAME_PROPAGATING_FLAGS: Self = Self(134234112);

    pub const fn from_bits(bits: i32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> i32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// tsc-port: getTransformFlagsSubtreeExclusions @6.0.3
    /// tsc-hash: 2d364dcf4298f054e648486f6e466f4b82d973fc80597df00ed06d9c612aa913
    /// tsc-span: _tsc.js:25125-25194
    pub const fn subtree_exclusions(kind: SyntaxKind) -> Self {
        if (kind as u16) >= SyntaxKind::FirstTypeNode as u16
            && (kind as u16) <= SyntaxKind::LastTypeNode as u16
        {
            return Self::TYPE_EXCLUDES;
        }
        match kind {
            SyntaxKind::CallExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::ArrayLiteralExpression => Self::ARRAY_LITERAL_OR_CALL_OR_NEW_EXCLUDES,
            SyntaxKind::ModuleDeclaration => Self::MODULE_EXCLUDES,
            SyntaxKind::Parameter => Self::PARAMETER_EXCLUDES,
            SyntaxKind::ArrowFunction => Self::ARROW_FUNCTION_EXCLUDES,
            SyntaxKind::FunctionExpression | SyntaxKind::FunctionDeclaration => {
                Self::FUNCTION_EXCLUDES
            }
            SyntaxKind::VariableDeclarationList => Self::VARIABLE_DECLARATION_LIST_EXCLUDES,
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => Self::CLASS_EXCLUDES,
            SyntaxKind::Constructor => Self::CONSTRUCTOR_EXCLUDES,
            SyntaxKind::PropertyDeclaration => Self::PROPERTY_EXCLUDES,
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                Self::METHOD_OR_ACCESSOR_EXCLUDES
            }
            SyntaxKind::AnyKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::TypeParameter
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration => Self::TYPE_EXCLUDES,
            SyntaxKind::ObjectLiteralExpression => Self::OBJECT_LITERAL_EXCLUDES,
            SyntaxKind::CatchClause => Self::CATCH_CLAUSE_EXCLUDES,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern => {
                Self::BINDING_PATTERN_EXCLUDES
            }
            SyntaxKind::TypeAssertionExpression
            | SyntaxKind::SatisfiesExpression
            | SyntaxKind::AsExpression
            | SyntaxKind::PartiallyEmittedExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::SuperKeyword => Self::OUTER_EXPRESSION_EXCLUDES,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                Self::PROPERTY_ACCESS_EXCLUDES
            }
            _ => Self::NODE_EXCLUDES,
        }
    }
}

impl std::ops::BitOr for TransformFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for TransformFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for TransformFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::Not for TransformFlags {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TransformationState {
    Uninitialized,
    Initialized,
    Completed,
    Disposed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitHint {
    SourceFile,
    Expression,
    IdentifierName,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnsupportedTransformFeature {
    Decorators,
    ExportEquals,
    ImportEquals,
    Jsx,
    ParameterProperties,
    RuntimeEnums,
    RuntimeNamespaces,
}

impl UnsupportedTransformFeature {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Decorators => "decorators",
            Self::ExportEquals => "export-equals",
            Self::ImportEquals => "import-equals",
            Self::Jsx => "JSX",
            Self::ParameterProperties => "parameter properties",
            Self::RuntimeEnums => "runtime enums",
            Self::RuntimeNamespaces => "runtime namespaces",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransformBundle {
    sources: Box<[TransformSourceId]>,
}

impl TransformBundle {
    pub fn new(sources: Vec<TransformSourceId>) -> Self {
        Self {
            sources: sources.into_boxed_slice(),
        }
    }

    pub fn sources(&self) -> &[TransformSourceId] {
        &self.sources
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransformRoot {
    SourceFile(TransformSourceId),
    Bundle(TransformBundle),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitHelper {
    name: Box<str>,
    scoped: bool,
    text: Option<Box<str>>,
    priority: u8,
    dependencies: Box<[EmitHelper]>,
}

impl EmitHelper {
    pub fn new(name: impl Into<Box<str>>, scoped: bool, dependencies: Vec<EmitHelper>) -> Self {
        Self {
            name: name.into(),
            scoped,
            text: None,
            priority: 0,
            dependencies: dependencies.into_boxed_slice(),
        }
    }

    pub fn with_text(
        name: impl Into<Box<str>>,
        scoped: bool,
        text: impl Into<Box<str>>,
        priority: u8,
        dependencies: Vec<EmitHelper>,
    ) -> Self {
        Self {
            name: name.into(),
            scoped,
            text: Some(text.into()),
            priority,
            dependencies: dependencies.into_boxed_slice(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn scoped(&self) -> bool {
        self.scoped
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub const fn priority(&self) -> u8 {
        self.priority
    }

    pub fn dependencies(&self) -> &[EmitHelper] {
        &self.dependencies
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LexicalEnvironment {
    variable_declarations: Vec<TransformNode>,
    function_declarations: Vec<TransformNode>,
    initialization_statements: Vec<TransformNode>,
}

impl LexicalEnvironment {
    pub fn variable_declarations(&self) -> &[TransformNode] {
        &self.variable_declarations
    }

    pub fn function_declarations(&self) -> &[TransformNode] {
        &self.function_declarations
    }

    pub fn initialization_statements(&self) -> &[TransformNode] {
        &self.initialization_statements
    }

    pub fn is_empty(&self) -> bool {
        self.variable_declarations.is_empty()
            && self.function_declarations.is_empty()
            && self.initialization_statements.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct LexicalEnvironmentFlags(u32);

impl LexicalEnvironmentFlags {
    pub const NONE: Self = Self(0);
    pub const IN_PARAMETERS: Self = Self(1);
    pub const VARIABLES_HOISTED_IN_PARAMETERS: Self = Self(2);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for LexicalEnvironmentFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for LexicalEnvironmentFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::Not for LexicalEnvironmentFlags {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

#[derive(Debug)]
pub struct TransformationContext {
    arena: TransformArena,
    state: TransformationState,
    enabled_syntax_features: Vec<u8>,
    lexical_environment: LexicalEnvironment,
    lexical_environment_flags: LexicalEnvironmentFlags,
    lexical_environment_stack: Vec<(LexicalEnvironment, LexicalEnvironmentFlags)>,
    lexical_environment_suspended: bool,
    block_scoped_variables: Vec<TransformNode>,
    block_scope_stack: Vec<Vec<TransformNode>>,
    emit_helpers: Vec<EmitHelper>,
    diagnostics: DiagnosticList,
}

impl TransformationContext {
    fn new(arena: TransformArena) -> Self {
        Self {
            arena,
            state: TransformationState::Uninitialized,
            enabled_syntax_features: vec![0; SyntaxKind::Count as usize],
            lexical_environment: LexicalEnvironment::default(),
            lexical_environment_flags: LexicalEnvironmentFlags::NONE,
            lexical_environment_stack: Vec::new(),
            lexical_environment_suspended: false,
            block_scoped_variables: Vec::new(),
            block_scope_stack: Vec::new(),
            emit_helpers: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub const fn state(&self) -> TransformationState {
        self.state
    }

    pub const fn arena(&self) -> &TransformArena {
        &self.arena
    }

    pub fn arena_mut(&mut self) -> Result<&mut TransformArena, TransformError> {
        self.require_before_disposed("access transform arena")?;
        Ok(&mut self.arena)
    }

    pub fn factory(&mut self) -> Result<NodeFactory<'_>, TransformError> {
        self.require_before_completed("construct or use the node factory")?;
        Ok(NodeFactory::new(&mut self.arena))
    }

    /// Constructs nodes owned by an emit-time substitution.
    ///
    /// TypeScript's substitution hooks may synthesize a replacement after the
    /// transform pipeline has completed. Keeping that capability separate from
    /// [`Self::factory`] makes the lifecycle distinction explicit: ordinary
    /// transforms cannot be resumed during printing, while a substitution may
    /// still append immutable replacement nodes to the session arena.
    pub fn substitution_factory(&mut self) -> Result<NodeFactory<'_>, TransformError> {
        self.require_before_disposed("construct an emit substitution")?;
        Ok(NodeFactory::new(&mut self.arena))
    }

    pub fn enable_substitution(&mut self, kind: SyntaxKind) -> Result<(), TransformError> {
        self.require_before_completed("enable substitution")?;
        self.enabled_syntax_features[kind as usize] |= 1;
        Ok(())
    }

    pub fn enable_emit_notification(&mut self, kind: SyntaxKind) -> Result<(), TransformError> {
        self.require_before_completed("enable emit notification")?;
        self.enabled_syntax_features[kind as usize] |= 2;
        Ok(())
    }

    pub fn is_substitution_enabled(&self, node: TransformNode) -> Result<bool, TransformError> {
        self.require_before_disposed("query substitution")?;
        let kind = self.arena.node(node)?.kind;
        let emit_flags = self
            .arena
            .metadata(node)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags());
        Ok(self.enabled_syntax_features[kind as usize] & 1 != 0
            && !emit_flags.intersects(EmitFlags::NO_SUBSTITUTION))
    }

    pub fn is_emit_notification_enabled(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        self.require_before_disposed("query emit notification")?;
        let kind = self.arena.node(node)?.kind;
        let emit_flags = self
            .arena
            .metadata(node)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags());
        Ok(self.enabled_syntax_features[kind as usize] & 2 != 0
            || emit_flags.intersects(EmitFlags::ADVISE_ON_EMIT_NODE))
    }

    pub fn start_lexical_environment(&mut self) -> Result<(), TransformError> {
        self.require_transforming("start a lexical environment")?;
        if self.lexical_environment_suspended {
            return Err(TransformError::LexicalEnvironmentSuspended);
        }
        self.lexical_environment_stack.push((
            std::mem::take(&mut self.lexical_environment),
            self.lexical_environment_flags,
        ));
        self.lexical_environment_flags = LexicalEnvironmentFlags::NONE;
        Ok(())
    }

    pub fn suspend_lexical_environment(&mut self) -> Result<(), TransformError> {
        self.require_transforming("suspend a lexical environment")?;
        if self.lexical_environment_suspended {
            return Err(TransformError::LexicalEnvironmentAlreadySuspended);
        }
        self.lexical_environment_suspended = true;
        Ok(())
    }

    pub fn resume_lexical_environment(&mut self) -> Result<(), TransformError> {
        self.require_transforming("resume a lexical environment")?;
        if !self.lexical_environment_suspended {
            return Err(TransformError::LexicalEnvironmentNotSuspended);
        }
        self.lexical_environment_suspended = false;
        Ok(())
    }

    pub fn end_lexical_environment(&mut self) -> Result<LexicalEnvironment, TransformError> {
        self.require_transforming("end a lexical environment")?;
        if self.lexical_environment_suspended {
            return Err(TransformError::LexicalEnvironmentSuspended);
        }
        let completed = std::mem::take(&mut self.lexical_environment);
        let (outer, flags) = self
            .lexical_environment_stack
            .pop()
            .ok_or(TransformError::LexicalEnvironmentUnderflow)?;
        self.lexical_environment = outer;
        self.lexical_environment_flags = flags;
        Ok(completed)
    }

    pub fn set_lexical_environment_flags(
        &mut self,
        flags: LexicalEnvironmentFlags,
        value: bool,
    ) -> Result<(), TransformError> {
        self.require_transforming("set lexical environment flags")?;
        self.lexical_environment_flags = if value {
            self.lexical_environment_flags | flags
        } else {
            self.lexical_environment_flags & !flags
        };
        Ok(())
    }

    pub const fn lexical_environment_flags(&self) -> LexicalEnvironmentFlags {
        self.lexical_environment_flags
    }

    pub fn hoist_variable_declaration(
        &mut self,
        declaration: TransformNode,
    ) -> Result<(), TransformError> {
        self.require_transforming("hoist a variable declaration")?;
        self.arena.node(declaration)?;
        self.lexical_environment
            .variable_declarations
            .push(declaration);
        if self
            .lexical_environment_flags
            .contains(LexicalEnvironmentFlags::IN_PARAMETERS)
        {
            self.lexical_environment_flags = self.lexical_environment_flags
                | LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS;
        }
        Ok(())
    }

    pub fn hoist_function_declaration(
        &mut self,
        declaration: TransformNode,
    ) -> Result<(), TransformError> {
        self.require_transforming("hoist a function declaration")?;
        self.arena.node(declaration)?;
        self.arena.metadata_mut(declaration).flags |= EmitFlags::CUSTOM_PROLOGUE;
        self.lexical_environment
            .function_declarations
            .push(declaration);
        Ok(())
    }

    pub fn add_initialization_statement(
        &mut self,
        statement: TransformNode,
    ) -> Result<(), TransformError> {
        self.require_transforming("add an initialization statement")?;
        self.arena.node(statement)?;
        self.arena.metadata_mut(statement).flags |= EmitFlags::CUSTOM_PROLOGUE;
        self.lexical_environment
            .initialization_statements
            .push(statement);
        Ok(())
    }

    pub fn start_block_scope(&mut self) -> Result<(), TransformError> {
        self.require_transforming("start a block scope")?;
        self.block_scope_stack
            .push(std::mem::take(&mut self.block_scoped_variables));
        Ok(())
    }

    pub fn add_block_scoped_variable(&mut self, name: TransformNode) -> Result<(), TransformError> {
        self.require_transforming("add a block-scoped variable")?;
        if self.block_scope_stack.is_empty() {
            return Err(TransformError::BlockScopeRequired);
        }
        self.arena.node(name)?;
        self.block_scoped_variables.push(name);
        Ok(())
    }

    pub fn end_block_scope(&mut self) -> Result<Vec<TransformNode>, TransformError> {
        self.require_transforming("end a block scope")?;
        let completed = std::mem::take(&mut self.block_scoped_variables);
        self.block_scoped_variables = self
            .block_scope_stack
            .pop()
            .ok_or(TransformError::BlockScopeUnderflow)?;
        Ok(completed)
    }

    pub fn request_emit_helper(&mut self, helper: EmitHelper) -> Result<(), TransformError> {
        self.require_transforming("request an emit helper")?;
        if helper.scoped {
            return Err(TransformError::ScopedEmitHelper(helper.name.clone()));
        }
        for dependency in helper.dependencies.iter().cloned() {
            self.request_emit_helper(dependency)?;
        }
        if self
            .emit_helpers
            .iter()
            .any(|existing| existing.name == helper.name)
        {
            return Ok(());
        }
        self.emit_helpers.push(helper);
        Ok(())
    }

    pub fn read_emit_helpers(&mut self) -> Result<Vec<EmitHelper>, TransformError> {
        self.require_transforming("read emit helpers")?;
        Ok(std::mem::take(&mut self.emit_helpers))
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) -> Result<(), TransformError> {
        self.require_before_disposed("add a transform diagnostic")?;
        self.diagnostics.push(diagnostic);
        Ok(())
    }

    fn require_transforming(&self, operation: &'static str) -> Result<(), TransformError> {
        if self.state != TransformationState::Initialized {
            return Err(TransformError::InvalidLifecycle {
                operation,
                state: self.state,
            });
        }
        Ok(())
    }

    fn require_before_completed(&self, operation: &'static str) -> Result<(), TransformError> {
        if matches!(
            self.state,
            TransformationState::Completed | TransformationState::Disposed
        ) {
            return Err(TransformError::InvalidLifecycle {
                operation,
                state: self.state,
            });
        }
        Ok(())
    }

    fn require_before_disposed(&self, operation: &'static str) -> Result<(), TransformError> {
        if self.state == TransformationState::Disposed {
            return Err(TransformError::InvalidLifecycle {
                operation,
                state: self.state,
            });
        }
        Ok(())
    }

    fn dispose(&mut self) {
        if self.state == TransformationState::Disposed {
            return;
        }
        self.lexical_environment = LexicalEnvironment::default();
        self.lexical_environment_stack.clear();
        self.block_scoped_variables.clear();
        self.block_scope_stack.clear();
        self.emit_helpers.clear();
        self.arena.clear_session_metadata();
        self.state = TransformationState::Disposed;
    }
}

/// Built-in transformer protocol. H1 exposes no custom-transformer ABI; this
/// trait is the internal ownership seam used by the three vendored built-ins.
pub trait Transformer {
    fn name(&self) -> &'static str;

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        Ok(())
    }

    fn transform_root(
        &mut self,
        _context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        Ok(root)
    }

    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }

    fn before_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        Ok(())
    }

    fn dispose(&mut self) {}
}

pub struct TransformationResult<'transformers> {
    context: TransformationContext,
    roots: Box<[TransformRoot]>,
    transformers: Vec<Box<dyn Transformer + 'transformers>>,
}

impl TransformationResult<'_> {
    pub const fn state(&self) -> TransformationState {
        self.context.state()
    }

    pub const fn arena(&self) -> &TransformArena {
        self.context.arena()
    }

    pub fn roots(&self) -> &[TransformRoot] {
        &self.roots
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.context.diagnostics
    }

    pub(crate) fn emit_helpers(&self) -> &[EmitHelper] {
        &self.context.emit_helpers
    }

    pub fn substitute_node(
        &mut self,
        hint: EmitHint,
        mut node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if !self.context.is_substitution_enabled(node)? {
            return Ok(node);
        }
        for transformer in &mut self.transformers {
            node = transformer.substitute_node(&mut self.context, hint, node)?;
            self.context.arena.node(node)?;
        }
        Ok(node)
    }

    pub fn before_emit_node(
        &mut self,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.context.is_emit_notification_enabled(node)? {
            for transformer in &mut self.transformers {
                transformer.before_emit_node(&self.context, hint, node)?;
            }
        }
        Ok(())
    }

    pub fn after_emit_node(
        &mut self,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.context.is_emit_notification_enabled(node)? {
            for transformer in self.transformers.iter_mut().rev() {
                transformer.after_emit_node(&self.context, hint, node)?;
            }
        }
        Ok(())
    }

    pub fn dispose(&mut self) {
        if self.context.state == TransformationState::Disposed {
            return;
        }
        for transformer in self.transformers.iter_mut().rev() {
            transformer.dispose();
        }
        self.context.dispose();
    }
}

impl Drop for TransformationResult<'_> {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// tsc-port: transformNodes @6.0.3
/// tsc-hash: ef2079da1a35b78b43d8794c034dd6caabdad5b71547b22c3270c40d47349e84
/// tsc-span: _tsc.js:115977-116276
pub fn transform_nodes<'transformers>(
    arena: TransformArena,
    roots: Vec<TransformRoot>,
    mut transformers: Vec<Box<dyn Transformer + 'transformers>>,
    allow_declaration_files: bool,
) -> Result<TransformationResult<'transformers>, TransformError> {
    let mut context = TransformationContext::new(arena);
    let transformed = (|| {
        if roots
            .iter()
            .any(|root| matches!(root, TransformRoot::Bundle(_)))
        {
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            ));
        }

        for transformer in &mut transformers {
            transformer.initialize(&mut context)?;
        }
        context.state = TransformationState::Initialized;

        let mut transformed = Vec::with_capacity(roots.len());
        for mut root in roots {
            let should_transform = match &root {
                TransformRoot::SourceFile(source) => {
                    let syntax = context.arena.source(*source)?.syntax();
                    allow_declaration_files || !syntax.is_declaration_file
                }
                TransformRoot::Bundle(_) => false,
            };
            if should_transform {
                for transformer in &mut transformers {
                    root = transformer.transform_root(&mut context, root)?;
                    if matches!(root, TransformRoot::Bundle(_)) {
                        return Err(TransformError::Unsupported(
                            UnsupportedEmitFeature::BundleRoot,
                        ));
                    }
                }
            }
            transformed.push(root);
        }
        context.state = TransformationState::Completed;
        Ok(transformed)
    })();

    let transformed = match transformed {
        Ok(transformed) => transformed,
        Err(error) => {
            for transformer in transformers.iter_mut().rev() {
                transformer.dispose();
            }
            context.dispose();
            return Err(error);
        }
    };
    Ok(TransformationResult {
        context,
        roots: transformed.into_boxed_slice(),
        transformers,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransformError {
    UnknownSource(TransformSourceId),
    UnknownNode(TransformNode),
    UnknownNodeArray(TransformNodeArray),
    CrossSourceNode {
        expected: TransformSourceId,
        actual: TransformSourceId,
    },
    FactoryKindMismatch {
        expected: SyntaxKind,
        actual: SyntaxKind,
    },
    FactoryTokenDataRequiresTokenConstructor,
    FactoryTokenKindExpected(SyntaxKind),
    RootKindExpected {
        actual: SyntaxKind,
    },
    RequiredChildRemoved {
        parent: SyntaxKind,
        field: &'static str,
    },
    MissingProgramSource(TransformNode),
    MissingProgramSourceForModuleFormat(TransformSourceId),
    EmitHostRequiredForImpliedModuleFormat,
    DeferredModuleFormat {
        format: i32,
        owner_slice: &'static str,
    },
    ParseDiagnosticsDeferred {
        count: usize,
        owner_slice: &'static str,
    },
    AstDepthDeferred {
        limit: usize,
        owner_slice: &'static str,
    },
    ImportAttributesDeferred {
        owner_slice: &'static str,
    },
    AdvancedCommentPlacementDeferred {
        owner_slice: &'static str,
    },
    UnsupportedCompilerOption {
        option: &'static str,
        detail: &'static str,
    },
    UnsupportedSyntax {
        feature: UnsupportedTransformFeature,
        node: TransformNode,
    },
    Resolver(EmitResolverError),
    InvalidLifecycle {
        operation: &'static str,
        state: TransformationState,
    },
    LexicalEnvironmentSuspended,
    LexicalEnvironmentAlreadySuspended,
    LexicalEnvironmentNotSuspended,
    LexicalEnvironmentUnderflow,
    BlockScopeRequired,
    BlockScopeUnderflow,
    ScopedEmitHelper(Box<str>),
    Unsupported(UnsupportedEmitFeature),
}

impl fmt::Display for TransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSource(source) => {
                write!(formatter, "unknown transform source {}", source.raw())
            }
            Self::UnknownNode(node) => write!(
                formatter,
                "unknown transform node {}:{}",
                node.source().raw(),
                node.node().0
            ),
            Self::UnknownNodeArray(array) => write!(
                formatter,
                "unknown transform node array {}:{}",
                array.source().raw(),
                array.array().0
            ),
            Self::CrossSourceNode { expected, actual } => write!(
                formatter,
                "transform node belongs to source {}, expected {}",
                actual.raw(),
                expected.raw()
            ),
            Self::FactoryKindMismatch { expected, actual } => write!(
                formatter,
                "node factory update changed kind from {expected:?} to {actual:?}"
            ),
            Self::FactoryTokenDataRequiresTokenConstructor => {
                formatter.write_str("NodeData::Token requires the token factory constructor")
            }
            Self::FactoryTokenKindExpected(actual) => write!(
                formatter,
                "token factory received non-token syntax kind {actual:?}"
            ),
            Self::RootKindExpected { actual } => {
                write!(
                    formatter,
                    "transform root must be a SourceFile, got {actual:?}"
                )
            }
            Self::RequiredChildRemoved { parent, field } => write!(
                formatter,
                "transform removed required child {field} from {parent:?}"
            ),
            Self::MissingProgramSource(node) => write!(
                formatter,
                "transform node {}:{} has no Program source for an emit-resolver query",
                node.source().raw(),
                node.node().0
            ),
            Self::MissingProgramSourceForModuleFormat(source) => write!(
                formatter,
                "transform source {} has no Program source for emitted module-format dispatch",
                source.raw()
            ),
            Self::EmitHostRequiredForImpliedModuleFormat => formatter
                .write_str("implied module-format transformation requires a Program emit host"),
            Self::DeferredModuleFormat {
                format,
                owner_slice,
            } => write!(
                formatter,
                "emitted module format {format} is deferred to {owner_slice}"
            ),
            Self::ParseDiagnosticsDeferred { count, owner_slice } => write!(
                formatter,
                "emit recovery for {count} parse diagnostics is deferred to {owner_slice}"
            ),
            Self::AstDepthDeferred { limit, owner_slice } => write!(
                formatter,
                "emit transform AST depth above {limit} is deferred to {owner_slice}"
            ),
            Self::ImportAttributesDeferred { owner_slice } => write!(
                formatter,
                "import attributes during emit are deferred to {owner_slice}"
            ),
            Self::AdvancedCommentPlacementDeferred { owner_slice } => write!(
                formatter,
                "advanced comment placement during emit is deferred to {owner_slice}"
            ),
            Self::UnsupportedCompilerOption { option, detail } => {
                write!(formatter, "unsupported transform option {option}: {detail}")
            }
            Self::UnsupportedSyntax { feature, node } => write!(
                formatter,
                "unsupported {} syntax at transform node {}:{}",
                feature.name(),
                node.source().raw(),
                node.node().0
            ),
            Self::Resolver(error) => error.fmt(formatter),
            Self::InvalidLifecycle { operation, state } => {
                write!(
                    formatter,
                    "cannot {operation} while transform state is {state:?}"
                )
            }
            Self::LexicalEnvironmentSuspended => {
                formatter.write_str("lexical environment is suspended")
            }
            Self::LexicalEnvironmentAlreadySuspended => {
                formatter.write_str("lexical environment is already suspended")
            }
            Self::LexicalEnvironmentNotSuspended => {
                formatter.write_str("lexical environment is not suspended")
            }
            Self::LexicalEnvironmentUnderflow => {
                formatter.write_str("lexical environment stack underflow")
            }
            Self::BlockScopeRequired => {
                formatter.write_str("block-scoped variable requires an active block scope")
            }
            Self::BlockScopeUnderflow => formatter.write_str("block scope stack underflow"),
            Self::ScopedEmitHelper(name) => {
                write!(
                    formatter,
                    "scoped emit helper {name} cannot be requested globally"
                )
            }
            Self::Unsupported(feature) => {
                write!(
                    formatter,
                    "unsupported transform request: {}",
                    feature.name()
                )
            }
        }
    }
}

impl Error for TransformError {}

impl From<EmitResolverError> for TransformError {
    fn from(value: EmitResolverError) -> Self {
        Self::Resolver(value)
    }
}
