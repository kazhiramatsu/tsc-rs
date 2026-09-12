# H2.8a G4a/G4b: reproducible source observations

Companion to the [design packet](h2-8a-declaration-comment-ranges.md).
This notebook executes only the vendored TypeScript, with ordinary parsed
source inputs. It writes research evidence into a new external directory;
it does not modify production, existing fixtures, or qualification artifacts.
The instrumented compiler records the actual selected location after the
original selector runs. Its complete command output must equal the unmodified
compiler output. Recorded positions are UTF-16 offsets, as in TypeScript.

Run from the repository root. The output directory must be new. Retain the
script, JSON outputs and actual exit code together. The full command tuples
are in `commands.json`; `traces.json` contains a separate internal observation
population, not additional compatible commands.

```sh
DECL_COMMENT_OUT=$(mktemp -d /tmp/tsc-rs-declaration-comments.XXXXXX)
export DECL_COMMENT_OUT
python3 - <<'PY'
import os
from pathlib import Path
p = Path('docs/design/greenfield/slices/h2-8a-declaration-comment-range-observations.md')
s = p.read_text().split('```javascript\n', 1)[1].split('\n```', 1)[0]
Path(os.environ['DECL_COMMENT_OUT'], 'observe.cjs').write_text(s + '\n')
PY
taskpolicy -b nice -n 15 node "$DECL_COMMENT_OUT/observe.cjs" "$PWD" "$DECL_COMMENT_OUT"
```

The following executable block is the source of the notebook, not a suggested
future script. The shared observer is read from its existing file and checked
by the companion packet's authority pins. The extraction boundaries are exact
and must each occur once. No library/source file is read from outside the
hermetic host except the explicitly mounted pinned library directory.

```javascript
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {createRequire} = require('node:module');
const {pathToFileURL} = require('node:url');
const root = path.resolve(process.argv[2]);
const out = path.resolve(process.argv[3]);
const sha = s => crypto.createHash('sha256').update(s).digest('hex');
const read = p => fs.readFileSync(path.join(root, p), 'utf8');
const write = (name, value) => fs.writeFileSync(path.join(out, name), JSON.stringify(value, null, 2) + '\n', {flag:'wx'});
const tsPath = path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js');
const ts = require(tsPath);
assert.equal(ts.version, '6.0.3');
const fixedFiles = {
  'vendor/typescript-6.0.3/lib/typescript.js':'569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39',
  'scripts/observe-jsdoc-block-scope-container.mjs':'cee190bee2cdca35a5fc9c6702e98cd5a089a53dd43c542723b588ce8fc2201f',
  'crates/oracle/vfs-directory-overlay.mjs':'2868391e75941127f8eb3a352232190fd98c6d794235913e288432e5626aab76',
};
for (const [name,expected] of Object.entries(fixedFiles)) assert.equal(sha(read(name)),expected,name);
const upstream = read('vendor/typescript-6.0.3/lib/_tsc.js');
assert.equal(sha(upstream), '1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3');

function replaceOnce(s, before, after) {
  assert.equal(s.split(before).length, 2, 'unique instrumentation anchor');
  return s.replace(before, after);
}
const compilerSource = fs.readFileSync(tsPath, 'utf8');
const signatureOwner = `function getSignatureTextRangeLocation(signature) {
        if (signature.declaration && signature.declaration.parent) {
          if (isBinaryExpression(signature.declaration.parent) && getAssignmentDeclarationKind(signature.declaration.parent) === 5 /* Property */) {
            return signature.declaration.parent;
          }
          if (isVariableDeclaration(signature.declaration.parent) && signature.declaration.parent.parent) {
            return signature.declaration.parent.parent;
          }
        }
        return signature.declaration;
      }`;
const wrappedOwner = signatureOwner.replace(
  'function getSignatureTextRangeLocation(signature) {',
  'function getSignatureTextRangeLocation(signature) { const selected = (() => {'
).replace(/\n      }$/, '\n      })(); recordRange("G4a", signature.declaration, selected); return selected; }');
let instrumentedSource = replaceOnce(compilerSource, signatureOwner, wrappedOwner);
const methodOwner = 'const location = sig.declaration && isPrototypePropertyAssignment(sig.declaration.parent) ? sig.declaration.parent : sig.declaration;';
instrumentedSource = replaceOnce(instrumentedSource, methodOwner,
  methodOwner + ' recordRange("G4b", sig.declaration, location);');
let trace = [];
function describe(node) {
  if (!node) return null;
  const file = ts.getSourceFileOfNode(node);
  return {kind:ts.SyntaxKind[node.kind],pos:node.pos,end:node.end,file:file?.fileName ?? null};
}
function recordRange(owner, declaration, location) {
  const parent = declaration?.parent;
  trace.push({owner,declaration:describe(declaration),parent:describe(parent),location:describe(location),
    assignment_kind:parent && ts.isBinaryExpression(parent) ? ts.getAssignmentDeclarationKind(parent) : null});
}
const instrumentedModule = {exports:{}};
new Function('module','exports','require','__filename','__dirname','recordRange',instrumentedSource)(
  instrumentedModule,instrumentedModule.exports,createRequire(tsPath),tsPath,path.dirname(tsPath),recordRange);
const probeTs = instrumentedModule.exports;

const defaults = {allowJs:true,checkJs:true,declaration:true,declarationMap:true,sourceMap:true,
  target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.CommonJS,skipDefaultLibCheck:true,
  noErrorTruncation:true,newLine:ts.NewLineKind.CarriageReturnLineFeed,outDir:'/project/out'};
const shapes = [
  ['module-exports','/** @typedef {number} Input */\n/** export owner\n * @param {Input} value\n */\nmodule.exports = function api(value) { return value; };\n'],
  ['exports-property','/** assignment owner\n * @param {number} value\n */\nexports.api = function api(value) { return value; };\n'],
  ['module-exports-property','/** assignment owner\n * @param {number} value\n */\nmodule.exports.api = function api(value) { return value; };\nmodule.exports.api.version = 1;\n'],
  ['ordinary-property','function host() {}\n/** property owner\n * @param {number} value\n */\nhost.api = function api(value) { return value; };\nmodule.exports = host;\n'],
  ['variable','/** variable owner\n * @param {number} value\n */\nvar api = function(value) { return value; };\napi.version = 1;\nmodule.exports = api;\n'],
  ['function-declaration','/** declaration owner\n * @param {number} value\n */\nfunction api(value) { return value; }\napi.version = 1;\nmodule.exports = api;\n'],
  ['prototype-dot','function C() {}\n/** method owner\n * @param {number} value\n */\nC.prototype.method = function(value) { return value; };\nmodule.exports = C;\n'],
  ['prototype-element','function C() {}\n/** method owner\n * @param {number} value\n */\nC["prototype"]["method"] = function(value) { return value; };\nmodule.exports = C;\n'],
  ['static-method','function C() {}\n/** static owner\n * @param {number} value\n */\nC.method = function(value) { return value; };\nmodule.exports = C;\n'],
  ['this-property','function C() {\n/** instance owner\n * @param {number} value\n */\nthis.method = function(value) { return value; };\n}\nmodule.exports = C;\n'],
  ['ordinary-method','class C {\n/** method owner\n * @param {number} value\n */\nmethod(value) { return value; }\n}\nmodule.exports = C;\n'],
  ['prototype-parenthesized','function C() {}\n/** outer owner */\nC.prototype.method = (/** inner owner\n * @param {number} value\n */ function(value) { return value; });\nmodule.exports = C;\n'],
];
const inputs = shapes.map(([name,text]) => ({case_id:'declaration-comment-range/'+name,
  roots:['/project/main.js'],files:[{path:'/project/main.js',text}],options:{...defaults}}));
for (const [name,extra] of [
  ['remove-comments',{removeComments:true}],['declaration-only',{emitDeclarationOnly:true}],
  ['lf-bom-listings',{newLine:ts.NewLineKind.LineFeed,emitBOM:true,listEmittedFiles:true}],
  ['esnext',{target:ts.ScriptTarget.ESNext}],
]) {
  inputs.push({case_id:'declaration-comment-range/'+name,roots:['/project/main.js'],
    files:[{path:'/project/main.js',text:shapes.find(([key])=>key==='prototype-dot')[1]}],options:{...defaults,...extra}});
}
inputs.push({case_id:'declaration-comment-range/blocked-semantic',roots:['/project/main.js'],
  files:[{path:'/project/main.js',text:shapes[6][1]+'/** @type {number} */\nconst invalid = "text";\n'}],
  options:{...defaults,noEmitOnError:true}});

async function main() {
  const {createHermeticDirectoryOverlay} = await import(pathToFileURL(path.join(root,'crates/oracle/vfs-directory-overlay.mjs')));
  const observerPath = 'scripts/observe-jsdoc-block-scope-container.mjs';
  const observer = read(observerPath);
  const start = 'function diagnostic(d) {';
  const end = 'const cases = inputs.map(input => {';
  assert.equal(observer.split(start).length,2);
  assert.equal(observer.split(end).length,2);
  const functions = start + observer.split(start)[1].split(end)[0];
  const makeObserve = api => new Function('root','ts','fs','path','assert','createHermeticDirectoryOverlay',functions+'\nreturn observe;')(
    root,api,fs,path,assert,createHermeticDirectoryOverlay);
  const ordinary = makeObserve(ts), instrumented = makeObserve(probeTs);
  const commands=[], traces=[];
  for (const input of inputs) {
    const first=ordinary(input); assert.deepEqual(ordinary(input),first,input.case_id+' ordinary repeat');
    trace=[]; assert.deepEqual(instrumented(input),first,input.case_id+' probe inertness'); const firstTrace=trace;
    trace=[]; assert.deepEqual(instrumented(input),first,input.case_id+' second probe inertness');
    assert.deepEqual(trace,firstTrace,input.case_id+' trace repeat');
    commands.push({...input,typescript_observation:first}); traces.push({case_id:input.case_id,trace:firstTrace});
  }
  const predicates=[];
  for (const extension of ['js','ts']) for (const [name,expression] of [
    ['property','o.m = function() {};'],['exports','exports.m = function() {};'],
    ['module','module.exports = function() {};'],['module-property','module.exports.m = function() {};'],
    ['prototype','C.prototype.m = function() {};'],['prototype-element','C["prototype"]["m"] = function() {};'],
    ['this','this.m = function() {};'],['plain-assignment','m = function() {};'],
    ['compound','o.m += function() {};'],['dynamic-receiver','make().m = function() {};'],
    ['void-zero','o.m = void 0;'],['parenthesized-key','C["prototype"][("m")] = function() {};'],
  ]) {
    const classify=()=>{
      const source=ts.createSourceFile('/project/predicate.'+extension,expression,ts.ScriptTarget.ESNext,true);
      return ts.getAssignmentDeclarationKind(source.statements[0].expression);
    };
    const kind=classify(); assert.equal(classify(),kind);
    predicates.push({case_id:name+'/'+extension,source:expression,assignment_kind:kind,
      signature_uses_assignment:kind===5,method_uses_assignment:kind===3});
  }
  // These are direct sentinel/identity observations, separate from Programs.
  // Execute the original selector body with the same exported predicates.
  const selectSignature=new Function('isBinaryExpression','getAssignmentDeclarationKind','isVariableDeclaration',
    'return ('+signatureOwner+');')(ts.isBinaryExpression,ts.getAssignmentDeclarationKind,ts.isVariableDeclaration);
  const sentinels=[];
  for (const shape of ['missing-declaration','parentless-declaration','variable-without-list','variable-with-list']) {
    const observe=()=>{
      const declaration=ts.factory.createFunctionExpression(undefined,undefined,undefined,undefined,[],undefined,ts.factory.createBlock([]));
      let variable, list;
      if (shape.startsWith('variable-')) {
        variable=ts.factory.createVariableDeclaration('fn',undefined,undefined,declaration);
        declaration.parent=variable;
        if (shape==='variable-with-list') {
          list=ts.factory.createVariableDeclarationList([variable]); variable.parent=list;
        }
      }
      const selected=selectSignature(shape==='missing-declaration'?{}:{declaration});
      return selected===undefined?'absent':selected===declaration?'declaration':selected===list?'variable-list':'unexpected';
    };
    const selected=observe(); assert.equal(observe(),selected);
    sentinels.push({case_id:shape,selected});
  }
  const pinPaths=['vendor/typescript-6.0.3/lib/_tsc.js','vendor/typescript-6.0.3/lib/typescript.js',observerPath,
    'crates/oracle/vfs-directory-overlay.mjs','crates/checker/src/node_builder/statements.rs',
    'crates/checker/src/node_builder/chains.rs','crates/binder/src/assignment.rs',
    'crates/compiler/tests/integration/h2_7c_declaration_blocking.rs','crates/compiler/tests/integration/h2_7b_w4a_controls.rs',
    'crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs','docs/design/greenfield/emitter-architecture.md',
    'docs/design/greenfield/post-h1-completion-slices.md','ratchets/h2-8a-global-after-a6-37.v1.json',
    'ratchets/h2-8a-convergence-causes.v1.json'];
  const authority=pinPaths.map(p=>({path:p,sha256:sha(fs.readFileSync(path.join(root,p)))}));
  const wanted=new Set(['getSignatureTextRangeLocation','serializeAsFunctionNamespaceMerge','makeSerializePropertySymbol',
    'getAssignmentDeclarationKind','getAssignmentDeclarationKindWorker','getAssignmentDeclarationPropertyAccessKind',
    'isPrototypePropertyAssignment','isPrototypeAccess','isBindableStaticAccessExpression','isBindableStaticElementAccessExpression',
    'isBindableStaticNameExpression','isLiteralLikeElementAccess','getElementOrPropertyAccessName',
    'getElementOrPropertyAccessArgumentExpressionOrName','getInitializerOfBinaryExpression','isVoidZero',
    'getRightMostAssignedExpression','isModuleExportsAccessExpression','isExportsIdentifier','isModuleIdentifier',
    'isInJSFile','isDynamicName','setTextRange2','setTextRange','getOriginalNode','getSourceFileOfNode']);
  const parsed=ts.createSourceFile('_tsc.js',upstream,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  const lines=upstream.split('\n'), spans=[], bodies=new Map();
  function visit(node) {
    if (ts.isFunctionDeclaration(node) && node.name && wanted.has(node.name.text)) {
      const start=parsed.getLineAndCharacterOfPosition(node.getStart(parsed)).line+1;
      const end=parsed.getLineAndCharacterOfPosition(node.end-1).line+1;
      spans.push({name:node.name.text,start,end,sha256:sha(lines.slice(start-1,end).join('\n')+'\n')});
      bodies.set(node.name.text,node.getText(parsed));
    }
    ts.forEachChild(node,visit);
  }
  visit(parsed); assert.deepEqual([...new Set(spans.map(s=>s.name))].sort(),[...wanted].sort());
  assert.equal(spans.length,wanted.size);
  const sourceSetRange=new Function('nodeIsSynthesized','factory','getSourceFileOfNode','getOriginalNode','setOriginalNode','setTextRange',
    'return ('+bodies.get('setTextRange2')+');')(ts.nodeIsSynthesized,ts.factory,ts.getSourceFileOfNode,ts.getOriginalNode,ts.setOriginalNode,ts.setTextRange);
  const ranges=[];
  for (const shape of ['same-source','foreign-source','no-location','parsed-input']) {
    const observe=()=>{
      const local=ts.createSourceFile('/project/a.ts','type Local = number;',ts.ScriptTarget.ESNext,true);
      const foreign=ts.createSourceFile('/project/b.ts','type Foreign = string;',ts.ScriptTarget.ESNext,true);
      const localName=local.statements[0].name, foreignName=foreign.statements[0].name;
      const input=shape==='parsed-input'?localName:ts.setOriginalNode(ts.factory.createIdentifier('generated'),localName);
      const location=shape==='foreign-source'?foreignName:shape==='no-location'?undefined:localName;
      const result=sourceSetRange({enclosingFile:local},input,location);
      return {same_identity:result===input,pos:result.pos,end:result.end,original:describe(ts.getOriginalNode(result))};
    };
    const observation=observe(); assert.deepEqual(observe(),observation); ranges.push({case_id:shape,observation});
  }
  write('commands.json',{version:1,typescript:ts.version,repetitions:2,cases:commands});
  write('traces.json',{version:1,repetitions:2,cases:traces,predicates,sentinels,ranges});
  write('authority.json',{files:authority,spans,probe_compiler_sha256:sha(instrumentedSource)});
  const files=['commands.json','traces.json','authority.json'].map(name=>({name,sha256:sha(fs.readFileSync(path.join(out,name)))}));
  write('receipt.json',{kind:'source-design-only',ordinary_commands:commands.length,repetitions:2,
    instrumented_commands:commands.length,repeated_inertness:true,predicate_controls:predicates.length,
    sentinel_controls:sentinels.length,range_controls:ranges.length,native_executions:0,files});
  console.log(JSON.stringify({ordinary_commands:commands.length,predicate_controls:predicates.length,sentinel_controls:sentinels.length,range_controls:ranges.length,files}));
}
main().catch(error=>{console.error(error);process.exitCode=1;});
```


## Parameter-tag dependency observations

The native before run exposed missing JSDoc parameter annotation reuse in
13 of the original 17 commands. This extension preserves those frozen inputs
and adds branch controls for the tag lookup dependency; it does not replace
any expectation. Run the second JavaScript block with the first block's
prefix through `const defaults` (stop immediately before `const shapes = [`):

```python
import re, subprocess, tempfile
from pathlib import Path
notebook = Path('docs/design/greenfield/slices/h2-8a-declaration-comment-range-observations.md').read_text()
blocks = re.findall(r'```javascript\n(.*?)\n```', notebook, re.S)
script = blocks[0].split('const shapes = [', 1)[0] + blocks[1]
out = Path(tempfile.mkdtemp(prefix='tsc-rs-declaration-parameter-tags-'))
program = out / 'observe-parameter-tags.cjs'
program.write_text(script)
subprocess.run(['taskpolicy', '-b', 'nice', '-n', '15', 'node',
    str(program), str(Path.cwd()), str(out)], check=True)
print(out)
```

```javascript
const parameterShapes = [
  ['binding-object', '/** @param {{ x: number }} item */\nexports.f = function({x}) { return x; };\n'],
  ['binding-array', '/** @param {[number, string]} pair */\nexports.f = function([first, second]) { return first; };\n'],
  ['binding-index', '/** @param {string} first\n * @param {{ x: number }} second */\nexports.f = function(first, {x}) { return x; };\n'],
  ['qualified-tag-negative', '/** @param {object} options\n * @param {number} options.value */\nexports.f = function(value) { return value; };\n'],
  ['missing-name', '/** @param {number} other */\nexports.f = function(value) { return value; };\n'],
  ['last-block', '/** @param {string} value */\n/** @param {number} value */\nexports.f = function(value) { return value; };\n'],
  ['first-typed', '/** @param value\n * @param {number} value */\nexports.f = function(value) { return value; };\n'],
  ['type-tag-precedence', '/** @param {number} value */\nexports.f = function(/** @type {string} */ value) { return value; };\n'],
  ['inline-function', 'exports.f = /** @param {number} value */ function(value) { return value; };\n'],
  ['untyped', 'exports.f = function(value) { return value; };\n'],
  ['ts-direct-type', '/** @param {string} value */\nexport function f(value: number) { return value; }\n', 'ts'],
  ['parenthesized-function', '/** @param {string} value */\nexports.f = (/** @param {number} value */ function(value) { return value; });\n'],
];
async function main() {
  const {createHermeticDirectoryOverlay} = await import(pathToFileURL(path.join(root,'crates/oracle/vfs-directory-overlay.mjs')));
  const observer = read('scripts/observe-jsdoc-block-scope-container.mjs');
  const start = 'function diagnostic(d) {', end = 'const cases = inputs.map(input => {';
  assert.equal(observer.split(start).length,2); assert.equal(observer.split(end).length,2);
  const functions = start + observer.split(start)[1].split(end)[0];
  const observe = new Function('root','ts','fs','path','assert','createHermeticDirectoryOverlay',functions+'\nreturn observe;')(
    root,ts,fs,path,assert,createHermeticDirectoryOverlay);
  const cases = [];
  for (const [name,text,extension='js'] of parameterShapes) {
    const filename='/project/main.'+extension;
    const input={case_id:'declaration-comment-parameter-tags/'+name,roots:[filename],files:[{path:filename,text}],options:{...defaults}};
    const first=observe(input); assert.deepEqual(observe(input),first,name+' complete command repeat');
    const inspect=()=>{
      const source=ts.createSourceFile(filename,text,ts.ScriptTarget.ES2015,true);
      const parameters=[];
      function visit(node) {
        if (ts.isParameter(node)) parameters.push({
          parameter:describe(node), name:node.name?.getText(source) ?? null,
          tags:ts.getJSDocParameterTags(node).map(tag=>({...describe(tag),name:tag.name?.getText(source) ?? null})),
          jsdoc_type:describe(ts.getJSDocType(node)), effective_type:describe(ts.getEffectiveTypeAnnotationNode(node)),
        });
        ts.forEachChild(node,visit);
      }
      visit(source);return parameters;
    };
    const parameters=inspect(); assert.deepEqual(inspect(),parameters,name+' parameter lookup repeat');
    cases.push({...input,typescript_observation:first,parameter_trace:parameters});
  }
  write('parameter-tags.json',{version:1,typescript:ts.version,repetitions:2,cases});
  write('parameter-tags-receipt.json',{kind:'source-observation',repetitions:2,commands:cases.length,
    fixture_sha256:sha(fs.readFileSync(path.join(out,'parameter-tags.json'))),
    diagnostics:cases.map(c=>({case_id:c.case_id,codes:c.typescript_observation.reported_diagnostics.map(d=>d.code)}))});
  console.log(fs.readFileSync(path.join(out,'parameter-tags-receipt.json'),'utf8'));
}
main().catch(error=>{console.error(error);process.exitCode=1;});
```
