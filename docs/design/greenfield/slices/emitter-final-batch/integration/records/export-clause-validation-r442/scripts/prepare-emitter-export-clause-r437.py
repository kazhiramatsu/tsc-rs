from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-export-clause-r437');O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest();rows=[]
p='crates/emitter/src/printer.rs';old=(R/p).read_bytes();s=old.decode();start=s.index('            NodeData::ExportDeclaration(data) => {');end=s.index('            NodeData::ImportAttributes(data) => {',start);part=s[start:end]
a='''                    let prefix = self.token_owned_child_prefix(
                        transformation,
                        export_item_anchor,
                        Some(clause),
                    )?;'''
b='''                    // emit(exportClause) starts its own comment phase. A
                    // recovered export keyword can end before that owner.
                    let clause_owner =
                        self.expression_comment_phase_owner_for_node(transformation, clause)?;
                    let prefix = self.token_owned_comment_phase_prefix(
                        transformation,
                        export_item_anchor,
                        clause_owner,
                    )?;''';assert part.count(a)==1;part=part.replace(a,b);s=s[:start]+part+s[end:];dst=O/p;dst.parent.mkdir(parents=True);dst.write_text(s);subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(dst)],check=True);rows.append({'path':p,'before':h(old),'after':h(dst.read_bytes())});q=subprocess.run(['diff','-u','--label','a/'+p,'--label','b/'+p,str(R/p),str(dst)],capture_output=True);assert q.returncode==1;(O/'proposal.patch').write_bytes(q.stdout);(O/'manifest.json').write_text(json.dumps({'head':'8859338f1c761d867cbc7f5ef58987dc3777f944','files':rows,'qualified':False,'scope':'ExportDeclaration child comment phase only; reuse existing owner-aware prefix projection; strict CommentResume invariants untouched.'},indent=2)+'\n');print(q.stdout.decode())
