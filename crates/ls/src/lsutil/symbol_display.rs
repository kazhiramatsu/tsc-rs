//! tsgo `ls/lsutil/symbol_display.go`: a symbol's script element kind and
//! kind modifiers.

use tsc_binder::node_util;
use tsc_checker::state::{CheckResult, CheckerState, SignatureKind};
use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::{CheckFlags, ModifierFlags, NodeFlags, SymbolFlags, SymbolId};

fn kind_of(state: &CheckerState<'_>, node: NodeId) -> SyntaxKind {
    state.binder.node_record(node).kind
}

fn parent_of(state: &CheckerState<'_>, node: NodeId) -> Option<NodeId> {
    state.binder.node_record(node).parent
}

/// tsgo `ScriptElementKind`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ScriptElementKind {
    Unknown,
    Warning,
    /// A predefined type (`void`) or keyword (`class`).
    Keyword,
    /// A top-level script node.
    ScriptElement,
    /// `module foo {}`
    ModuleElement,
    /// `class X {}`
    ClassElement,
    /// `var x = class X {}`
    LocalClassElement,
    /// `interface Y {}`
    InterfaceElement,
    /// `type T = ...`
    TypeElement,
    /// `enum E {}`
    EnumElement,
    EnumMemberElement,
    /// `const v = ...` in a module or script.
    VariableElement,
    /// A variable inside a function.
    LocalVariableElement,
    /// `using foo = ...`
    VariableUsingElement,
    /// `await using foo = ...`
    VariableAwaitUsingElement,
    /// `function f() {}` in a module or script.
    FunctionElement,
    /// A function inside a function.
    LocalFunctionElement,
    /// `class X { foo() {} }`
    MemberFunctionElement,
    MemberGetAccessorElement,
    MemberSetAccessorElement,
    /// `class X { foo: number; }`, `interface Y { foo: number; }`
    MemberVariableElement,
    /// `class X { accessor foo: number; }`
    MemberAccessorVariableElement,
    /// `class X { constructor() {} }`, `class X { static {} }`
    ConstructorImplementationElement,
    /// `interface Y { (): number; }`
    CallSignatureElement,
    /// `interface Y { []: number; }`
    IndexSignatureElement,
    /// `interface Y { new (): Y; }`
    ConstructSignatureElement,
    ParameterElement,
    TypeParameterElement,
    PrimitiveType,
    Label,
    Alias,
    ConstElement,
    LetElement,
    Directory,
    ExternalModuleName,
    String,
    /// The `{@link ` and `}` around a JSDoc link.
    Link,
    /// A JSDoc link's entity name.
    LinkName,
    /// A JSDoc link's text.
    LinkText,
}

/// tsgo `ScriptElementKindModifier`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScriptElementKindModifier(pub u32);

impl ScriptElementKindModifier {
    pub const NONE: Self = Self(0);
    pub const PUBLIC: Self = Self(1 << 1);
    pub const PRIVATE: Self = Self(1 << 2);
    pub const PROTECTED: Self = Self(1 << 3);
    pub const EXPORTED: Self = Self(1 << 4);
    pub const AMBIENT: Self = Self(1 << 5);
    pub const STATIC: Self = Self(1 << 6);
    pub const ABSTRACT: Self = Self(1 << 7);
    pub const OPTIONAL: Self = Self(1 << 8);
    pub const DEPRECATED: Self = Self(1 << 9);
    pub const DTS: Self = Self(1 << 10);
    pub const TS: Self = Self(1 << 11);
    pub const TSX: Self = Self(1 << 12);
    pub const JS: Self = Self(1 << 13);
    pub const JSX: Self = Self(1 << 14);
    pub const JSON: Self = Self(1 << 15);
    pub const DMTS: Self = Self(1 << 16);
    pub const MTS: Self = Self(1 << 17);
    pub const MJS: Self = Self(1 << 18);
    pub const DCTS: Self = Self(1 << 19);
    pub const CTS: Self = Self(1 << 20);
    pub const CJS: Self = Self(1 << 21);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ScriptElementKindModifier {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for ScriptElementKindModifier {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// tsgo `GetSymbolKind`. `with_checker` is tsgo's non-nil type checker:
/// without it (the auto-import extractor's nil checker) only the symbol's
/// binding is read.
pub fn get_symbol_kind(
    state: &mut CheckerState<'_>,
    symbol: SymbolId,
    location: NodeId,
    with_checker: bool,
) -> CheckResult<ScriptElementKind> {
    let result = constructor_property_method_accessor_function_or_var_kind(
        state,
        symbol,
        location,
        with_checker,
    )?;
    if result != ScriptElementKind::Unknown {
        return Ok(result);
    }
    let flags = combined_local_and_export_symbol_flags(state, symbol);
    Ok(if flags.intersects(SymbolFlags::CLASS) {
        if declaration_of_kind(state, symbol, SyntaxKind::ClassExpression).is_some() {
            ScriptElementKind::LocalClassElement
        } else {
            ScriptElementKind::ClassElement
        }
    } else if flags.intersects(SymbolFlags::ENUM) {
        ScriptElementKind::EnumElement
    } else if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        ScriptElementKind::TypeElement
    } else if flags.intersects(SymbolFlags::INTERFACE) {
        ScriptElementKind::InterfaceElement
    } else if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        ScriptElementKind::TypeParameterElement
    } else if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        ScriptElementKind::EnumMemberElement
    } else if flags.intersects(SymbolFlags::ALIAS) {
        ScriptElementKind::Alias
    } else if flags.intersects(SymbolFlags::MODULE) {
        ScriptElementKind::ModuleElement
    } else {
        ScriptElementKind::Unknown
    })
}

/// tsgo `getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar`.
fn constructor_property_method_accessor_function_or_var_kind(
    state: &mut CheckerState<'_>,
    symbol: SymbolId,
    location: NodeId,
    with_checker: bool,
) -> CheckResult<ScriptElementKind> {
    let roots = if with_checker {
        state.get_root_symbols(symbol)?
    } else {
        vec![symbol]
    };
    // A method from a mapped type stays a method while it still has a call
    // signature (not in `{ [K in keyof I]: number }`).
    if let [root] = roots.as_slice() {
        if state.symbol_flags(*root).intersects(SymbolFlags::METHOD) {
            if !with_checker {
                return Ok(ScriptElementKind::MemberFunctionElement);
            }
            let ty = state.get_type_of_symbol_at_location(symbol, Some(location))?;
            let ty = state.get_non_nullable_type(ty)?;
            if !state
                .get_signatures_of_type(ty, SignatureKind::Call)?
                .is_empty()
            {
                return Ok(ScriptElementKind::MemberFunctionElement);
            }
        }
    }
    if with_checker {
        if state.is_undefined_symbol(symbol) {
            return Ok(ScriptElementKind::VariableElement);
        }
        if state.is_arguments_symbol(symbol) {
            return Ok(ScriptElementKind::LocalVariableElement);
        }
        if kind_of(state, location) == SyntaxKind::ThisKeyword
            && state.is_expression_node_like_tsgo(location)
            || state.is_this_in_type_query(location)
        {
            return Ok(ScriptElementKind::ParameterElement);
        }
    }

    let flags = combined_local_and_export_symbol_flags(state, symbol);
    if flags.intersects(SymbolFlags::VARIABLE) {
        let value_declaration = state.binder.symbol(symbol).value_declaration;
        let list_flags =
            value_declaration.map(|declaration| variable_list_flags(state, declaration));
        return Ok(if is_first_declaration_of_symbol_parameter(state, symbol) {
            ScriptElementKind::ParameterElement
        } else if list_flags.is_some_and(|flags| block_scope_kind(flags) == NodeFlags::CONST) {
            ScriptElementKind::ConstElement
        } else if list_flags.is_some_and(|flags| block_scope_kind(flags) == NodeFlags::USING) {
            ScriptElementKind::VariableUsingElement
        } else if list_flags.is_some_and(|flags| block_scope_kind(flags) == NodeFlags::AWAIT_USING)
        {
            ScriptElementKind::VariableAwaitUsingElement
        } else if state
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| {
                block_scope_kind(variable_list_flags(state, declaration)) == NodeFlags::LET
            })
        {
            ScriptElementKind::LetElement
        } else if is_local_variable_or_function(state, symbol) {
            ScriptElementKind::LocalVariableElement
        } else {
            ScriptElementKind::VariableElement
        });
    }
    if flags.intersects(SymbolFlags::FUNCTION) {
        return Ok(if is_local_variable_or_function(state, symbol) {
            ScriptElementKind::LocalFunctionElement
        } else {
            ScriptElementKind::FunctionElement
        });
    }
    // A getter and setter share one symbol, which nearly always has the
    // getter flag (tsgo's FIXME).
    if flags.intersects(SymbolFlags::GET_ACCESSOR) {
        return Ok(ScriptElementKind::MemberGetAccessorElement);
    }
    if flags.intersects(SymbolFlags::SET_ACCESSOR) {
        return Ok(ScriptElementKind::MemberSetAccessorElement);
    }
    if flags.intersects(SymbolFlags::METHOD) {
        return Ok(ScriptElementKind::MemberFunctionElement);
    }
    if flags.intersects(SymbolFlags::CONSTRUCTOR) {
        return Ok(ScriptElementKind::ConstructorImplementationElement);
    }
    if flags.intersects(SymbolFlags::SIGNATURE) {
        return Ok(ScriptElementKind::IndexSignatureElement);
    }
    if flags.intersects(SymbolFlags::PROPERTY) {
        if with_checker
            && flags.intersects(SymbolFlags::TRANSIENT)
            && state
                .get_check_flags(symbol)
                .intersects(CheckFlags::SYNTHETIC)
        {
            // A union property of a non-method (a property, accessor or
            // variable) is a property.
            if roots.iter().any(|&root| {
                state
                    .symbol_flags(root)
                    .intersects(SymbolFlags::PROPERTY_OR_ACCESSOR | SymbolFlags::VARIABLE)
            }) {
                return Ok(ScriptElementKind::MemberVariableElement);
            }
            // A union of methods is a method when it has call signatures.
            let ty = state.get_type_of_symbol_at_location(symbol, Some(location))?;
            return Ok(
                if state
                    .get_signatures_of_type(ty, SignatureKind::Call)?
                    .is_empty()
                {
                    ScriptElementKind::MemberVariableElement
                } else {
                    ScriptElementKind::MemberFunctionElement
                },
            );
        }
        return Ok(ScriptElementKind::MemberVariableElement);
    }
    Ok(ScriptElementKind::Unknown)
}

/// tsgo `Symbol.CombinedLocalAndExportSymbolFlags`.
fn combined_local_and_export_symbol_flags(
    state: &CheckerState<'_>,
    symbol: SymbolId,
) -> SymbolFlags {
    let data = state.binder.symbol(symbol);
    match data.export_symbol {
        Some(export) => data.flags | state.binder.symbol(export).flags,
        None => data.flags,
    }
}

fn declaration_of_kind(
    state: &CheckerState<'_>,
    symbol: SymbolId,
    kind: SyntaxKind,
) -> Option<NodeId> {
    state
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| kind_of(state, declaration) == kind)
}

/// The block-scope flags of a variable declaration's list (tsgo
/// `IsVarConst`, `IsVarUsing`, `IsVarAwaitUsing`, `IsLet`).
fn variable_list_flags(state: &CheckerState<'_>, declaration: NodeId) -> NodeFlags {
    if kind_of(state, declaration) != SyntaxKind::VariableDeclaration {
        return NodeFlags::NONE;
    }
    parent_of(state, declaration)
        .filter(|&list| kind_of(state, list) == SyntaxKind::VariableDeclarationList)
        .map_or(NodeFlags::NONE, |list| state.binder.flags_of(list))
}

fn block_scope_kind(flags: NodeFlags) -> NodeFlags {
    flags & NodeFlags::BLOCK_SCOPED
}

/// tsgo `isFirstDeclarationOfSymbolParameter`: the first declaration is a
/// parameter or inside a parameter's binding pattern.
fn is_first_declaration_of_symbol_parameter(state: &CheckerState<'_>, symbol: SymbolId) -> bool {
    let mut node = state.binder.symbol(symbol).declarations.first().copied();
    while let Some(current) = node {
        match kind_of(state, current) {
            SyntaxKind::Parameter => return true,
            SyntaxKind::BindingElement
            | SyntaxKind::ObjectBindingPattern
            | SyntaxKind::ArrayBindingPattern => node = parent_of(state, current),
            _ => return false,
        }
    }
    false
}

/// tsgo `isLocalVariableOrFunction`: declared inside a function body (or a
/// function expression).
fn is_local_variable_or_function(state: &CheckerState<'_>, symbol: SymbolId) -> bool {
    let data = state.binder.symbol(symbol);
    if data.parent.is_some() {
        // An exported symbol.
        return false;
    }
    for &declaration in data.declarations.iter() {
        match kind_of(state, declaration) {
            // A function expression is local.
            SyntaxKind::FunctionExpression => return true,
            SyntaxKind::VariableDeclaration | SyntaxKind::FunctionDeclaration => {}
            _ => continue,
        }
        let mut parent = parent_of(state, declaration);
        while let Some(current) = parent {
            if is_function_block(state, current) {
                return true;
            }
            if matches!(
                kind_of(state, current),
                SyntaxKind::SourceFile | SyntaxKind::ModuleBlock
            ) {
                break;
            }
            parent = parent_of(state, current);
        }
    }
    false
}

/// tsgo `ast.IsFunctionBlock`: a block that is a function-like's body.
fn is_function_block(state: &CheckerState<'_>, node: NodeId) -> bool {
    kind_of(state, node) == SyntaxKind::Block
        && parent_of(state, node)
            .is_some_and(|parent| crate::utilities::is_function_like_kind(kind_of(state, parent)))
}

/// tsgo `GetSymbolModifiers`; `with_checker` as [`get_symbol_kind`]'s.
pub fn get_symbol_modifiers(
    state: &mut CheckerState<'_>,
    symbol: SymbolId,
    with_checker: bool,
) -> CheckResult<ScriptElementKindModifier> {
    let mut modifiers = normalized_symbol_modifiers(state, symbol);
    if with_checker && state.symbol_flags(symbol).intersects(SymbolFlags::ALIAS) {
        let resolved = state.get_aliased_symbol(symbol)?;
        if resolved != symbol {
            modifiers |= normalized_symbol_modifiers(state, resolved);
        }
    }
    if state.symbol_flags(symbol).intersects(SymbolFlags::OPTIONAL) {
        modifiers |= ScriptElementKindModifier::OPTIONAL;
    }
    Ok(modifiers)
}

/// tsgo `getNormalizedSymbolModifiers`: the first declaration's, without
/// `deprecated` when some later declaration is not deprecated.
fn normalized_symbol_modifiers(
    state: &CheckerState<'_>,
    symbol: SymbolId,
) -> ScriptElementKindModifier {
    let declarations = &state.binder.symbol(symbol).declarations;
    let Some((&declaration, rest)) = declarations.split_first() else {
        return ScriptElementKindModifier::NONE;
    };
    let exclude = if !rest.is_empty()
        && state.is_deprecated_declaration(declaration)
        && rest
            .iter()
            .any(|&other| !state.is_deprecated_declaration(other))
    {
        ModifierFlags::DEPRECATED
    } else {
        ModifierFlags::NONE
    };
    node_modifiers(state, declaration, exclude)
}

/// tsgo `getNodeModifiers`.
fn node_modifiers(
    state: &CheckerState<'_>,
    node: NodeId,
    exclude: ModifierFlags,
) -> ScriptElementKindModifier {
    let source = state.binder.source_of_node(node);
    let mut flags = ModifierFlags::NONE;
    if node_util::is_declaration(source, node) {
        flags = node_util::get_combined_modifier_flags(source, node);
        if state.is_deprecated_declaration(node) {
            flags |= ModifierFlags::DEPRECATED;
        }
        flags = ModifierFlags::from_bits(flags.bits() & !exclude.bits());
    }
    let mut result = ScriptElementKindModifier::NONE;
    for (flag, modifier) in [
        (ModifierFlags::PRIVATE, ScriptElementKindModifier::PRIVATE),
        (
            ModifierFlags::PROTECTED,
            ScriptElementKindModifier::PROTECTED,
        ),
        (ModifierFlags::PUBLIC, ScriptElementKindModifier::PUBLIC),
        (ModifierFlags::STATIC, ScriptElementKindModifier::STATIC),
        (ModifierFlags::ABSTRACT, ScriptElementKindModifier::ABSTRACT),
        (ModifierFlags::EXPORT, ScriptElementKindModifier::EXPORTED),
        (
            ModifierFlags::DEPRECATED,
            ScriptElementKindModifier::DEPRECATED,
        ),
        (ModifierFlags::AMBIENT, ScriptElementKindModifier::AMBIENT),
    ] {
        if flags.intersects(flag) {
            result |= modifier;
        }
    }
    if state.binder.flags_of(node).intersects(NodeFlags::AMBIENT) {
        result |= ScriptElementKindModifier::AMBIENT;
    }
    if kind_of(state, node) == SyntaxKind::ExportAssignment {
        result |= ScriptElementKindModifier::EXPORTED;
    }
    result
}
