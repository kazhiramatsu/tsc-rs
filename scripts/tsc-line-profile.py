#!/usr/bin/env python3
"""Build a line-profiling copy of the vendored TypeScript compiler.

Every checkSourceElement / checkDeferredNode is timed and attributed to the
line of its node (skipTrivia(node.pos), as tsc-rs's TSRS_LINE_PROFILE does),
with the deltas of totalInstantiationCount and typeCount, self and inclusive.
The copy writes the TSRS_LINE_PROFILE TSV layout to the path in
TSC_LINE_PROFILE, so scripts/line-profile-diff.py can join the two profiles.

usage: scripts/tsc-line-profile.py            # writes target/tsc-line-profile/_tsc.js
       TSC_LINE_PROFILE=out.tsv node target/tsc-line-profile/_tsc.js --pretty false -p tsconfig.json
"""
import os, pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "vendor/typescript-6.0.3/lib/_tsc.js"
OUT_DIR = ROOT / "target/tsc-line-profile"


def replace_once(text, old, new):
    count = text.count(old)
    if count != 1:
        sys.exit(f"anchor found {count} times in {SRC}: {old[:100]!r}")
    return text.replace(old, new)


def build():
    text = SRC.read_text()
    text = replace_once(
        text,
        "function createTypeChecker(host) {\n",
        r'''var __lp_rows = new Map();
var __lp_registered = false;
function __lp_register() {
  if (__lp_registered) return; __lp_registered = true;
  process.on("exit", () => {
    const out = process.env.TSC_LINE_PROFILE; if (!out) return;
    const ops = ["signatures","conditional","relations","resolve_name","mapped","indexed","members","instantiations","types"];
    const head = ["self_ms","inclusive_ms","hits"].concat(ops, ops.map(o => "incl_" + o), ["file","line","text"]).join("\t");
    const lines = [head];
    const rows = [...__lp_rows.values()].sort((a, b) => b.self_ms - a.self_ms);
    for (const r of rows) {
      const self = [0,0,0,0,0,0,0,r.inst,r.types], incl = [0,0,0,0,0,0,0,r.incl_inst,r.incl_types];
      lines.push([r.self_ms.toFixed(3), r.incl_ms.toFixed(3), r.hits].concat(self, incl, [r.file, r.line, r.text]).join("\t"));
    }
    require("fs").writeFileSync(out, lines.join("\n") + "\n");
    let total = 0; for (const r of rows) total += r.self_ms;
    process.stderr.write(`tsc line profile: ${rows.length} lines, ${total.toFixed(1)} ms self time in statements; heaviest:\n`);
    for (const r of rows.slice(0, 15)) process.stderr.write(`${r.self_ms.toFixed(1).padStart(10)} ms ${String(r.inst).padStart(7)} inst ${String(r.types).padStart(6)} types  ${r.file.split("/").slice(-1)[0]}:${r.line}  ${r.text}\n`);
  });
}
function createTypeChecker(host) {
  __lp_register();
  var __lp_stack = [];
  function __lp_enter(node) {
    __lp_stack.push({ node, t: performance.now(), inst: totalInstantiationCount, types: typeCount, child_ms: 0, child_inst: 0, child_types: 0 });
  }
  function __lp_exit() {
    const f = __lp_stack.pop();
    const incl_ms = performance.now() - f.t, incl_inst = totalInstantiationCount - f.inst, incl_types = typeCount - f.types;
    if (__lp_stack.length) { const p = __lp_stack[__lp_stack.length - 1]; p.child_ms += incl_ms; p.child_inst += incl_inst; p.child_types += incl_types; }
    const sf = getSourceFileOfNode(f.node); if (!sf) return;
    const pos = skipTrivia(sf.text, f.node.pos);
    const lc = getLineAndCharacterOfPosition(sf, pos);
    const key = sf.fileName + "\t" + (lc.line + 1);
    let r = __lp_rows.get(key);
    if (!r) {
      const starts = getLineStarts(sf); const a = starts[lc.line], b = lc.line + 1 < starts.length ? starts[lc.line + 1] : sf.text.length;
      r = { file: sf.fileName, line: lc.line + 1, self_ms: 0, incl_ms: 0, hits: 0, inst: 0, types: 0, incl_inst: 0, incl_types: 0, text: sf.text.slice(a, b).replace(/[\r\n\t]+/g, " ").trim().slice(0, 160) };
      __lp_rows.set(key, r);
    }
    r.hits++; r.self_ms += incl_ms - f.child_ms; r.incl_ms += incl_ms;
    r.inst += incl_inst - f.child_inst; r.types += incl_types - f.child_types; r.incl_inst += incl_inst; r.incl_types += incl_types;
  }
''',
    )
    text = replace_once(
        text,
        """      currentNode = node;
      instantiationCount = 0;
      checkSourceElementWorker(node);
      currentNode = saveCurrentNode;
""",
        """      currentNode = node;
      instantiationCount = 0;
      __lp_enter(node);
      try { checkSourceElementWorker(node); } finally { __lp_exit(); }
      currentNode = saveCurrentNode;
""",
    )
    text = replace_once(
        text,
        """    const saveCurrentNode = currentNode;
    currentNode = node;
    instantiationCount = 0;
    switch (node.kind) {
      case 214 /* CallExpression */:
      case 215 /* NewExpression */:
      case 216 /* TaggedTemplateExpression */:
      case 171 /* Decorator */:
      case 287 /* JsxOpeningElement */:
        resolveUntypedCall(node);
        break;
""",
        """    const saveCurrentNode = currentNode;
    currentNode = node;
    instantiationCount = 0;
    __lp_enter(node);
    try { switch (node.kind) {
      case 214 /* CallExpression */:
      case 215 /* NewExpression */:
      case 216 /* TaggedTemplateExpression */:
      case 171 /* Decorator */:
      case 287 /* JsxOpeningElement */:
        resolveUntypedCall(node);
        break;
""",
    )
    text = replace_once(
        text,
        """      case 227 /* BinaryExpression */:
        if (isInstanceOfExpression(node)) {
          resolveUntypedCall(node);
        }
        break;
    }
    currentNode = saveCurrentNode;
    (_b = tracing) == null ? void 0 : _b.pop();
  }
  function checkSourceFile(node, nodesToCheck) {
""",
        """      case 227 /* BinaryExpression */:
        if (isInstanceOfExpression(node)) {
          resolveUntypedCall(node);
        }
        break;
    } } finally { __lp_exit(); }
    currentNode = saveCurrentNode;
    (_b = tracing) == null ? void 0 : _b.pop();
  }
  function checkSourceFile(node, nodesToCheck) {
""",
    )
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / "_tsc.js").write_text(text)
    # The compiler locates its default libraries next to the executing script.
    for lib in sorted(SRC.parent.glob("lib.*.d.ts")):
        link = OUT_DIR / lib.name
        if link.is_symlink() or link.exists():
            link.unlink()
        os.symlink(lib, link)
    print(f"wrote {OUT_DIR / '_tsc.js'} ({len(text)} bytes) and {len(list(OUT_DIR.glob('lib.*.d.ts')))} library links")


if __name__ == "__main__":
    build()
