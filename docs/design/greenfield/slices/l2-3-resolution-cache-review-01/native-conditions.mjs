import assert from "node:assert/strict";
import ts from "../../../../../vendor/typescript-6.0.3/lib/typescript.js";

assert.equal(ts.version, "6.0.3");
function observe(customConditions) {
  const files = new Map([
    ["/p/main.ts", "import 'pkg';"],
    ["/p/node_modules/pkg/package.json", '{"name":"pkg","exports":{"a,b":"./joined.d.ts","a":"./split.d.ts"}}'],
    ["/p/node_modules/pkg/joined.d.ts", "export {};"],
    ["/p/node_modules/pkg/split.d.ts", "export {};"],
  ]);
  const host = {
    fileExists: p => files.has(p),
    readFile: p => files.get(p),
    directoryExists: p => [...files.keys()].some(f => f.startsWith(p + "/")),
    getCurrentDirectory: () => "/p",
    useCaseSensitiveFileNames: true,
    realpath: p => p,
  };
  return ts.resolveModuleName("pkg", "/p/main.ts", {
    moduleResolution: ts.ModuleResolutionKind.NodeNext,
    module: ts.ModuleKind.NodeNext,
    customConditions,
  }, host, undefined, undefined, ts.ModuleKind.CommonJS);
}
const cases = [["a,b"], ["a", "b"]].map(customConditions => {
  const result = observe(customConditions);
  assert.deepEqual(observe(customConditions), result);
  return {customConditions, result};
});
assert.equal(cases[0].result.resolvedModule.resolvedFileName, "/p/node_modules/pkg/joined.d.ts");
assert.equal(cases[1].result.resolvedModule.resolvedFileName, "/p/node_modules/pkg/split.d.ts");
console.log(JSON.stringify({typescript:ts.version, repetitions:2, cases}, null, 2));
