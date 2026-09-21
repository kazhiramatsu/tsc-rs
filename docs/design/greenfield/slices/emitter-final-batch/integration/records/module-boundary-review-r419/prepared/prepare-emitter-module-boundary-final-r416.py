from pathlib import Path
import hashlib,json,difflib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');A=Path('/tmp/emitter-module-boundary-repair-r413');O=Path('/tmp/emitter-module-boundary-final-r416');O.mkdir(exist_ok=False)
h=lambda b:hashlib.sha256(b).hexdigest();m=json.loads((A/'manifest.json').read_bytes());files=[];patch=[]
for f in m['files']:
 rel=f['path'];before=(R/rel).read_text();after=(A/rel).read_text();assert h(before.encode())==f['before']
 if rel.endswith('builtins.rs'):
  old='''        let Some(clause) = data.export_clause else {
            // updateExportDeclaration receives every current child unchanged
            // for a star export, so its recovery modifiers remain intact.
            return Ok(Some(original.node()));
        };
        let clause_node = self.node(clause);
        if self.context.arena().node(clause_node)?.kind == SyntaxKind::NamespaceExport {
            return Ok(Some(original.node()));
        }'''
  new='''        let clause_node = match data.export_clause.map(|clause| self.node(clause)) {
            Some(clause) if self.context.arena().node(clause)?.kind != SyntaxKind::NamespaceExport => {
                clause
            }
            export_clause => {
                // tsc's typed update preserves all current fields for star
                // and namespace exports, including recovery modifiers.
                let modifiers = data.modifiers.map(|modifiers| self.array(modifiers));
                let module_specifier = data.module_specifier.map(|module| self.node(module));
                let attributes = data.attributes.map(|attributes| self.node(attributes));
                return Ok(Some(
                    self.context
                        .factory()?
                        .update_export_declaration(
                            original,
                            modifiers,
                            data.is_type_only,
                            export_clause,
                            module_specifier,
                            attributes,
                        )?
                        .node(),
                ));
            }
        };'''
  assert after.count(old)==1;after=after.replace(old,new)
 dest=O/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_text(after);files.append({'path':rel,'before':h(before.encode()),'after':h(after.encode())});patch+=list(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='a/'+rel,tofile='b/'+rel))
(O/'proposal.patch').write_text(''.join(patch));(O/'manifest.json').write_text(json.dumps({'scope':'Unapplied reviewed source proposal. Opus192 withdrew modifier API change after measured collector correction. Use typed ExportDeclaration factory update for an explicit producer boundary, preserving current fields; current original parameter already denotes the node being visited, not the parse-tree original. No claim that direct return would discard previous-pass children.','head':m['head'],'files':files,'review':'/tmp/emitter-import-map-review-r412/response.json','native_qualified':False},indent=2)+'\n');print(O)
