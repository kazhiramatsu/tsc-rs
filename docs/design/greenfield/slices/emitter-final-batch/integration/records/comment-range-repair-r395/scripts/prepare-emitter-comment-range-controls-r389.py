from pathlib import Path
import subprocess,json,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-comment-range-controls-r389');O.mkdir(exist_ok=False)
paths={}
def put(path,s):
 p=O/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(s);paths[path]={'before':hashlib.sha256((R/path).read_bytes()).hexdigest(),'after':hashlib.sha256(p.read_bytes()).hexdigest()}
f='scripts/observe-list-comment-flags.mjs';s=(R/f).read_text();s=s.replace('function observe(source, variant, removeComments) {','function observe(source, variant, removeComments, bodyRange) {').replace('  const statement = file.statements[0];','  let statement = file.statements[0];')
needle='''  if (ts.isFunctionDeclaration(statement)) {
''';extra='''    if (bodyRange) {
      const original = statement.body.statements;
      const statements = ts.factory.createNodeArray([...original]);
      ts.setTextRange(statements, {
        pos: bodyRange === "EndOnly" || bodyRange === "Synthesized" ? -1 : original.pos,
        end: bodyRange === "StartOnly" || bodyRange === "Synthesized" ? -1 : original.end,
      });
      const body = ts.factory.updateBlock(statement.body, statements);
      statement = ts.factory.updateFunctionDeclaration(statement, statement.modifiers,
        statement.asteriskToken, statement.name, statement.typeParameters,
        statement.parameters, statement.type, body);
      file = ts.factory.updateSourceFile(file, [statement, ...file.statements.slice(1)]);
    }
''';assert s.count(needle)==1;s=s.replace(needle,needle+extra)
needle='const artifact = {version:1,';extra='''// Factory-only endpoint controls: EndOnly mirrors dotted namespace arrays;
// StartOnly and Synthesized exercise the public factory/printer boundary without
// claiming that ordinary command transforms currently produce StartOnly bodies.
for (const [shape, source] of bodies) for (const body_range of ["StartOnly", "EndOnly", "Synthesized"])
  for (const variant of variants) for (const remove_comments of [false, true]) {
    const output = observe(source, variant, remove_comments, body_range);
    assert.equal(observe(source, variant, remove_comments, body_range), output);
    cases.push({case_id:`${shape}/${body_range}/${variant}/${remove_comments ? "removed" : "retained"}`,
      source, variant, body_range, remove_comments, output});
  }
assert.equal(cases.length, 244);
''';assert s.count(needle)==1;s=s.replace(needle,extra+needle);put(f,s)
f='crates/emitter/tests/list_comment_flags_contract.rs';s=(R/f).read_text();s=s.replace('    clone_name: bool,','    clone_name: bool,\n    body_range: Option<(bool, bool)>,',1)
start=s.index('        if let NodeData::FunctionDeclaration(data) = &arena.node(statement)?.data {');end=s.index('        let parent = match &arena.node(statement)?.data {',start)
s=s[:start]+'''        if let NodeData::FunctionDeclaration(mut function) = arena.node(statement)?.data.clone() {
            let mut statement = statement;
            let mut body = arena.node_ref(source, function.body.unwrap()).unwrap();
            if let Some((keep_start, keep_end)) = self.body_range {
                let NodeData::Block(mut block) = arena.node(body)?.data.clone() else {
                    unreachable!()
                };
                let original = arena.node_array_ref(source, block.statements.unwrap()).unwrap();
                let original = arena.node_array(original)?;
                let (pos, end) = (original.pos, original.end);
                let children = original.nodes.iter().map(|&n| arena.node_ref(source, n).unwrap()).collect();
                let mut root_children: Vec<_> = arena.node_array(statements)?.nodes.iter()
                    .map(|&n| arena.node_ref(source, n).unwrap()).collect();
                let mut file = file.clone();
                let body_flags = arena.transform_flags(body);
                let function_flags = arena.transform_flags(statement);
                let file_flags = arena.transform_flags(root_node);
                let array = context.factory()?.create_node_array(source, children)?;
                context.factory()?.set_node_array_text_range(array,
                    if keep_start { pos } else { u32::MAX },
                    if keep_end { end } else { u32::MAX })?;
                block.statements = Some(array.array());
                body = context.factory()?.update_node(body, NodeData::Block(block), body_flags)?;
                function.body = Some(body.node());
                statement = context.factory()?.update_node(statement,
                    NodeData::FunctionDeclaration(function), function_flags)?;
                root_children[0] = statement;
                let array = context.factory()?.update_node_array(statements, root_children)?;
                file.statements = Some(array.array());
                let updated = context.factory()?.update_node(root_node,
                    NodeData::SourceFile(file), file_flags)?;
                context.arena_mut()?.replace_root(source, updated)?;
            }
            if self.parent || self.parent_and_body {
                context.arena_mut()?.metadata_mut(statement).add_flags(self.flags);
            }
            if !self.parent {
                context.arena_mut()?.metadata_mut(body).add_flags(self.flags);
            }
            return Ok(root);
        }
'''+s[end:]
s=s.replace('assert_eq!(cases.len(), 172);','assert_eq!(cases.len(), 244);')
needle='                    clone_name: case["variant"] == "CloneName",';assert s.count(needle)==1;s=s.replace(needle,needle+'''
                    body_range: case["body_range"].as_str().map(|range| match range {
                        "StartOnly" => (true, false),
                        "EndOnly" => (false, true),
                        "Synthesized" => (false, false),
                        _ => unreachable!(),
                    }),''');put(f,s)
f='scripts/observe-emitter-context-recovery.mjs';s=(R/f).read_text();needle='function diagnostic(d) {'
extra='''// Function-body comment endpoint regressions from full emitter integration.
const bodyRangeControls = [
  ["dotted-export", "namespace Shape.Utils { export function convert() { return null; } }\\nnamespace Explicit { export namespace Nested { export function convert() { return null; } } }\\n"],
  ["dotted-tail", "namespace hello.hi.world\\r\\n{\\r\\n    function foo() {}\\r\\n\\r\\n    // 😀 inner tail\\r\\n}\\r\\n"],
  ["dotted-empty-runtime", "namespace Empty.Outer.Inner {\\n    export declare const value: number;\\n    // inner empty tail\\n}\\n"],
  ["system-default-asi", "const Home = {};\\nexport default Home\\n// trailing export comment\\n"],
  ["system-default-semicolon", "const Home = {};\\nexport default Home;\\n// trailing export comment\\n"],
];
for (const target of ["es5", "es2015"])
  for (const alwaysStrict of [false, true]) for (const removeComments of [false, true])
    for (const [shape, text] of bodyRangeControls) {
      const main = "/project/main.ts";
      inputs.push({case_id:`emitter-context-recovery/body-range/${target}/strict-${alwaysStrict}/remove-${removeComments}/${shape}`,
        roots:[main], files:[{path:main, text}], options:{},
        config:JSON.stringify({compilerOptions:{target, module:"system", alwaysStrict, removeComments,
          strict:false, skipDefaultLibCheck:true, noErrorTruncation:true,
          sourceMap:true, declaration:true, declarationMap:true,
          ignoreDeprecations:"6.0", outDir:"/project/out"}, files:["main.ts"]})});
    }
assert.equal(inputs.length, 860);

''';assert s.count(needle)==1;s=s.replace(needle,extra+needle).replace('assert.equal(cases.length, 748);','assert.equal(cases.length, 788);');put(f,s)
subprocess.run(['rustfmt','--edition','2021',str(O/'crates/emitter/tests/list_comment_flags_contract.rs')],check=True)
for f in paths:paths[f]['after']=hashlib.sha256((O/f).read_bytes()).hexdigest()
(O/'manifest.json').write_text(json.dumps({'scope':'Unapplied controls; TypeScript observations and native comparisons pending. Original172+72 new factory flags; original748+40 ordinary commands, original72 refusals unchanged.','files':paths},indent=2)+'\n');print(O)
