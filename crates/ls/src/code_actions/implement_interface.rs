//! tsgo `ls/codeactions_fixclassincorrectlyimplementsinterface.go`: the
//! members a class lacks of an interface it implements, added (index
//! signatures, then each missing member) with the imports their types
//! need — "Implement interface 'I'" and "Implement all unimplemented
//! interfaces".

use std::collections::BTreeSet;

use tsc_binder::node_util;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_diagnostics::gen;
use tsc_emitter::TransformNode;
use tsc_lsproto::TextEdit;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{ModifierFlags, SymbolId, TypeId};

use super::import_fixes::prepared_view;
use super::missing_member::{MissingMemberFixer, PRESERVE_OPTIONAL_ALL};
use super::{CodeAction, CodeFixContext, CodeFixProvider, CombinedCodeActions};
use crate::autoimport::{file_edits, ImportAdder};
use crate::change::Tracker;
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::new_line_kind;
use crate::request::Request;

const FIX_ID: &str = "fixClassIncorrectlyImplementsInterface";

/// tsgo `fixClassIncorrectlyImplementsInterfaceErrorCodes`.
fn error_codes() -> Vec<u32> {
    [
        &gen::Class_0_incorrectly_implements_interface_1,
        &gen::Class_0_incorrectly_implements_class_1_Did_you_mean_to_extend_1_and_inherit_its_members_as_a_subclass,
    ]
    .iter()
    .map(|message| message.code)
    .collect()
}

/// tsgo `FixClassIncorrectlyImplementsInterfaceProvider`.
pub(super) const PROVIDER: CodeFixProvider = CodeFixProvider {
    error_codes,
    get_code_actions,
    get_all_code_actions: Some(get_all_code_actions),
};

/// tsgo `getCodeActionsToFixClassIncorrectlyImplementsInterface`: one
/// action per implemented type with members to add.
fn get_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Vec<CodeAction>, LsError> {
    let format_settings = service.format_options().clone();
    let actions =
        service.with_request_at(context.file, context.span.0 as u32, false, |request, _| {
            let source = request.state.binder.source(context.file);
            let Some(class_declaration) = get_class(source, context.span.0) else {
                return Ok(Ok(Vec::new()));
            };
            let mut actions = Vec::new();
            for implemented_type_node in
                node_util::get_implements_heritage_clause_elements(source, class_declaration)
            {
                let changes = match add_and_get_changes(
                    request,
                    &format_settings,
                    context.file,
                    &[(class_declaration, vec![implemented_type_node])],
                )? {
                    Ok(changes) => changes,
                    Err(error) => return Ok(Err(error)),
                };
                if changes.is_empty() {
                    continue;
                }
                let text = node_text(source, implemented_type_node);
                actions.push(CodeAction {
                    description: tsc_diagnostics::format_message(
                        gen::Implement_interface_0.template_in(request.catalog),
                        &[text],
                    ),
                    changes,
                    fix_id: FIX_ID.to_owned(),
                });
            }
            Ok(Ok(actions))
        })?;
    actions.unwrap_or(Ok(Vec::new()))
}

/// tsgo `getAllCodeActionsToFixClassIncorrectlyImplementsInterface`: every
/// class of the file's diagnostics, each implemented type, at once.
fn get_all_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Option<CombinedCodeActions>, LsError> {
    let codes = error_codes();
    let diagnostics: Vec<usize> = service
        .all_diagnostics(context.file)?
        .into_iter()
        .filter(|diagnostic| super::is_fixable_diagnostic(diagnostic, &codes))
        .map(|diagnostic| diagnostic.start.unwrap_or_default() as usize)
        .collect();
    let format_settings = service.format_options().clone();
    let description = service.localize(&gen::Implement_all_unimplemented_interfaces, &[]);
    let changes = service.with_request_at(context.file, 0, false, |request, _| {
        let source = request.state.binder.source(context.file);
        let mut seen_class_declarations = BTreeSet::new();
        let mut classes = Vec::new();
        for &start in &diagnostics {
            let Some(class_declaration) = get_class(source, start) else {
                continue;
            };
            if seen_class_declarations.insert(class_declaration) {
                classes.push((
                    class_declaration,
                    node_util::get_implements_heritage_clause_elements(source, class_declaration),
                ));
            }
        }
        add_and_get_changes(request, &format_settings, context.file, &classes)
    })?;
    match changes {
        Some(Ok(changes)) if !changes.is_empty() => Ok(Some(CombinedCodeActions {
            description,
            changes,
        })),
        Some(Err(error)) => Err(error),
        _ => Ok(None),
    }
}

/// The edits of `addChanges` for each class and implemented type, with one
/// tracker and import adder (tsgo `getChanges`); an error when the
/// registry is not prepared for the file.
fn add_and_get_changes(
    request: &mut Request<'_, '_>,
    format_settings: &crate::lsutil::FormatCodeSettings,
    file: usize,
    classes: &[(NodeId, Vec<NodeId>)],
) -> CheckResult<Result<Vec<TextEdit>, LsError>> {
    let source = request.state.binder.source(file);
    let mut view = match prepared_view(request, file) {
        Ok(view) => view,
        Err(error) => return Ok(Err(error)),
    };
    let mut adder = view.as_ref().map(|_| {
        ImportAdder::new(
            source,
            request.state.options,
            format_settings.clone(),
            request.converters,
            request.preferences,
        )
    });
    let mut tracker = Tracker::new(
        new_line_kind(request.state.options),
        format_settings.clone(),
    );
    let target = tracker.source_in_program(source, request.state.program_source_id(file));
    let method_not_implemented = gen::Method_not_implemented
        .template_in(request.catalog)
        .to_owned();
    for (class_declaration, implemented_type_nodes) in classes {
        for &implemented_type_node in implemented_type_nodes {
            let mut fixer = MissingMemberFixer {
                tracker: &mut tracker,
                file: source,
                target,
                options: request.state.options,
                preferences: request.preferences,
                import_adder: match (&mut adder, &mut view) {
                    (Some(adder), Some(view)) => Some((adder, view)),
                    _ => None,
                },
                method_not_implemented: method_not_implemented.clone(),
            };
            if let Err(error) = add_changes(
                &mut fixer,
                request.state,
                *class_declaration,
                implemented_type_node,
            )? {
                return Ok(Err(LsError(error)));
            }
        }
    }
    let mut changes = match file_edits(tracker, source, request.converters) {
        Ok(changes) => changes,
        Err(error) => return Ok(Err(LsError(error))),
    };
    if let Some(adder) = adder.filter(ImportAdder::has_fixes) {
        match adder.edits() {
            Ok(edits) => changes.extend(edits),
            Err(error) => return Ok(Err(LsError(error.to_string()))),
        }
    }
    Ok(Ok(changes))
}

/// tsgo `addChanges`: the index signatures and the missing members of an
/// implemented type, after the class's constructor or at its start.
fn add_changes(
    fixer: &mut MissingMemberFixer<'_, '_, '_>,
    checker: &mut CheckerState<'_>,
    class_declaration: NodeId,
    implemented_type_node: NodeId,
) -> CheckResult<Result<(), String>> {
    let source = fixer.file;
    let constructor = get_constructor(source, class_declaration);
    let implemented_type = checker.get_type_at_location(implemented_type_node)?;
    let class_type = checker.get_type_at_location(class_declaration)?;
    let mut index_key_types = Vec::new();
    if checker.get_number_index_type(class_type)?.is_none() {
        index_key_types.push(checker.tables.intrinsics.number);
    }
    if checker.get_string_index_type(class_type)?.is_none() {
        index_key_types.push(checker.tables.intrinsics.string);
    }
    for key_type in index_key_types {
        let member = fixer.create_index_signature_declaration_from_type(
            checker,
            class_declaration,
            implemented_type,
            key_type,
        );
        let inserted = member.and_then(|member| {
            insert_interface_member_nodes(fixer, class_declaration, constructor, member)
        });
        if let Err(error) = inserted {
            return Ok(Err(error));
        }
    }
    let missing_members =
        get_missing_members(checker, source, class_declaration, &[implemented_type])?;
    for member in missing_members {
        let member_nodes = fixer.create_member_from_symbol(
            checker,
            member,
            class_declaration,
            None,
            PRESERVE_OPTIONAL_ALL,
            false,
        );
        let inserted = member_nodes.and_then(|member_nodes| {
            insert_interface_member_nodes(fixer, class_declaration, constructor, member_nodes)
        });
        if let Err(error) = inserted {
            return Ok(Err(error));
        }
    }
    Ok(Ok(()))
}

/// tsgo `insertInterfaceMemberNode` for each member: after the
/// constructor, else at the class's start.
fn insert_interface_member_nodes(
    fixer: &mut MissingMemberFixer<'_, '_, '_>,
    class_declaration: NodeId,
    constructor: Option<NodeId>,
    members: impl IntoIterator<Item = TransformNode>,
) -> Result<(), String> {
    let file = fixer.file;
    for member in members {
        match constructor {
            None => fixer
                .tracker
                .insert_member_at_start(file, class_declaration, member),
            Some(constructor) => fixer
                .tracker
                .insert_node_after(file, constructor, member)
                .map_err(|error| error.to_string())?,
        }
    }
    Ok(())
}

/// tsgo `getClass`: the class containing the token at the position (tsgo
/// `GetContainingClass`, the ancestors from the token's parent).
fn get_class(source: &SourceFile, position: usize) -> Option<NodeId> {
    let navigator = crate::astnav::Navigator::new(source);
    let token = navigator.token_at(position as u32);
    match token {
        crate::astnav::Found::Node(crate::tree::View::Node(node)) => {
            node_util::get_containing_class(source, node)
        }
        // A token is no node here: its parent is the first ancestor.
        found => match navigator.parent(found) {
            Some(crate::astnav::Found::Node(crate::tree::View::Node(parent))) => {
                if matches!(
                    source.arena.node(parent).kind,
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) {
                    Some(parent)
                } else {
                    node_util::get_containing_class(source, parent)
                }
            }
            _ => None,
        },
    }
}

/// tsgo `getConstructor`: the class's first constructor.
fn get_constructor(source: &SourceFile, class_declaration: NodeId) -> Option<NodeId> {
    let members = match &source.arena.node(class_declaration).data {
        NodeData::ClassDeclaration(data) => data.members,
        NodeData::ClassExpression(data) => data.members,
        _ => None,
    }?;
    source
        .arena
        .node_array(members)
        .nodes
        .iter()
        .copied()
        .find(|&member| source.arena.node(member).kind == SyntaxKind::Constructor)
}

/// tsgo `getMissingMembers`: the implemented types' non-private properties
/// the class neither declares nor inherits, each name once.
fn get_missing_members(
    checker: &mut CheckerState<'_>,
    source: &SourceFile,
    class_declaration: NodeId,
    implemented_types: &[TypeId],
) -> CheckResult<Vec<SymbolId>> {
    let inherited_members = get_inherited_members(checker, source, class_declaration)?;
    let class_members = checker
        .binder
        .node_symbol(class_declaration)
        .map(|symbol| checker.binder.symbol(symbol).members().clone());
    let mut seen_members = BTreeSet::new();
    let mut missing_members = Vec::new();
    for &implemented_type in implemented_types {
        for symbol in checker.get_properties_of_type(implemented_type)? {
            let name = checker.binder.symbol(symbol).escaped_name;
            if class_members
                .as_ref()
                .is_some_and(|members| members.get(name).is_some())
            {
                continue;
            }
            if inherited_members.contains(&name) || seen_members.contains(&name) {
                continue;
            }
            let flags = checker.get_declaration_modifier_flags_from_symbol(symbol);
            if !flags.intersects(ModifierFlags::PRIVATE) {
                seen_members.insert(name);
                missing_members.push(symbol);
            }
        }
    }
    Ok(missing_members)
}

/// tsgo `getInheritedMembers`: the names of the base class's non-private
/// properties.
fn get_inherited_members(
    checker: &mut CheckerState<'_>,
    source: &SourceFile,
    class_declaration: NodeId,
) -> CheckResult<BTreeSet<tsc_types::EscapedName>> {
    let Some(type_node) = node_util::get_class_extends_heritage_element(source, class_declaration)
    else {
        return Ok(BTreeSet::new());
    };
    let base_type = checker.get_type_at_location(type_node)?;
    let mut inherited_members = BTreeSet::new();
    for symbol in checker.get_properties_of_type(base_type)? {
        let flags = checker.get_declaration_modifier_flags_from_symbol(symbol);
        if !flags.intersects(ModifierFlags::PRIVATE) {
            inherited_members.insert(checker.binder.symbol(symbol).escaped_name);
        }
    }
    Ok(inherited_members)
}

/// tsgo `scanner.GetTextOfNode`.
fn node_text(source: &SourceFile, node: NodeId) -> String {
    let navigator = crate::astnav::Navigator::new(source);
    let start = navigator.start_of_node(
        crate::astnav::Found::Node(crate::tree::View::Node(node)),
        false,
    ) as usize;
    let end = source.arena.node(node).end as usize;
    source.text()[start..end].to_owned()
}
