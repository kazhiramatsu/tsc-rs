//! M7 8.3/8.4 unused-identifier producers.
//!
//! Workers land by declaration owner. The semantic error surface is
//! activated first under `noUnusedLocals` / `noUnusedParameters`; the
//! same registrations feed the suggestion surface in 8.4.

use tsrs2_binder::node_util;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult2, CheckerState, Unsupported};

impl<'a> CheckerState<'a> {
    /// tsc-port: registerForUnusedIdentifiersCheck @6.0.3
    /// tsc-hash: bd4d966695b8aae018cbaea7cf4462c968f8d9672dc8812f6a7b06cbf76fa16f
    /// tsc-span: _tsc.js:82942-82953
    /// d2: d2:08b79e6517d01e5d88bb72d904893471db59fd488401a64241687e5df4e9affe
    ///
    /// The Rust checker stores only the current file's entry and
    /// drains after deferred nodes; this is the eager equivalent of
    /// tsc's addLazyDiagnostic + source-file map.
    pub(crate) fn register_for_unused_identifiers_check(&mut self, node: NodeId) {
        self.potentially_unused_identifiers.push(node);
    }

    /// tsrs-native: incremental error-mode projection of tsc
    /// checkUnusedIdentifiers. Only registered producers can reach
    /// this match; later 8.3 slices add their registrations and arms.
    pub(crate) fn check_unused_identifiers_error_mode(&mut self) {
        let nodes = std::mem::take(&mut self.potentially_unused_identifiers);
        for node in nodes {
            if self.contains_parse_error_for_unused(node)
                || self.is_ambient_for_unused(node)
                || self.options.no_unused_locals != Some(true)
            {
                continue;
            }
            let result = match self.kind_of(node) {
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                    self.check_unused_class_members(node)
                }
                _ => Ok(()),
            };
            if let Err(unsupported) = result {
                self.mark_partially_checked_node(node, unsupported.reason);
            }
        }
    }

    fn contains_parse_error_for_unused(&self, node: NodeId) -> bool {
        NodeFlags::from_bits(self.node_flags(node))
            .intersects(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
    }

    fn is_ambient_for_unused(&self, node: NodeId) -> bool {
        self.binder.flags_of(node).intersects(NodeFlags::AMBIENT)
            || NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
            || node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::AMBIENT,
            )
    }

    /// tsc-port: checkUnusedClassMembers @6.0.3
    /// tsc-hash: b5c9ae6d244cc4bb01e39b9b4fd715a5417bb06e780f0a33cbb49b96ff1f65af
    /// tsc-span: _tsc.js:83008-83038
    /// d2: d2:5a2c45fdca4506945d356d1d7cf0abdfbf8b3db6c524587eb3031fd4e0169d16
    fn check_unused_class_members(&mut self, node: NodeId) -> CheckResult2<()> {
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => return Ok(()),
        };
        for member in self.nodes_of(members) {
            match self.kind_of(member) {
                SyntaxKind::MethodDeclaration
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor => {
                    let symbol = self.get_symbol_of_declaration(member)?;
                    if self.kind_of(member) == SyntaxKind::SetAccessor
                        && self
                            .binder
                            .symbol(symbol)
                            .flags
                            .intersects(SymbolFlags::GET_ACCESSOR)
                    {
                        continue;
                    }
                    let Some(name) = self.name_of_node(member) else {
                        continue;
                    };
                    let private = node_util::get_combined_modifier_flags(
                        self.binder.source_of_node(member),
                        member,
                    )
                    .intersects(ModifierFlags::PRIVATE)
                        || self.kind_of(name) == SyntaxKind::PrivateIdentifier;
                    if !self.links.symbol(symbol).is_referenced
                        && private
                        && !self.is_ambient_for_unused(member)
                    {
                        let display = self.declaration_name_display(name);
                        self.error_at(
                            Some(name),
                            &diagnostics::_0_is_declared_but_its_value_is_never_read,
                            &[&display],
                        );
                    }
                }
                SyntaxKind::Constructor => {
                    let parameters = match self.data_of(member) {
                        NodeData::Constructor(data) => data.parameters,
                        _ => None,
                    };
                    for parameter in self.nodes_of(parameters) {
                        let symbol = self.get_symbol_of_declaration(parameter)?;
                        if self.links.symbol(symbol).is_referenced
                            || !node_util::has_syntactic_modifier(
                                self.binder.source_of_node(parameter),
                                parameter,
                                ModifierFlags::PRIVATE,
                            )
                        {
                            continue;
                        }
                        let Some(name) = self.name_of_node(parameter) else {
                            continue;
                        };
                        let display = self.declaration_name_display(name);
                        self.error_at(
                            Some(name),
                            &diagnostics::Property_0_is_declared_but_its_value_is_never_read,
                            &[&display],
                        );
                    }
                }
                SyntaxKind::IndexSignature
                | SyntaxKind::SemicolonClassElement
                | SyntaxKind::ClassStaticBlockDeclaration => {}
                _ => {
                    return Err(Unsupported::new(
                        "checkUnusedClassMembers unexpected class member (Debug.fail transcription, parse recovery)",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{check_program, CompilerOptions, InputFile};
    use tsrs2_diags::DiagnosticCategory;

    fn unused_rows(
        text: &str,
        options: &CompilerOptions,
    ) -> Vec<(u32, DiagnosticCategory, u32, u32, String)> {
        check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: text.to_owned(),
            }],
            options,
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 6133 | 6138))
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text().to_owned(),
            )
        })
        .collect()
    }

    const CLASS_PROBE: &str = "class C {
  #used = 0;
  #unused = 0;
  private oldUsed = 0;
  private oldUnused = 0;
  get #pair() { return 0; }
  set #pair(value: number) {}
  get #dead() { return 0; }
  set #dead(value: number) {}
  constructor(private live: number, private dead: number) {
    this.#used;
    this.oldUsed;
    this.#pair;
    this.live;
  }
}
";

    #[test]
    fn unused_private_class_members_follow_reference_and_accessor_anchors() {
        let rows = unused_rows(
            CLASS_PROBE,
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows.iter()
                .map(|(code, category, _, _, message)| (*code, *category, message.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'#unused' is declared but its value is never read."
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'oldUnused' is declared but its value is never read."
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'#dead' is declared but its value is never read."
                ),
                (
                    6138,
                    DiagnosticCategory::Error,
                    "Property 'dead' is declared but its value is never read."
                ),
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|(_, _, start, length, _)| (*start, *length))
                .collect::<Vec<_>>(),
            [(25, 7), (71, 9), (150, 5), (246, 4)]
        );
    }

    #[test]
    fn unused_class_member_errors_require_no_unused_locals() {
        assert!(unused_rows(CLASS_PROBE, &CompilerOptions::default()).is_empty());
        assert!(unused_rows(
            CLASS_PROBE,
            &CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn private_brand_in_expression_counts_as_a_read() {
        let rows = unused_rows(
            "class C { #unused: undefined; #brand: undefined; has(v: any) { return #brand in v; } }\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 6133);
        assert_eq!(
            rows[0].4,
            "'#unused' is declared but its value is never read."
        );
    }
}
