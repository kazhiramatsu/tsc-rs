#!/usr/bin/env python3
"""EF5-ESCAPED: an identifier cloned from a parsed identifier keeps that identifier's source spelling
(tsc cloneNode + setTextRange -> getTextOfNode reads the source text)."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:80])
        s = s.replace(old, new)
    open(path, "w").write(s)

patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/metadata.rs", [
("""    pub(crate) unchecked_identifier_text: Option<tsc_diagnostics::JsString>,
    pub(crate) helpers: Vec<Box<str>>,
""", """    pub(crate) unchecked_identifier_text: Option<tsc_diagnostics::JsString>,
    /// The node is `cloneNode(parsedIdentifier)` (+ `setTextRange`): tsc's
    /// `getTextOfNode` prints such a clone from the source text, so an
    /// escaped spelling (`\\u0046oo`) survives the clone even though this
    /// arena keeps the clone's own positions synthetic.
    pub(crate) cloned_identifier_spelling: bool,
    pub(crate) helpers: Vec<Box<str>>,
"""),
("""        if source.unchecked_identifier_text.is_some() {
            self.unchecked_identifier_text = source.unchecked_identifier_text.clone();
        }
""", """        if source.unchecked_identifier_text.is_some() {
            self.unchecked_identifier_text = source.unchecked_identifier_text.clone();
        }
        self.cloned_identifier_spelling |= source.cloned_identifier_spelling;
"""),
])
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/factory.rs", [
("""        self.arena.set_transform_flags(clone, transform_flags);
        self.arena.set_original_node(clone, Some(original))?;
        self.arena.copy_literal_properties(original, clone);
        Ok(clone)
    }
""", """        self.arena.set_transform_flags(clone, transform_flags);
        self.arena.set_original_node(clone, Some(original))?;
        self.arena.copy_literal_properties(original, clone);
        if matches!(
            record.data,
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_)
        ) {
            self.arena.metadata_mut(clone).cloned_identifier_spelling = true;
        }
        Ok(clone)
    }
"""),
])
patch("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/printer.rs", [
("""                if self.transformed_identifier_can_reuse_source_spelling(
                    transformation,
                    node,
                    &data.text,
                )? {
                    self.write_original_without_leading_trivia(transformation, node, writer)
                } else {
                    writer.write_symbol(&data.text);
                    Ok(())
                }
            }
            NodeData::PrivateIdentifier(data) if changed => {
                if self.transformed_identifier_can_reuse_source_spelling(
                    transformation,
                    node,
                    &data.text,
                )? {
                    self.write_original_without_leading_trivia(transformation, node, writer)
                } else {
                    writer.write_symbol(&data.text);
                    Ok(())
                }
            }
""", """                if let Some(spelling) = self.transformed_identifier_source_spelling_node(
                    transformation,
                    node,
                    &data.text,
                )? {
                    self.write_original_without_leading_trivia(transformation, spelling, writer)
                } else {
                    writer.write_symbol(&data.text);
                    Ok(())
                }
            }
            NodeData::PrivateIdentifier(data) if changed => {
                if let Some(spelling) = self.transformed_identifier_source_spelling_node(
                    transformation,
                    node,
                    &data.text,
                )? {
                    self.write_original_without_leading_trivia(transformation, spelling, writer)
                } else {
                    writer.write_symbol(&data.text);
                    Ok(())
                }
            }
"""),
("""    fn transformed_identifier_can_reuse_source_spelling(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        text: &str,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        if original == node || original.source() != node.source() {
            return Ok(false);
        }
        let record = transformation.arena().node(node)?;
        let original_record = transformation.arena().node(original)?;
        let original_text = match &original_record.data {
            NodeData::Identifier(identifier) => &identifier.text,
            NodeData::PrivateIdentifier(identifier) => &identifier.text,
            _ => return Ok(false),
        };
        if original_text != text
            || record.pos != original_record.pos
            || record.end != original_record.end
        {
            return Ok(false);
        }
        let source = transformation.arena().source(node.source())?.syntax();
        Ok(matches!(
            SourceRange::from_raw(record.pos, record.end, source.positions())?,
            SourceRange::Original(_)
        ))
    }
""", """    fn transformed_identifier_source_spelling_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        text: &str,
    ) -> Result<Option<TransformNode>, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        if original == node || original.source() != node.source() {
            return Ok(None);
        }
        let record = transformation.arena().node(node)?;
        let original_record = transformation.arena().node(original)?;
        let original_text = match &original_record.data {
            NodeData::Identifier(identifier) => &identifier.text,
            NodeData::PrivateIdentifier(identifier) => &identifier.text,
            _ => return Ok(None),
        };
        if original_text != text {
            return Ok(None);
        }
        let source = transformation.arena().source(node.source())?.syntax();
        let original_is_parsed = matches!(
            SourceRange::from_raw(
                original_record.pos,
                original_record.end,
                source.positions()
            )?,
            SourceRange::Original(_)
        );
        if !original_is_parsed {
            return Ok(None);
        }
        // Positions threaded through (`setTextRange` on the clone) or a
        // clone that keeps its positions synthetic but records that its
        // spelling is the parsed identifier's: tsc prints both from the
        // source text (getTextOfNode, canUseSourceFile).
        if record.pos == original_record.pos && record.end == original_record.end {
            return Ok(Some(node));
        }
        let cloned = transformation
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.cloned_identifier_spelling);
        Ok(cloned.then_some(original))
    }
"""),
])
print("EF5-ESCAPED patch applied")
