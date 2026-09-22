import ts from "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js";
const source="function f() {\n// head\n\nx();\n// tail\n}\n";
for(const [name,flag] of [["none",0],["no-leading",ts.EmitFlags.NoLeadingComments],["no-trailing",ts.EmitFlags.NoTrailingComments],["no-nested",ts.EmitFlags.NoNestedComments],["no-own",ts.EmitFlags.NoComments]]) {
 const file=ts.createSourceFile("/a.ts",source,ts.ScriptTarget.Latest,true);ts.setEmitFlags(file.statements[0].body,flag);
 console.log(JSON.stringify({name,text:ts.createPrinter({newLine:ts.NewLineKind.LineFeed}).printFile(file)}));
}
