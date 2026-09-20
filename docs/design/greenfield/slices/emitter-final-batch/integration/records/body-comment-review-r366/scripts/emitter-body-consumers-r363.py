from pathlib import Path
import json,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-body-consumers-r363');O.mkdir(exist_ok=False)
p=R/'crates/emitter/tests/list_comment_flags_contract.rs';s=p.read_text();(O/p.name).write_text(s)
s=s.replace('    parent: bool,','    parent: bool,\n    parent_and_body: bool,')
a='''        let parent = match &arena.node(statement)?.data {'''
b='''        if let NodeData::FunctionDeclaration(data) = &arena.node(statement)?.data {
            let body = arena.node_ref(source, data.body.unwrap()).unwrap();
            if self.parent || self.parent_and_body {
                context.arena_mut()?.metadata_mut(statement).add_flags(self.flags);
            }
            if !self.parent {
                context.arena_mut()?.metadata_mut(body).add_flags(self.flags);
            }
            return Ok(root);
        }
'''+a
assert s.count(a)==1;s=s.replace(a,b).replace('assert_eq!(cases.len(), 147);','assert_eq!(cases.len(), 172);').replace('"NoNestedComments" | "ParentNoNestedComments" => EmitFlags::NO_NESTED_COMMENTS,','"NoNestedComments" | "ParentNoNestedComments" | "ParentAndBodyNoNestedComments" => EmitFlags::NO_NESTED_COMMENTS,').replace('parent: case["variant"] == "ParentNoNestedComments",','parent: case["variant"] == "ParentNoNestedComments",\n                    parent_and_body: case["variant"] == "ParentAndBodyNoNestedComments",');p.write_text(s)
p=R/'crates/emitter/tests/printer_failure_contract.rs';s=p.read_text();(O/p.name).write_text(s)
a='''                assert_eq!(entry["path"], "expression");
                let node = expression_of(arena, s0);'''
b='''                let node = match entry["path"].as_str().unwrap() {
                    "expression" => expression_of(arena, s0),
                    "body" => {
                        let NodeData::FunctionDeclaration(data) = &arena.node(s0).unwrap().data else {
                            panic!("function declaration");
                        };
                        arena.node_ref(s0.source(), data.body.unwrap()).unwrap()
                    }
                    path => panic!("unknown emit-flag path {path}"),
                };'''
assert s.count(a)==1;s=s.replace(a,b).replace('assert_eq!(cases.len(), 21);','assert_eq!(cases.len(), 22);');p.write_text(s)
print(O)
