use tsc_diagnostics::{gen as d, Diagnostic, DiagnosticMessage, MessageChain};
use tsc_program::SourceFileId;
use tsc_syntax::{Node, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::{
    CommentRange, EmitHost, EmitResolverNode, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, SourceRange, TransformArena, TransformError, TransformNode,
    TransformSourceId,
};

use super::tracker::{DiagnosticArgument, DiagnosticSpec, TrackerAnchor};
use super::DeclarationTransformer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticContext {
    None,
    ForNode(TransformNode),
    ForNodeName(TransformNode),
    JsFile(TransformSourceId),
    DefaultExport(TransformNode),
    ClassExtends { class: TransformNode },
}

#[derive(Clone, Copy)]
enum MessageChoice {
    Fixed(&'static DiagnosticMessage),
    Module {
        private_module: &'static DiagnosticMessage,
        private_name: &'static DiagnosticMessage,
    },
    ExternalModule {
        cannot_be_named: &'static DiagnosticMessage,
        private_module: &'static DiagnosticMessage,
        private_name: &'static DiagnosticMessage,
    },
}

impl MessageChoice {
    fn select(self, result: &EmitSymbolAccessibilityResult) -> &'static DiagnosticMessage {
        match self {
            Self::Fixed(message) => message,
            Self::Module {
                private_module,
                private_name,
            } => {
                if result.error_module_name.is_some() {
                    private_module
                } else {
                    private_name
                }
            }
            Self::ExternalModule {
                cannot_be_named,
                private_module,
                private_name,
            } => {
                if result.error_module_name.is_none() {
                    private_name
                } else if result.accessibility == EmitSymbolAccessibility::CannotBeNamed {
                    cannot_be_named
                } else {
                    private_module
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct DiagnosticTemplate {
    message: MessageChoice,
    error_node: TrackerAnchor,
    type_name: Option<TrackerAnchor>,
}

#[derive(Clone)]
pub(crate) enum DiagnosticContextPlan {
    None,
    NoDiagnostic,
    Template(DiagnosticTemplate),
    JsFile { fallback: TrackerAnchor },
}

impl DiagnosticContext {
    /// tsrs-native: stable diagnostic plan captured before a resolver
    /// callback releases its transform-arena borrow.
    pub(crate) fn plan(
        self,
        arena: &TransformArena,
    ) -> Result<DiagnosticContextPlan, TransformError> {
        match self {
            Self::None => Ok(DiagnosticContextPlan::None),
            Self::ForNode(node) if arena.node(node)?.kind == SyntaxKind::Constructor => {
                // createGetSymbolAccessibilityDiagnosticForNode routes a
                // constructor through getVariableDeclarationTypeVisibilityError,
                // whose message selector intentionally returns undefined.
                Ok(DiagnosticContextPlan::NoDiagnostic)
            }
            Self::ForNode(node) => Ok(DiagnosticContextPlan::Template(
                create_get_symbol_accessibility_diagnostic_for_node(arena, node)?,
            )),
            Self::ForNodeName(node) => Ok(DiagnosticContextPlan::Template(
                create_get_symbol_accessibility_diagnostic_for_node_name(arena, node)?,
            )),
            Self::JsFile(source) => Ok(DiagnosticContextPlan::JsFile {
                fallback: TrackerAnchor::Transform(arena.root(source)?),
            }),
            Self::DefaultExport(node) => Ok(DiagnosticContextPlan::Template(DiagnosticTemplate {
                message: MessageChoice::Fixed(
                    &d::Default_export_of_the_module_has_or_is_using_private_name_0,
                ),
                error_node: TrackerAnchor::Transform(node),
                type_name: None,
            })),
            Self::ClassExtends { class } => {
                let source = arena.source(class.source())?.syntax();
                let name = name_of_declaration(source, class.node());
                Ok(DiagnosticContextPlan::Template(DiagnosticTemplate {
                    message: MessageChoice::Fixed(
                        &d::extends_clause_of_exported_class_0_has_or_is_using_private_name_1,
                    ),
                    error_node: TrackerAnchor::Transform(class),
                    type_name: name.map(|name| {
                        TrackerAnchor::Transform(TransformNode::new(class.source(), name))
                    }),
                }))
            }
        }
    }
}

impl DiagnosticContextPlan {
    /// tsrs-native: lazily select and populate a symbol-accessibility
    /// diagnostic from a callback result.
    pub(crate) fn resolve(
        &self,
        host: &dyn EmitHost,
        result: &EmitSymbolAccessibilityResult,
    ) -> Result<Option<DiagnosticSpec>, TransformError> {
        let template = match self {
            Self::None => {
                return Err(DeclarationTransformer::contract(
                    "diagnostic emitted without a declaration diagnostic context",
                ))
            }
            Self::NoDiagnostic => return Ok(None),
            Self::Template(template) => template.clone(),
            Self::JsFile { fallback } => {
                if let Some(error) = result.error_node {
                    if let Some(source) = host.source_file(error.source()) {
                        if let Some(syntax) = source.syntax() {
                            let kind = syntax.arena.node(error.node()).kind;
                            if kind == SyntaxKind::Constructor {
                                return Ok(None);
                            }
                            if can_produce_diagnostics(kind) {
                                template_for_node(
                                    syntax,
                                    error.node(),
                                    AnchorFactory::Resolver(error.source()),
                                )?
                            } else {
                                js_file_template(result, TrackerAnchor::resolver(error))
                            }
                        } else {
                            js_file_template(result, TrackerAnchor::resolver(error))
                        }
                    } else {
                        js_file_template(result, TrackerAnchor::resolver(error))
                    }
                } else {
                    js_file_template(result, fallback.clone())
                }
            }
        };

        let anchor = result
            .error_node
            .map(TrackerAnchor::resolver)
            .unwrap_or_else(|| template.error_node.clone());
        let mut args = Vec::new();
        if let Some(type_name) = template.type_name {
            args.push(DiagnosticArgument::NodeText(type_name));
        }
        args.push(DiagnosticArgument::Text(
            result.error_symbol_name.clone().unwrap_or_default(),
        ));
        args.push(DiagnosticArgument::Text(
            result.error_module_name.clone().unwrap_or_default(),
        ));
        Ok(Some(DiagnosticSpec {
            message: template.message.select(result),
            args,
            anchor,
            related: Vec::new(),
        }))
    }
}

fn js_file_template(
    result: &EmitSymbolAccessibilityResult,
    error_node: TrackerAnchor,
) -> DiagnosticTemplate {
    DiagnosticTemplate {
        message: MessageChoice::Fixed(if result.error_module_name.is_some() {
            &d::Declaration_emit_for_this_file_requires_using_private_name_0_from_module_1_An_explicit_type_annotation_may_unblock_declaration_emit
        } else {
            &d::Declaration_emit_for_this_file_requires_using_private_name_0_An_explicit_type_annotation_may_unblock_declaration_emit
        }),
        error_node,
        type_name: None,
    }
}

#[derive(Clone, Copy)]
enum AnchorFactory {
    Transform(TransformSourceId),
    Resolver(SourceFileId),
}

impl AnchorFactory {
    fn anchor(self, node: NodeId) -> TrackerAnchor {
        match self {
            Self::Transform(source) => TrackerAnchor::Transform(TransformNode::new(source, node)),
            Self::Resolver(source) => TrackerAnchor::resolver(EmitResolverNode::new(source, node)),
        }
    }
}

/// tsc-port: canProduceDiagnostics @6.0.3
/// tsc-hash: 9c62bfcc13fb7af3d9b424d453144e220984abcef07edfd90dd3186730cd7e07
/// tsc-span: _tsc.js:113796-113798
pub(crate) const fn can_produce_diagnostics(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::VariableDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::BindingElement
            | SyntaxKind::SetAccessor
            | SyntaxKind::GetAccessor
            | SyntaxKind::ConstructSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Parameter
            | SyntaxKind::TypeParameter
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::IndexSignature
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocEnumTag
    )
}

/// tsc-port: createGetSymbolAccessibilityDiagnosticForNodeName @6.0.3
/// tsc-hash: f6b4c9cfd747948485e5121d302e3f871eab125a25fedcf1ebad3adc946ecb6d
/// tsc-span: _tsc.js:113799-113841
fn create_get_symbol_accessibility_diagnostic_for_node_name(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<DiagnosticTemplate, TransformError> {
    let source = arena.source(node.source())?.syntax();
    let kind = source.arena.node(node.node()).kind;
    if matches!(kind, SyntaxKind::SetAccessor | SyntaxKind::GetAccessor) {
        let message = if is_static(source, node.node()) {
            external(
                &d::Public_static_property_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                &d::Public_static_property_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
                &d::Public_static_property_0_of_exported_class_has_or_is_using_private_name_1,
            )
        } else if parent_kind(source, node.node()) == Some(SyntaxKind::ClassDeclaration) {
            external(
                &d::Public_property_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                &d::Public_property_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
                &d::Public_property_0_of_exported_class_has_or_is_using_private_name_1,
            )
        } else {
            module(
                &d::Property_0_of_exported_interface_has_or_is_using_name_1_from_private_module_2,
                &d::Property_0_of_exported_interface_has_or_is_using_private_name_1,
            )
        };
        return Ok(named_template(
            source,
            node.node(),
            message,
            AnchorFactory::Transform(node.source()),
        ));
    }
    if matches!(
        kind,
        SyntaxKind::MethodSignature | SyntaxKind::MethodDeclaration
    ) {
        let message = if is_static(source, node.node()) {
            external(
                &d::Public_static_method_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                &d::Public_static_method_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
                &d::Public_static_method_0_of_exported_class_has_or_is_using_private_name_1,
            )
        } else if parent_kind(source, node.node()) == Some(SyntaxKind::ClassDeclaration) {
            external(
                &d::Public_method_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                &d::Public_method_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
                &d::Public_method_0_of_exported_class_has_or_is_using_private_name_1,
            )
        } else {
            module(
                &d::Method_0_of_exported_interface_has_or_is_using_name_1_from_private_module_2,
                &d::Method_0_of_exported_interface_has_or_is_using_private_name_1,
            )
        };
        return Ok(named_template(
            source,
            node.node(),
            message,
            AnchorFactory::Transform(node.source()),
        ));
    }
    create_get_symbol_accessibility_diagnostic_for_node(arena, node)
}

/// tsc-port: createGetSymbolAccessibilityDiagnosticForNode @6.0.3
/// tsc-hash: 7b6f0f6df3d5ee79f4b1f3da4df3c03d8655581fcc7d81511b9f892fdf797c15
/// tsc-span: _tsc.js:113842-114053
fn create_get_symbol_accessibility_diagnostic_for_node(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<DiagnosticTemplate, TransformError> {
    let source = arena.source(node.source())?.syntax();
    template_for_node(source, node.node(), AnchorFactory::Transform(node.source()))
}

fn template_for_node(
    source: &SourceFile,
    node: NodeId,
    anchors: AnchorFactory,
) -> Result<DiagnosticTemplate, TransformError> {
    let kind = source.arena.node(node).kind;
    let template = match kind {
        SyntaxKind::VariableDeclaration | SyntaxKind::BindingElement => named_template(
            source,
            node,
            external(
                &d::Exported_variable_0_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                &d::Exported_variable_0_has_or_is_using_name_1_from_private_module_2,
                &d::Exported_variable_0_has_or_is_using_private_name_1,
            ),
            anchors,
        ),
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAccessExpression
        | SyntaxKind::ElementAccessExpression
        | SyntaxKind::BinaryExpression => {
            let message = property_message(source, node);
            named_template(source, node, message, anchors)
        }
        SyntaxKind::Parameter if parameter_is_private_property(source, node) => named_template(
            source,
            node,
            property_message(source, node),
            anchors,
        ),
        SyntaxKind::SetAccessor | SyntaxKind::GetAccessor => {
            let message = if kind == SyntaxKind::SetAccessor && is_static(source, node) {
                module(
                    &d::Parameter_type_of_public_static_setter_0_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Parameter_type_of_public_static_setter_0_from_exported_class_has_or_is_using_private_name_1,
                )
            } else if kind == SyntaxKind::SetAccessor {
                module(
                    &d::Parameter_type_of_public_setter_0_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Parameter_type_of_public_setter_0_from_exported_class_has_or_is_using_private_name_1,
                )
            } else if is_static(source, node) {
                external(
                    &d::Return_type_of_public_static_getter_0_from_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                    &d::Return_type_of_public_static_getter_0_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Return_type_of_public_static_getter_0_from_exported_class_has_or_is_using_private_name_1,
                )
            } else {
                external(
                    &d::Return_type_of_public_getter_0_from_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                    &d::Return_type_of_public_getter_0_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Return_type_of_public_getter_0_from_exported_class_has_or_is_using_private_name_1,
                )
            };
            let name = name_of_declaration(source, node).unwrap_or(node);
            DiagnosticTemplate {
                message,
                error_node: anchors.anchor(name),
                type_name: Some(anchors.anchor(name)),
            }
        }
        SyntaxKind::ConstructSignature => unnamed_template(
            node,
            module(
                &d::Return_type_of_constructor_signature_from_exported_interface_has_or_is_using_name_0_from_private_module_1,
                &d::Return_type_of_constructor_signature_from_exported_interface_has_or_is_using_private_name_0,
            ),
            anchors,
        ),
        SyntaxKind::CallSignature => unnamed_template(
            node,
            module(
                &d::Return_type_of_call_signature_from_exported_interface_has_or_is_using_name_0_from_private_module_1,
                &d::Return_type_of_call_signature_from_exported_interface_has_or_is_using_private_name_0,
            ),
            anchors,
        ),
        SyntaxKind::IndexSignature => unnamed_template(
            node,
            module(
                &d::Return_type_of_index_signature_from_exported_interface_has_or_is_using_name_0_from_private_module_1,
                &d::Return_type_of_index_signature_from_exported_interface_has_or_is_using_private_name_0,
            ),
            anchors,
        ),
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
            let message = if is_static(source, node) {
                external(
                    &d::Return_type_of_public_static_method_from_exported_class_has_or_is_using_name_0_from_external_module_1_but_cannot_be_named,
                    &d::Return_type_of_public_static_method_from_exported_class_has_or_is_using_name_0_from_private_module_1,
                    &d::Return_type_of_public_static_method_from_exported_class_has_or_is_using_private_name_0,
                )
            } else if parent_kind(source, node) == Some(SyntaxKind::ClassDeclaration) {
                external(
                    &d::Return_type_of_public_method_from_exported_class_has_or_is_using_name_0_from_external_module_1_but_cannot_be_named,
                    &d::Return_type_of_public_method_from_exported_class_has_or_is_using_name_0_from_private_module_1,
                    &d::Return_type_of_public_method_from_exported_class_has_or_is_using_private_name_0,
                )
            } else {
                module(
                    &d::Return_type_of_method_from_exported_interface_has_or_is_using_name_0_from_private_module_1,
                    &d::Return_type_of_method_from_exported_interface_has_or_is_using_private_name_0,
                )
            };
            DiagnosticTemplate {
                message,
                error_node: anchors.anchor(name_of_declaration(source, node).unwrap_or(node)),
                type_name: None,
            }
        }
        SyntaxKind::FunctionDeclaration => DiagnosticTemplate {
            message: external(
                &d::Return_type_of_exported_function_has_or_is_using_name_0_from_external_module_1_but_cannot_be_named,
                &d::Return_type_of_exported_function_has_or_is_using_name_0_from_private_module_1,
                &d::Return_type_of_exported_function_has_or_is_using_private_name_0,
            ),
            error_node: anchors.anchor(name_of_declaration(source, node).unwrap_or(node)),
            type_name: None,
        },
        SyntaxKind::Parameter => parameter_template(source, node, anchors)?,
        SyntaxKind::TypeParameter => type_parameter_template(source, node, anchors)?,
        SyntaxKind::ExpressionWithTypeArguments => heritage_template(source, node, anchors)?,
        SyntaxKind::ImportEqualsDeclaration => named_template(
            source,
            node,
            MessageChoice::Fixed(&d::Import_declaration_0_is_using_private_name_1),
            anchors,
        ),
        SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::JSDocTypedefTag
        | SyntaxKind::JSDocCallbackTag
        | SyntaxKind::JSDocEnumTag => {
            let (error, name) = if kind == SyntaxKind::TypeAliasDeclaration {
                let NodeData::TypeAliasDeclaration(data) = &source.arena.node(node).data else {
                    return Err(DeclarationTransformer::contract(
                        "type-alias diagnostic kind/data mismatch",
                    ));
                };
                (data.r#type, data.name)
            } else {
                (jsdoc_type_expression(source, node), name_of_declaration(source, node))
            };
            DiagnosticTemplate {
                message: module(
                    &d::Exported_type_alias_0_has_or_is_using_private_name_1_from_module_2,
                    &d::Exported_type_alias_0_has_or_is_using_private_name_1,
                ),
                error_node: anchors.anchor(error.unwrap_or(node)),
                type_name: name.map(|name| anchors.anchor(name)),
            }
        }
        _ => return Err(DeclarationTransformer::contract(
            "unhandled declaration diagnostic context node",
        )),
    };
    Ok(template)
}

fn property_message(source: &SourceFile, node: NodeId) -> MessageChoice {
    if is_static(source, node) {
        external(
            &d::Public_static_property_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
            &d::Public_static_property_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
            &d::Public_static_property_0_of_exported_class_has_or_is_using_private_name_1,
        )
    } else if parent_kind(source, node) == Some(SyntaxKind::ClassDeclaration)
        || source.arena.node(node).kind == SyntaxKind::Parameter
    {
        external(
            &d::Public_property_0_of_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
            &d::Public_property_0_of_exported_class_has_or_is_using_name_1_from_private_module_2,
            &d::Public_property_0_of_exported_class_has_or_is_using_private_name_1,
        )
    } else {
        module(
            &d::Property_0_of_exported_interface_has_or_is_using_name_1_from_private_module_2,
            &d::Property_0_of_exported_interface_has_or_is_using_private_name_1,
        )
    }
}

fn parameter_template(
    source: &SourceFile,
    node: NodeId,
    anchors: AnchorFactory,
) -> Result<DiagnosticTemplate, TransformError> {
    let parent_node = parent(source, node).ok_or_else(|| {
        DeclarationTransformer::contract("parameter diagnostic context has no parent")
    })?;
    let parent_kind = source.arena.node(parent_node).kind;
    let message = match parent_kind {
        SyntaxKind::Constructor => external(
            &d::Parameter_0_of_constructor_from_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
            &d::Parameter_0_of_constructor_from_exported_class_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_constructor_from_exported_class_has_or_is_using_private_name_1,
        ),
        SyntaxKind::ConstructSignature | SyntaxKind::ConstructorType => module(
            &d::Parameter_0_of_constructor_signature_from_exported_interface_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_constructor_signature_from_exported_interface_has_or_is_using_private_name_1,
        ),
        SyntaxKind::CallSignature => module(
            &d::Parameter_0_of_call_signature_from_exported_interface_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_call_signature_from_exported_interface_has_or_is_using_private_name_1,
        ),
        SyntaxKind::IndexSignature => module(
            &d::Parameter_0_of_index_signature_from_exported_interface_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_index_signature_from_exported_interface_has_or_is_using_private_name_1,
        ),
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
            if is_static(source, parent_node) {
                external(
                    &d::Parameter_0_of_public_static_method_from_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                    &d::Parameter_0_of_public_static_method_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Parameter_0_of_public_static_method_from_exported_class_has_or_is_using_private_name_1,
                )
            } else if parent(source, parent_node)
                .is_some_and(|owner| source.arena.node(owner).kind == SyntaxKind::ClassDeclaration)
            {
                external(
                    &d::Parameter_0_of_public_method_from_exported_class_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
                    &d::Parameter_0_of_public_method_from_exported_class_has_or_is_using_name_1_from_private_module_2,
                    &d::Parameter_0_of_public_method_from_exported_class_has_or_is_using_private_name_1,
                )
            } else {
                module(
                    &d::Parameter_0_of_method_from_exported_interface_has_or_is_using_name_1_from_private_module_2,
                    &d::Parameter_0_of_method_from_exported_interface_has_or_is_using_private_name_1,
                )
            }
        }
        SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionType => external(
            &d::Parameter_0_of_exported_function_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
            &d::Parameter_0_of_exported_function_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_exported_function_has_or_is_using_private_name_1,
        ),
        SyntaxKind::SetAccessor | SyntaxKind::GetAccessor => external(
            &d::Parameter_0_of_accessor_has_or_is_using_name_1_from_external_module_2_but_cannot_be_named,
            &d::Parameter_0_of_accessor_has_or_is_using_name_1_from_private_module_2,
            &d::Parameter_0_of_accessor_has_or_is_using_private_name_1,
        ),
        _ => return Err(DeclarationTransformer::contract(
            "unknown parent for declaration parameter diagnostic",
        )),
    };
    Ok(named_template(source, node, message, anchors))
}

fn type_parameter_template(
    source: &SourceFile,
    node: NodeId,
    anchors: AnchorFactory,
) -> Result<DiagnosticTemplate, TransformError> {
    let parent = parent(source, node).ok_or_else(|| {
        DeclarationTransformer::contract("type-parameter diagnostic context has no parent")
    })?;
    let parent_kind = source.arena.node(parent).kind;
    let message = match parent_kind {
        SyntaxKind::ClassDeclaration => &d::Type_parameter_0_of_exported_class_has_or_is_using_private_name_1,
        SyntaxKind::InterfaceDeclaration => &d::Type_parameter_0_of_exported_interface_has_or_is_using_private_name_1,
        SyntaxKind::MappedType => &d::Type_parameter_0_of_exported_mapped_object_type_is_using_private_name_1,
        SyntaxKind::ConstructorType | SyntaxKind::ConstructSignature => &d::Type_parameter_0_of_constructor_signature_from_exported_interface_has_or_is_using_private_name_1,
        SyntaxKind::CallSignature => &d::Type_parameter_0_of_call_signature_from_exported_interface_has_or_is_using_private_name_1,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature if is_static(source, parent) => &d::Type_parameter_0_of_public_static_method_from_exported_class_has_or_is_using_private_name_1,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature
            if super_parent_kind(source, node) == Some(SyntaxKind::ClassDeclaration) => &d::Type_parameter_0_of_public_method_from_exported_class_has_or_is_using_private_name_1,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => &d::Type_parameter_0_of_method_from_exported_interface_has_or_is_using_private_name_1,
        SyntaxKind::FunctionType | SyntaxKind::FunctionDeclaration => &d::Type_parameter_0_of_exported_function_has_or_is_using_private_name_1,
        SyntaxKind::InferType => &d::Extends_clause_for_inferred_type_0_has_or_is_using_private_name_1,
        SyntaxKind::TypeAliasDeclaration => &d::Type_parameter_0_of_exported_type_alias_has_or_is_using_private_name_1,
        _ => return Err(DeclarationTransformer::contract(
            "unknown parent for declaration type-parameter diagnostic",
        )),
    };
    Ok(named_template(
        source,
        node,
        MessageChoice::Fixed(message),
        anchors,
    ))
}

fn heritage_template(
    source: &SourceFile,
    node: NodeId,
    anchors: AnchorFactory,
) -> Result<DiagnosticTemplate, TransformError> {
    let clause = parent(source, node).ok_or_else(|| {
        DeclarationTransformer::contract("heritage diagnostic context has no clause")
    })?;
    let declaration = parent(source, clause).ok_or_else(|| {
        DeclarationTransformer::contract("heritage diagnostic context has no declaration")
    })?;
    let declaration_kind = source.arena.node(declaration).kind;
    let message = if declaration_kind == SyntaxKind::ClassDeclaration {
        if matches!(
            &source.arena.node(clause).data,
            NodeData::HeritageClause(data) if data.token == SyntaxKind::ImplementsKeyword
        ) {
            &d::Implements_clause_of_exported_class_0_has_or_is_using_private_name_1
        } else if name_of_declaration(source, declaration).is_some() {
            &d::extends_clause_of_exported_class_0_has_or_is_using_private_name_1
        } else {
            &d::extends_clause_of_exported_class_has_or_is_using_private_name_0
        }
    } else {
        &d::extends_clause_of_exported_interface_0_has_or_is_using_private_name_1
    };
    Ok(DiagnosticTemplate {
        message: MessageChoice::Fixed(message),
        error_node: anchors.anchor(node),
        type_name: name_of_declaration(source, declaration).map(|name| anchors.anchor(name)),
    })
}

fn named_template(
    source: &SourceFile,
    node: NodeId,
    message: MessageChoice,
    anchors: AnchorFactory,
) -> DiagnosticTemplate {
    DiagnosticTemplate {
        message,
        error_node: anchors.anchor(node),
        type_name: direct_name_of_declaration(source, node).map(|name| anchors.anchor(name)),
    }
}

fn unnamed_template(
    node: NodeId,
    message: MessageChoice,
    anchors: AnchorFactory,
) -> DiagnosticTemplate {
    DiagnosticTemplate {
        message,
        error_node: anchors.anchor(node),
        type_name: None,
    }
}

const fn module(
    private_module: &'static DiagnosticMessage,
    private_name: &'static DiagnosticMessage,
) -> MessageChoice {
    MessageChoice::Module {
        private_module,
        private_name,
    }
}

const fn external(
    cannot_be_named: &'static DiagnosticMessage,
    private_module: &'static DiagnosticMessage,
    private_name: &'static DiagnosticMessage,
) -> MessageChoice {
    MessageChoice::ExternalModule {
        cannot_be_named,
        private_module,
        private_name,
    }
}

fn parent(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    source.arena.node(node).parent
}

fn parent_kind(source: &SourceFile, node: NodeId) -> Option<SyntaxKind> {
    parent(source, node).map(|parent| source.arena.node(parent).kind)
}

fn super_parent_kind(source: &SourceFile, node: NodeId) -> Option<SyntaxKind> {
    parent(source, node)
        .and_then(|parent_node| parent(source, parent_node))
        .map(|parent_node| source.arena.node(parent_node).kind)
}

fn modifiers(node: &Node) -> Option<tsc_syntax::NodeArrayId> {
    match &node.data {
        NodeData::TypeParameter(data) => data.modifiers,
        NodeData::Parameter(data) => data.modifiers,
        NodeData::PropertySignature(data) => data.modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers,
        NodeData::MethodSignature(data) => data.modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers,
        NodeData::Constructor(data) => data.modifiers,
        NodeData::GetAccessor(data) => data.modifiers,
        NodeData::SetAccessor(data) => data.modifiers,
        NodeData::IndexSignature(data) => data.modifiers,
        NodeData::ConstructorType(data) => data.modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        _ => None,
    }
}

/// tsrs-native: declaration-local syntactic modifier projection.
pub(crate) fn effective_modifier_flags(source: &SourceFile, node: NodeId) -> ModifierFlags {
    let Some(modifiers) = modifiers(source.arena.node(node)) else {
        return ModifierFlags::NONE;
    };
    let mut flags = ModifierFlags::NONE;
    for &modifier in &source.arena.node_array(modifiers).nodes {
        flags |= match source.arena.node(modifier).kind {
            SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
            SyntaxKind::PrivateKeyword => ModifierFlags::PRIVATE,
            SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
            SyntaxKind::ReadonlyKeyword => ModifierFlags::READONLY,
            SyntaxKind::OverrideKeyword => ModifierFlags::OVERRIDE,
            SyntaxKind::ExportKeyword => ModifierFlags::EXPORT,
            SyntaxKind::AbstractKeyword => ModifierFlags::ABSTRACT,
            SyntaxKind::DeclareKeyword => ModifierFlags::AMBIENT,
            SyntaxKind::StaticKeyword => ModifierFlags::STATIC,
            SyntaxKind::AccessorKeyword => ModifierFlags::ACCESSOR,
            SyntaxKind::AsyncKeyword => ModifierFlags::ASYNC,
            SyntaxKind::DefaultKeyword => ModifierFlags::DEFAULT,
            SyntaxKind::ConstKeyword => ModifierFlags::CONST,
            SyntaxKind::InKeyword => ModifierFlags::IN,
            SyntaxKind::OutKeyword => ModifierFlags::OUT,
            _ => ModifierFlags::NONE,
        };
    }
    flags
}

fn is_static(source: &SourceFile, node: NodeId) -> bool {
    effective_modifier_flags(source, node).contains(ModifierFlags::STATIC)
}

fn parameter_is_private_property(source: &SourceFile, node: NodeId) -> bool {
    parent(source, node).is_some_and(|parent| {
        source.arena.node(parent).kind == SyntaxKind::Constructor
            && effective_modifier_flags(source, node)
                .intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
            && effective_modifier_flags(source, parent).contains(ModifierFlags::PRIVATE)
    })
}

/// tsrs-native: declaration name projection used by diagnostic templates.
pub(crate) fn name_of_declaration(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::Identifier(_) => Some(node),
        NodeData::VariableDeclaration(data) => data.name,
        NodeData::BindingElement(data) => data.name,
        NodeData::ClassDeclaration(data) => data.name,
        NodeData::ClassExpression(data) => data.name,
        NodeData::EnumDeclaration(data) => data.name,
        NodeData::EnumMember(data) => data.name,
        NodeData::InterfaceDeclaration(data) => data.name,
        NodeData::TypeAliasDeclaration(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::FunctionExpression(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::Parameter(data) => data.name,
        NodeData::TypeParameter(data) => data.name,
        NodeData::ImportEqualsDeclaration(data) => data.name,
        NodeData::NamespaceImport(data) => data.name,
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::ElementAccessExpression(data) => data.argument_expression,
        NodeData::BinaryExpression(data) => data.left.and_then(|left| {
            access_expression_name(source, left).or_else(|| {
                (source.arena.node(left).kind == SyntaxKind::Identifier).then_some(left)
            })
        }),
        NodeData::JSDocTypedefTag(data) => data
            .name
            .or_else(|| name_for_nameless_jsdoc_alias(source, node)),
        NodeData::JSDocCallbackTag(data) => data.name,
        NodeData::JSDocEnumTag(_) => name_for_nameless_jsdoc_alias(source, node),
        _ => None,
    }
}

fn direct_name_of_declaration(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::BindingElement(data) => data.name,
        NodeData::ClassDeclaration(data) => data.name,
        NodeData::ClassExpression(data) => data.name,
        NodeData::EnumDeclaration(data) => data.name,
        NodeData::EnumMember(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::FunctionExpression(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::ImportEqualsDeclaration(data) => data.name,
        NodeData::InterfaceDeclaration(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::ModuleDeclaration(data) => data.name,
        NodeData::NamespaceImport(data) => data.name,
        NodeData::Parameter(data) => data.name,
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::TypeAliasDeclaration(data) => data.name,
        NodeData::TypeParameter(data) => data.name,
        NodeData::VariableDeclaration(data) => data.name,
        NodeData::JSDocCallbackTag(data) => data.name,
        NodeData::JSDocTypedefTag(data) => data.name,
        _ => None,
    }
}

fn access_expression_name(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::ElementAccessExpression(data) => data.argument_expression,
        _ => None,
    }
}

fn name_for_nameless_jsdoc_alias(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    let host = parent(source, node).and_then(|doc| parent(source, doc))?;
    if let Some(name) = direct_name_of_declaration(source, host) {
        return (source.arena.node(name).kind == SyntaxKind::Identifier).then_some(name);
    }
    match &source.arena.node(host).data {
        NodeData::VariableStatement(data) => {
            let declaration = data.declaration_list.and_then(|list| {
                let NodeData::VariableDeclarationList(data) = &source.arena.node(list).data else {
                    return None;
                };
                data.declarations.and_then(|declarations| {
                    source.arena.node_array(declarations).nodes.first().copied()
                })
            })?;
            direct_name_of_declaration(source, declaration)
                .filter(|name| source.arena.node(*name).kind == SyntaxKind::Identifier)
        }
        NodeData::ExpressionStatement(data) => {
            let mut expression = data.expression?;
            if let NodeData::BinaryExpression(binary) = &source.arena.node(expression).data {
                if binary
                    .operator_token
                    .is_some_and(|token| source.arena.node(token).kind == SyntaxKind::EqualsToken)
                {
                    expression = binary.left?;
                }
            }
            access_expression_name(source, expression)
                .filter(|name| source.arena.node(*name).kind == SyntaxKind::Identifier)
        }
        _ => None,
    }
}

fn jsdoc_type_expression(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::JSDocTypedefTag(data) => data.type_expression,
        NodeData::JSDocCallbackTag(data) => data.type_expression,
        NodeData::JSDocEnumTag(data) => data.type_expression,
        _ => None,
    }
}

/// tsrs-native: declaration-transform diagnostic materialization over an
/// already-resolved parse-tree source and TypeScript error span.
pub(crate) fn diagnostic_for_source_node(
    source: &SourceFile,
    node: NodeId,
    message: &'static DiagnosticMessage,
    args: &[String],
) -> Diagnostic {
    let (start, end) = error_span_for_node(source, node);
    let to_utf16 = |byte: usize| {
        source
            .positions()
            .byte_to_utf16(byte as u32)
            .unwrap_or(byte as u32)
    };
    let start = to_utf16(start);
    let end = to_utf16(end);
    Diagnostic::new(
        Some(source.file_name.clone()),
        Some(start),
        Some(end.saturating_sub(start)),
        MessageChain::new(message, args),
    )
}

/// tsc-port: getErrorSpanForNode @6.0.3
/// tsc-hash: 2d2ca68c825de352e44893a3a69b54b87090a276ae158a59d05c5e3ebfec35dd
/// tsc-span: _tsc.js:14023-14115
fn error_span_for_node(source: &SourceFile, node: NodeId) -> (usize, usize) {
    let record = source.arena.node(node);
    let mut error_node = Some(node);
    match &record.data {
        NodeData::SourceFile(_) => {
            let start = tsc_syntax::skip_trivia(source.text(), 0);
            if start == source.text().len() {
                return (0, 0);
            }
            return token_span(source, start);
        }
        NodeData::ArrowFunction(data) => {
            return arrow_function_span(source, node, data.body);
        }
        NodeData::CaseClause(data) => {
            let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
            return case_clause_span(source, record.end as usize, start, data.statements);
        }
        NodeData::DefaultClause(data) => {
            let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
            return case_clause_span(source, record.end as usize, start, data.statements);
        }
        NodeData::ReturnStatement(_) | NodeData::YieldExpression(_) => {
            let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
            return token_span(source, start);
        }
        NodeData::SatisfiesExpression(data) => {
            if let Some(expression) = data.expression {
                let start = tsc_syntax::skip_trivia(
                    source.text(),
                    source.arena.node(expression).end as usize,
                );
                return token_span(source, start);
            }
        }
        NodeData::JSDocSatisfiesTag(data) => {
            if let Some(tag_name) = data.tag_name {
                let tag_name = source.arena.node(tag_name);
                let start = tsc_syntax::skip_trivia(source.text(), tag_name.pos as usize);
                return token_span(source, start);
            }
        }
        NodeData::Constructor(_) => {
            let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
            for token in tsc_syntax::scan_tokens(&source.text()[start..], source.language_variant) {
                if token.kind == SyntaxKind::ConstructorKeyword {
                    let end = source
                        .positions()
                        .byte_offset_from_utf16_delta(start as u32, token.end)
                        .unwrap_or(record.end) as usize;
                    return (start, end);
                }
            }
            return (start, record.end as usize);
        }
        _ => {
            if matches!(
                record.kind,
                SyntaxKind::VariableDeclaration
                    | SyntaxKind::BindingElement
                    | SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::EnumDeclaration
                    | SyntaxKind::EnumMember
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::PropertyDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::NamespaceImport
            ) {
                error_node = direct_name_of_declaration(source, node);
            }
        }
    }

    let Some(error_node) = error_node else {
        return token_span(source, record.pos as usize);
    };
    let error = source.arena.node(error_node);
    let is_missing = error.pos == error.end && error.kind != SyntaxKind::EndOfFileToken;
    let start = if is_missing || record.kind == SyntaxKind::JsxText {
        error.pos as usize
    } else {
        tsc_syntax::skip_trivia(source.text(), error.pos as usize)
    };
    (start, error.end as usize)
}

fn case_clause_span(
    source: &SourceFile,
    node_end: usize,
    start: usize,
    statements: Option<NodeArrayId>,
) -> (usize, usize) {
    let end = statements
        .map(|statements| &source.arena.node_array(statements).nodes)
        .and_then(|nodes| nodes.first())
        .map(|&first| source.arena.node(first).pos as usize)
        .unwrap_or(node_end);
    (start, end)
}

fn arrow_function_span(source: &SourceFile, node: NodeId, body: Option<NodeId>) -> (usize, usize) {
    let record = source.arena.node(node);
    let start = tsc_syntax::skip_trivia(source.text(), record.pos as usize);
    if let Some(body) = body {
        let body = source.arena.node(body);
        if body.kind == SyntaxKind::Block {
            let starts = byte_line_starts(source.text());
            let start_line = line_of_bytes(&starts, body.pos as usize);
            let end_line = line_of_bytes(&starts, body.end as usize);
            if start_line < end_line {
                return (
                    start,
                    end_line_position(source.text(), &starts, start_line) + 1,
                );
            }
        }
    }
    (start, record.end as usize)
}

fn token_span(source: &SourceFile, start: usize) -> (usize, usize) {
    let Some(token) = tsc_syntax::scan_tokens(&source.text()[start..], source.language_variant)
        .into_iter()
        .next()
    else {
        return (start, start);
    };
    let positions = source.positions();
    let to_byte = |relative_utf16| {
        positions
            .byte_offset_from_utf16_delta(start as u32, relative_utf16)
            .unwrap_or(start as u32) as usize
    };
    (to_byte(token.start), to_byte(token.end))
}

fn byte_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut chars = text.char_indices().peekable();
    while let Some((byte, ch)) = chars.next() {
        match ch {
            '\r' => {
                let mut next = byte + 1;
                if let Some(&(next_byte, '\n')) = chars.peek() {
                    chars.next();
                    next = next_byte + 1;
                }
                starts.push(next);
            }
            '\n' => starts.push(byte + 1),
            '\u{2028}' | '\u{2029}' => starts.push(byte + ch.len_utf8()),
            _ => {}
        }
    }
    starts
}

fn line_of_bytes(starts: &[usize], position: usize) -> usize {
    match starts.binary_search(&position) {
        Ok(line) => line,
        Err(insert) => insert.saturating_sub(1),
    }
}

fn end_line_position(text: &str, starts: &[usize], line: usize) -> usize {
    if line + 1 == starts.len() {
        return text.len().saturating_sub(1);
    }
    let start = starts[line];
    let mut position = starts[line + 1].saturating_sub(1);
    while position >= start {
        if text.is_char_boundary(position) {
            match text[position..].chars().next() {
                Some('\n' | '\r' | '\u{2028}' | '\u{2029}') => {}
                _ => break,
            }
            if position == 0 {
                break;
            }
            position -= 1;
        } else {
            position -= 1;
        }
    }
    position
}

/// tsrs-native: declaration visitor comment-range lookup over parse and
/// synthesized nodes.
pub(crate) fn comment_range(
    arena: &TransformArena,
    node: TransformNode,
) -> Result<Option<CommentRange>, TransformError> {
    if let Some(range) = arena
        .metadata(node)
        .and_then(crate::EmitMetadata::comment_range)
    {
        return Ok(Some(range));
    }
    let record = arena.node(node)?;
    let source = arena.source(node.source())?.syntax();
    Ok(
        SourceRange::from_raw(record.pos, record.end, source.positions())
            .ok()
            .map(|range| CommentRange::new(node.source(), range)),
    )
}
