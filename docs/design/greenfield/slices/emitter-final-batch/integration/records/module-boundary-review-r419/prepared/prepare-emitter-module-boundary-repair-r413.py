from pathlib import Path
import hashlib,json,difflib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-module-boundary-repair-r413');O.mkdir(exist_ok=False)
h=lambda b:hashlib.sha256(b).hexdigest();files=[];patch=[]
for rel in ['crates/emitter/src/printer.rs','crates/emitter/src/builtins.rs']:
 before=(R/rel).read_text();after=before
 if rel.endswith('printer.rs'):
  old='''                let import_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ImportKeyword),
                    import_anchor,
                    false,
                    writer,
                )?;'''
  new='''                // The modifier node already owns its trailing comments.
                // The keyword visits only getLeadingCommentRanges at that end.
                let import_keyword = self.emit_source_leading_token_with_context(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ImportKeyword),
                    import_anchor,
                    TokenLeadingSpace::None,
                    expression_context,
                    writer,
                )?;'''
  assert after.count(old)==1;after=after.replace(old,new)
  old='''                let export_anchor =
                    self.token_after_modifiers_cursor(transformation, node, data.modifiers)?;'''
  assert after.count(old)==2;after=after.replace(old,'''                // Unlike import declarations, tsc starts this token at node.pos.
                let export_anchor = self.node_start_cursor(transformation, node)?;''')
  start=after.index('            NodeData::ExportAssignment(data) => {');end=after.index('                let export_keyword = ',start)
  old=after[start:end]
  assert old.count('self.emit_modifiers(')==1
  new='''            NodeData::ExportAssignment(data) => {
                // emitExportAssignment never prints its recovery modifiers,
                // including when the declaration transform retains them.
                let export_anchor = self.node_start_cursor(transformation, node)?;
'''
  after=after[:start]+new+after[end:]
 else:
  old='''        let Some(clause_id) = data.import_clause else {
            return Ok(Some(
                self.update_generic(original, NodeData::ImportDeclaration(data))?,
            ));
        };'''
  new='''        let Some(clause_id) = data.import_clause else {
            // tsc returns a side-effect import unchanged, including recovery
            // modifiers. A generic child visit would erase type modifiers.
            return Ok(Some(original.node()));
        };'''
  assert after.count(old)==1;after=after.replace(old,new)
  old='''        let Some(clause) = data.export_clause else {
            return Ok(Some(
                self.update_generic(original, NodeData::ExportDeclaration(data))?,
            ));
        };
        let clause_node = self.node(clause);'''
  new='''        let Some(clause) = data.export_clause else {
            // updateExportDeclaration receives every current child unchanged
            // for a star export, so its recovery modifiers remain intact.
            return Ok(Some(original.node()));
        };
        let clause_node = self.node(clause);
        if self.context.arena().node(clause_node)?.kind == SyntaxKind::NamespaceExport {
            return Ok(Some(original.node()));
        }'''
  assert after.count(old)==1;after=after.replace(old,new)
 dest=O/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_text(after);files.append({'path':rel,'before':h(before.encode()),'after':h(after.encode())});patch+=list(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='a/'+rel,tofile='b/'+rel))
(O/'proposal.patch').write_text(''.join(patch));(O/'manifest.json').write_text(json.dumps({'scope':'UNAPPLIED proposed module declaration producer/printer repair; await actual Opus192 and canonical396 completion. No qualification claimed.','head':'8859338f1c761d867cbc7f5ef58987dc3777f944','files':files},indent=2)+'\n');print(O)
