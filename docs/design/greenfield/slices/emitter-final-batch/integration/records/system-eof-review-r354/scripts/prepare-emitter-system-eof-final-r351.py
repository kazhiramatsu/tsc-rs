from pathlib import Path
import json,hashlib,difflib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');O=Path('/tmp/emitter-system-eof-final-r351');O.mkdir(exist_ok=False);files={}
p='crates/emitter/src/printer.rs';s=Path('/tmp/emitter-system-eof-review-r345/printer.rs').read_text()
a='''        raw_anchor: u32,
        token_start: usize,''';b='''        raw_anchor: SourceBytePosition,
        token_start: usize,''';assert s.count(a)==1;s=s.replace(a,b)
a='''        SourceBytePosition::new(raw_anchor, positions).ok()?;
        // Source slice ends use bytes; map columns use UTF-16 units.''';b='''        // Source slice ends use bytes; map columns use UTF-16 units.''';assert s.count(a)==1;s=s.replace(a,b)
a='SourceRange::from_raw(raw_anchor, end, positions)';assert s.count(a)==1;s=s.replace(a,'SourceRange::from_raw(raw_anchor.value(), end, positions)')
a='''                cursor_source,
                start_position.value(),
                token_start,''';assert s.count(a)==1;s=s.replace(a,'''                cursor_source,
                start_position,
                token_start,''')
a='''        let syntax = transformation.arena().source(source)?.syntax();
        let token_start = skip_trivia(syntax.text(), raw_anchor as usize);
        Ok(TokenMapDefault::from_token('''
b='''        let syntax = transformation.arena().source(source)?.syntax();
        // Trivia scanning indexes real source text. Synthetic and invalid
        // anchors have no default map, and must never reach that scanner.
        let Ok(raw_anchor) = SourceBytePosition::new(raw_anchor, syntax.positions()) else {
            return Ok(None);
        };
        let token_start = skip_trivia(syntax.text(), raw_anchor.value() as usize);
        Ok(TokenMapDefault::from_token(''';assert s.count(a)==1;s=s.replace(a,b);files[p]=s
p='crates/emitter/tests/unit/builtins/tests.rs';s=Path('/tmp/emitter-synthetic-meta-tests-r348/tests.rs').read_text()
a='''                    "synthetic" => {''';assert s.count(a)==1;s=s.replace(a,'''                    "synthetic-owner" => {
                        return self.arena.factory().create_node(
                            self.source,
                            NodeData::MetaProperty(meta.clone()),
                            crate::TransformFlags::NONE,
                        ).map(|node| Some(node.node()));
                    }
                    "synthetic" => {''')
start=s.index('fn meta_property_token_maps_internal_invariants_match_typescript()');end=s.index('\n#[test]',start);chunk=s[start:end];assert chunk.count('assert_eq!(rows.len(), 10);')==1;chunk=chunk.replace('assert_eq!(rows.len(), 10);','assert_eq!(rows.len(), 12);')
a='''                    "{id}: complete internal map JSON"
                );''';b=a+'''
                let unrecorded = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_target(ScriptTarget::ES2015),
                ).print(&mut transformed, PrintRequest::SourceFile(source), None).unwrap();
                assert_eq!(unrecorded.text(), expected_text, "{id}: unrecorded printer text");
                assert!(unrecorded.source_map().is_none());''';assert chunk.count(a)==1;chunk=chunk.replace(a,b);s=s[:start]+chunk+s[end:];files[p]=s
p='crates/emitter/tests/unit/source_map/tests.rs';s=(R/p).read_text()
a='''            [(0, line, column), (0, line, column + 1)],
            "{text:?}"
        );''';b=a+'''
        assert_eq!(
            segments.iter().map(|segment| (segment.generated_line, segment.generated_character)).collect::<Vec<_>>(),
            [(1, 0), (1, 1)],
            "{text:?}: close brace generated positions",
        );''';assert s.count(a)==1;s=s.replace(a,b);files[p]=s
manifest={'status':'DRAFT ONLY; approved actual182 guard +181 spellingcleanup +negativefactory12controls; apply after342 restores tests.','paths':[]}
for p,s in files.items():
 out=O/p;out.parent.mkdir(parents=True,exist_ok=True);out.write_text(s)
 subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(out)],check=True)
 before=(R/p).read_bytes();after=out.read_bytes();manifest['paths'].append({'path':p,'original_sha256':hashlib.sha256(before).hexdigest(),'draft_sha256':hashlib.sha256(after).hexdigest()})
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps(manifest))
