// Expands the A41-BINDING (C02) witness families into a concrete input
// manifest of complete TypeScript commands. Every family/variant row becomes
// one case per option combination; the manifest fixes all IDs and the planned
// count before any observation runs (handoff: IDs and the input manifest are
// fixed before the native run, expected suffixes are observed, never written).
//
// Families (handoff "必須 witness"): parse-census, global, nested, reserved,
// computed, ordering, lifecycle (bundle publication across sources; CommonJS
// module publication contrasts). synthetic-census and the direct lifecycle
// rows are direct controls (`observe-decorator-bindings.mjs direct`).
//
// usage: node scripts/generate-decorator-binding-inputs.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/decorator-binding-inputs.json");

const targets = [["es2015", 2], ["es2022", 9], ["esnext", 99]];
const modes = [["set", false], ["define", true]];

const PRELUDE = `const events: unknown[] = [];
function dec(value: any, context: any): any { events.push(["dec", String(context.name)]); return value; }
function key(): "x" { events.push("key"); return "x"; }
function key2(): "y" { events.push("key2"); return "y"; }
function record(value: unknown) { events.push(value); }
const ns = { dec };
const keys = { get x(): "x" { events.push("key"); return "x"; }, get y(): "y" { events.push("key2"); return "y"; } };
`;
const TAIL = `export const tail = events.length;\n`;

// A decorated class whose helper names cover every producer of the
// standard-decorator transform: class decorator (_classDecorators,
// _classDescriptor, _classExtraInitializers, _classThis, _metadata), a
// decorated instance field (_x_decorators/_x_initializers/_x_extraInitializers),
// a decorated static method (_static_m_decorators, _staticExtraInitializers),
// a decorated instance method (_instanceExtraInitializers) and an extends
// clause (_classSuper).
const FULL_CLASS = `@dec
export class C extends Object {
    @dec x = 1;
    @dec static m() { return 1; }
    @dec n() { return 2; }
}
record(C.name);
`;
// A member decorator whose expression uses lexical `this` inside an object
// method makes the pending decorator assignment reference `_outerThis`
// (SUPER witnesses `member-decorator-outer-this` / `outer-this-and-cache-temp`).
const OUTER_THIS_CLASS = `const holder = { dec, make() { return @dec class extends Object { @(this.dec) static n() {} }; } };
const D = holder.make();
record(D.name);
`;
// A decorated computed member hoists the computed-key cache temp `_a`
// (getGeneratedNameForNode(computedPropertyName), ReservedInNestedScopes).
// Fields and accessors use the entity-name getter `keys.x` (late-bindable,
// so no TS1166; still not simple-inlineable, so `__propKey` and the cache
// temp are produced and the evaluation stays observable); methods keep `key()`.
const COMPUTED_CLASS = `@dec
export class K {
    @dec static [keys.x] = 1;
}
record(K.name);
`;
// Anonymous decorated class expression / default export: the class
// reference is getLocalName(node) = class_1 / default_1.
const CLASS_EXPRESSION = `const E = @dec class { @dec static m() {} };
record(E.name);
`;
const DEFAULT_EXPORT = `export default @dec class { @dec static m() {} }
`;

const variants = [];
const add = (family, variant, body, options = {}) => variants.push({ family, variant, body, ...options });

// ---------------------------------------------------------------- parse-census
// The spelling appears in the parsed source (SourceFile.identifiers) in a
// declaration/reference, or only in a string / comment / JSDoc tag / type
// position / nested scope / escaped form. Contrast rows per name class.
const censusNames = [
  ["_metadata", FULL_CLASS], ["_classThis", FULL_CLASS], ["_classSuper", FULL_CLASS], ["_classDecorators", FULL_CLASS],
  ["_x_decorators", FULL_CLASS], ["_static_m_decorators", FULL_CLASS], ["_staticExtraInitializers", FULL_CLASS],
  ["_a", COMPUTED_CLASS], ["_outerThis", OUTER_THIS_CLASS], ["class_1", CLASS_EXPRESSION], ["default_1", DEFAULT_EXPORT],
];
for (const [name, body] of censusNames) {
  add("parse-census", `declared-${name}`, `let ${name} = 0;\nrecord(${name});\n` + body);
}
add("parse-census", "declared-after-class-_metadata", FULL_CLASS + `let _metadata = 0;\nrecord(_metadata);\n`);
add("parse-census", "property-key-_metadata", `const o = { _metadata: 1 };\nrecord(o._metadata);\n` + FULL_CLASS);
add("parse-census", "property-key-_x_decorators", `const o = { _x_decorators: 1 };\nrecord(o._x_decorators);\n` + FULL_CLASS);
add("parse-census", "string-only-_metadata", `record("_metadata _classThis _x_decorators _a");\n` + FULL_CLASS);
add("parse-census", "comment-only-_metadata", `// _metadata _classThis _x_decorators _a\n/* _classSuper */\n` + FULL_CLASS);
add("parse-census", "jsdoc-tag-only-_metadata", `/** @param _metadata the metadata */\nfunction f(x: number) { return x; }\nrecord(f(1));\n` + FULL_CLASS);
add("parse-census", "jsdoc-see-only-_classThis", `/** @see _classThis */\nfunction f(x: number) { return x; }\nrecord(f(1));\n` + FULL_CLASS);
add("parse-census", "type-position-only-_metadata", `type T = { _metadata: number };\nconst t: T = { _metadata: 1 };\nrecord(t);\n` + FULL_CLASS);
add("parse-census", "type-alias-name-_classThis", `type _classThis = number;\nconst v: _classThis = 1;\nrecord(v);\n` + FULL_CLASS);
add("parse-census", "nested-scope-only-_metadata", `function f() { let _metadata = 1; return _metadata; }\nrecord(f());\n` + FULL_CLASS);
add("parse-census", "nested-scope-only-_x_decorators", `function f() { let _x_decorators = 1; return _x_decorators; }\nrecord(f());\n` + FULL_CLASS);
add("parse-census", "escaped-identifier-_metadata", `let \\u005fmetadata = 0;\nrecord(_metadata);\n` + FULL_CLASS);
add("parse-census", "escaped-identifier-_a", `let \\u005fa = 0;\nrecord(_a);\n` + COMPUTED_CLASS);
add("parse-census", "numbered-taken-_metadata_1", `let _metadata = 0, _metadata_1 = 1;\nrecord(_metadata + _metadata_1);\n` + FULL_CLASS);
add("parse-census", "numbered-taken-_x_decorators_1", `let _x_decorators = 0, _x_decorators_1 = 1;\nrecord(_x_decorators + _x_decorators_1);\n` + FULL_CLASS);
add("parse-census", "temp-sequence-_a-_b", `let _a = 0, _b = 1;\nrecord(_a + _b);\n` + COMPUTED_CLASS);
add("parse-census", "class-reference-class_2", `let class_1 = 0, class_2 = 1;\nrecord(class_1 + class_2);\n` + CLASS_EXPRESSION);
add("parse-census", "label-only-_metadata", `_metadata: for (const i of [1]) { record(i); break _metadata; }\n` + FULL_CLASS);

// ---------------------------------------------------------------- global
// A second root file. A script (no import/export) contributes its top-level
// declarations to the checker's `globals`; a module's locals do not.
const globalNames = [
  ["_metadata", FULL_CLASS], ["_classThis", FULL_CLASS], ["_classSuper", FULL_CLASS], ["_x_decorators", FULL_CLASS],
  ["_staticExtraInitializers", FULL_CLASS], ["_a", COMPUTED_CLASS], ["_outerThis", OUTER_THIS_CLASS],
  ["class_1", CLASS_EXPRESSION], ["default_1", DEFAULT_EXPORT],
];
for (const [name, body] of globalNames) {
  add("global", `script-let-${name}`, body, { extraFiles: [{ path: "/project/globals.ts", text: `let ${name} = 0;\n` }] });
}
add("global", "script-function-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `function _metadata() { return 1; }\n` }] });
add("global", "script-type-alias-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `type _metadata = number;\n` }] });
add("global", "script-namespace-_classThis", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `namespace _classThis { export const v = 1; }\n` }] });
add("global", "script-declare-let-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `declare let _metadata: number;\n` }] });
add("global", "script-block-scoped-not-global-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `{ let _metadata = 0; }\n` }] });
add("global", "script-numbered-chain-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `let _metadata = 0, _metadata_1 = 1, _metadata_2 = 2;\n` }] });
add("global", "script-temp-chain-_a-_b", COMPUTED_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `let _a = 0, _b = 1;\n` }] });
add("global", "module-local-_metadata", FULL_CLASS, { extraFiles: [{ path: "/project/other.ts", text: `export const _metadata = 1;\n` }] });
add("global", "module-local-_x_decorators", FULL_CLASS, { extraFiles: [{ path: "/project/other.ts", text: `export const _x_decorators = 1;\n` }] });
add("global", "declare-global-var-_metadata", `declare global { var _metadata: number; }\n` + FULL_CLASS);
add("global", "declare-global-in-module-file-_classThis", FULL_CLASS, { extraFiles: [{ path: "/project/other.ts", text: `export {};\ndeclare global { let _classThis: number; }\n` }] });
add("global", "script-and-source-both-_metadata", `let _metadata = 0;\nrecord(_metadata);\n` + FULL_CLASS, { extraFiles: [{ path: "/project/globals.ts", text: `let _metadata_1 = 0;\n` }] });

// ---------------------------------------------------------------- nested
const INNER = `@dec class Inner extends Object { @dec x = 1; @dec static m() { return 1; } }\nrecord(Inner.name);`;
add("nested", "inner-in-static-block", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    @dec static m() { return 1; }\n    static {\n        ${INNER}\n    }\n}\nrecord(C.name);\n`);
add("nested", "inner-in-method", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    @dec static m() { return 1; }\n    run() {\n        ${INNER}\n    }\n}\nrecord(new C().run());\n`);
add("nested", "inner-in-constructor", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    constructor() {\n        super();\n        ${INNER}\n    }\n}\nrecord(new C().x);\n`);
add("nested", "inner-in-static-field-initializer", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    static f = (() => { ${INNER} return 1; })();\n}\nrecord(C.f);\n`);
add("nested", "inner-in-instance-field-initializer", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    f = (() => { ${INNER} return 1; })();\n}\nrecord(new C().f);\n`);
add("nested", "inner-in-decorator-expression", `function wrap(c: any) { events.push(["wrap", c.name]); return dec; }\n@wrap(@dec class A extends Object { @dec x = 1; @dec static m() { return 1; } })\nexport class C extends Object {\n    @dec x = 1;\n    @dec static m() { return 1; }\n}\nrecord(C.name);\n`);
add("nested", "inner-in-heritage", `@dec\nexport class C extends (@dec class B extends Object { @dec x = 1; @dec static m() { return 1; } }) {\n    @dec x = 1;\n    @dec static m() { return 1; }\n}\nrecord(C.name);\n`);
add("nested", "sibling-same-members", FULL_CLASS + `@dec\nexport class D extends Object {\n    @dec x = 1;\n    @dec static m() { return 1; }\n    @dec n() { return 2; }\n}\nrecord(D.name);\n`);
add("nested", "sibling-outer-this-twice", OUTER_THIS_CLASS + `const holder2 = { dec, make() { return @dec class extends Object { @(this.dec) static p() {} }; } };\nconst D2 = holder2.make();\nrecord(D2.name);\n`);
add("nested", "sibling-class-expressions", CLASS_EXPRESSION + `const F = @dec class { @dec static m() {} };\nrecord(F.name);\n`);
add("nested", "sibling-default-and-expression", DEFAULT_EXPORT + CLASS_EXPRESSION);
add("nested", "sibling-computed-temps", COMPUTED_CLASS + `@dec\nexport class L {\n    @dec static [keys.y] = 1;\n}\nrecord(L.name);\n`);
add("nested", "ordinary-function-scopes", `function make1() { @dec class C1 extends Object { @dec x = 1; @dec static m() { return 1; } } return C1; }\nfunction make2() { @dec class C2 extends Object { @dec x = 1; @dec static m() { return 1; } } return C2; }\nrecord(make1().name);\nrecord(make2().name);\n`);
add("nested", "inner-computed-in-outer-computed-scope", `@dec\nexport class K {\n    @dec static [keys.x] = 1;\n    static {\n        @dec class Inner { @dec static [keys.y] = 1; }\n        record(Inner.name);\n    }\n}\nrecord(K.name);\n`);
add("nested", "triple-nesting", `@dec\nexport class C extends Object {\n    @dec x = 1;\n    static {\n        @dec class M extends Object {\n            @dec x = 1;\n            static {\n                @dec class I extends Object { @dec x = 1; }\n                record(I.name);\n            }\n        }\n        record(M.name);\n    }\n}\nrecord(C.name);\n`);

// ---------------------------------------------------------------- reserved
// createClassInfo: a static private / static auto-accessor member selects
// `_classThis` with ReservedInNestedScopes (isUniqueName domain: parsed
// identifiers, reserved names and the printer's generatedNames) instead of
// FileLevel (parsed identifiers and globals only). The domain difference is
// visible only next to another class.
const PLAIN_A = `@dec\nexport class A extends Object {\n    @dec x = 1;\n    static f = 1;\n}\nrecord(A.name);\n`;
const reservedTriggers = [
  ["static-private-field", `    static #p = 1;\n    static readP() { return B.#p; }\n`],
  ["static-private-method", `    static #m() { return 1; }\n    static callM() { return B.#m(); }\n`],
  ["static-private-accessor", `    static get #g() { return 1; }\n    static readG() { return B.#g; }\n`],
  ["static-auto-accessor", `    static accessor a = 1;\n`],
  ["static-private-auto-accessor", `    static accessor #a = 1;\n    static readA() { return B.#a; }\n`],
  ["decorated-static-private-auto-accessor", `    @dec static accessor #a = 1;\n    static readA() { return B.#a; }\n`],
  ["instance-private-field-only", `    #p = 1;\n    readP() { return this.#p; }\n`],
  ["public-static-field-only", `    static f = 1;\n`],
  ["instance-auto-accessor-only", `    accessor a = 1;\n`],
  ["static-block-only", `    static { record("block"); }\n`],
];
for (const [name, members] of reservedTriggers) {
  const B = `@dec\nexport class B extends Object {\n    @dec x = 1;\n${members}}\nrecord(B.name);\n`;
  add("reserved", `file-level-then-${name}`, PLAIN_A + B);
  add("reserved", `${name}-then-file-level`, B + PLAIN_A);
}
add("reserved", "scoped-then-scoped", `@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n}\nrecord(B.name);\n` + `@dec\nexport class D extends Object {\n    @dec x = 1;\n    static #q = 1;\n    static readQ() { return D.#q; }\n}\nrecord(D.name);\n`);
add("reserved", "outer-file-level-inner-scoped", `@dec\nexport class A extends Object {\n    @dec x = 1;\n    static {\n        @dec class B extends Object { @dec x = 1; static #p = 1; static readP() { return B.#p; } }\n        record(B.name);\n    }\n}\nrecord(A.name);\n`);
add("reserved", "outer-scoped-inner-file-level", `@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n    static {\n        @dec class A extends Object { @dec x = 1; }\n        record(A.name);\n    }\n}\nrecord(B.name);\n`);
add("reserved", "outer-scoped-inner-scoped", `@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n    static {\n        @dec class D extends Object { @dec x = 1; static #q = 1; static readQ() { return D.#q; } }\n        record(D.name);\n    }\n}\nrecord(B.name);\n`);
add("reserved", "private-accessor-storage-nested", `@dec\nexport class B {\n    @dec accessor #a = 1;\n    readA() { return this.#a; }\n    static {\n        @dec class D { @dec accessor #a = 2; readA() { return this.#a; } }\n        record(new D().readA());\n    }\n}\nrecord(new B().readA());\n`);
add("reserved", "private-accessor-storage-siblings", `@dec\nexport class B {\n    @dec accessor #a = 1;\n    readA() { return this.#a; }\n}\n@dec\nexport class D {\n    @dec accessor #a = 2;\n    readA() { return this.#a; }\n}\nrecord(new B().readA() + new D().readA());\n`);
add("reserved", "private-and-public-same-stem", `@dec\nexport class B {\n    @dec accessor #a = 1;\n    @dec accessor a = 2;\n    readA() { return this.#a + this.a; }\n}\nrecord(new B().readA());\n`);
add("reserved", "class-this-parsed-and-scoped", `let _classThis = 0;\nrecord(_classThis);\n@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n}\nrecord(B.name);\n`);

// ---------------------------------------------------------------- computed
add("computed", "static-accessor-decorated", `@dec\nexport class K {\n    @dec static accessor [keys.x] = 1;\n}\nrecord(K.name);\n`);
add("computed", "instance-accessor-decorated", `@dec\nexport class K {\n    @dec accessor [keys.x] = 1;\n}\nrecord(K.name);\n`);
add("computed", "method-decorated", `@dec\nexport class K {\n    @dec [key()]() { return 1; }\n}\nrecord(K.name);\n`);
add("computed", "static-method-decorated", `@dec\nexport class K {\n    @dec static [key()]() { return 1; }\n}\nrecord(K.name);\n`);
add("computed", "getter-setter-decorated", `@dec\nexport class K {\n    @dec get [keys.x]() { return 1; }\n    @dec set [keys.x](value: number) { record(value); }\n}\nrecord(K.name);\n`);
add("computed", "field-decorated", `@dec\nexport class K {\n    @dec [keys.x] = 1;\n}\nrecord(K.name);\n`);
add("computed", "static-field-decorated", COMPUTED_CLASS);
add("computed", "multiple-keys", `@dec\nexport class K {\n    @dec [keys.x] = 1;\n    @dec static [keys.y] = 2;\n}\nrecord(K.name);\n`);
add("computed", "user-identifier-_a", `let _a = 0;\nrecord(_a);\n` + `@dec\nexport class K {\n    @dec [keys.x] = 1;\n    @dec static [keys.y] = 2;\n}\nrecord(K.name);\n`);
add("computed", "undecorated-computed-between", `@dec\nexport class K {\n    @dec p = 1;\n    [keys.x] = 2;\n    @dec q = 3;\n}\nrecord(K.name);\n`);
add("computed", "undecorated-computed-method-after", `@dec\nexport class K {\n    @dec x = 1;\n    [key()]() { return 2; }\n}\nrecord(K.name);\n`);
add("computed", "class-expression-computed", `const E = @dec class { @dec static [keys.x] = 1; };\nrecord(E.name);\n`);
add("computed", "member-only-decorators-computed", `export class K {\n    @dec static [keys.x] = 1;\n    @dec [key2()]() { return 2; }\n}\nrecord(K.name);\n`);
add("computed", "computed-literal-and-expression", `@dec\nexport class K {\n    @dec ["lit"] = 1;\n    @dec [keys.x] = 2;\n}\nrecord(K.name);\n`);
add("computed", "computed-with-decorator-temp", `@dec\nexport class K {\n    @ns.dec static [keys.x] = 1;\n}\nrecord(K.name);\n`);
add("computed", "computed-in-static-block-nested-class", `export class K {\n    static {\n        @dec class Inner { @dec static [keys.x] = 1; }\n        record(Inner.name);\n    }\n}\n`);

// ---------------------------------------------------------------- ordering
add("ordering", "decorator-temp-and-anonymous-heritage", `@ns.dec\nexport class C extends (class { static v = 1; }) {\n    @dec x = 1;\n}\nrecord(C.name);\n`);
add("ordering", "member-decorator-temp-then-computed", `@dec\nexport class C {\n    @ns.dec static [keys.x] = 1;\n    @ns.dec y = 2;\n}\nrecord(C.name);\n`);
add("ordering", "pending-computed-around-constructor", `@dec\nexport class C extends Object {\n    @dec p = 1;\n    constructor() { super(); events.push("ctor"); }\n    [keys.x] = 2;\n    @dec q = 3;\n}\nrecord(new C().p);\n`);
add("ordering", "heritage-temp-and-member-temps", `@ns.dec\nexport class C extends (class { static v = 1; }) {\n    @ns.dec static [keys.x] = 1;\n    static { record(super.v); }\n}\nrecord(C.name);\n`);
add("ordering", "outer-this-and-computed-cache", `const holder = { dec, make() { return @dec class extends Object { @(this.dec) static [key()]() {} }; } };\nconst D = holder.make();\nrecord(D.name);\n`);
add("ordering", "two-classes-decorator-temps", `@ns.dec\nexport class C { @ns.dec x = 1; }\n@ns.dec\nexport class D { @ns.dec x = 1; }\nrecord(C.name + D.name);\n`);

// ---------------------------------------------------------------- lifecycle
// Bundle publication: writeBundle resets the printer's generatedNames once
// after the whole bundle (per-source parsed-identifier tables), so numbering
// and the file-wide set continue across sources.
const bundle = { module: 4, outFile: "/project/out/bundle.js", outDir: undefined, ignoreDeprecations: "6.0" };
add("lifecycle", "bundle-outer-this-two-files", OUTER_THIS_CLASS, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + OUTER_THIS_CLASS + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-class-expression-two-files", CLASS_EXPRESSION, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + CLASS_EXPRESSION + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-default-two-files", DEFAULT_EXPORT, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + DEFAULT_EXPORT + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-file-level-then-scoped-across-files", PLAIN_A, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + `@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n}\nrecord(B.name);\n` + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-parse-census-per-file", FULL_CLASS, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + `let _metadata = 0;\nrecord(_metadata);\n` + FULL_CLASS + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-scoped-names-two-files", FULL_CLASS, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + FULL_CLASS + TAIL }], compilerOptions: bundle });
add("lifecycle", "bundle-computed-temps-two-files", COMPUTED_CLASS, { extraFiles: [{ path: "/project/second.ts", text: PRELUDE + COMPUTED_CLASS + TAIL }], compilerOptions: bundle });
// CommonJS module publication of the same producers.
const cjs = { module: 1 };
add("lifecycle", "cjs-full-class", FULL_CLASS, { compilerOptions: cjs });
add("lifecycle", "cjs-default-export", DEFAULT_EXPORT, { compilerOptions: cjs });
add("lifecycle", "cjs-class-expression", CLASS_EXPRESSION, { compilerOptions: cjs });
add("lifecycle", "cjs-computed", COMPUTED_CLASS, { compilerOptions: cjs });
add("lifecycle", "cjs-global-script-_metadata", FULL_CLASS, { compilerOptions: cjs, extraFiles: [{ path: "/project/globals.ts", text: `let _metadata = 0;\n` }] });
add("lifecycle", "cjs-sibling-file-level-then-scoped", PLAIN_A + `@dec\nexport class B extends Object {\n    @dec x = 1;\n    static #p = 1;\n    static readP() { return B.#p; }\n}\nrecord(B.name);\n`, { compilerOptions: cjs });

const ids = new Set();
const cases = [];
for (const [targetName, target] of targets) {
  for (const [modeName, define] of modes) {
    for (const variant of variants) {
      const caseId = `decorator-binding/${variant.family}/${targetName}/${modeName}/${variant.variant}`;
      assert.ok(!ids.has(caseId), caseId);
      ids.add(caseId);
      const text = PRELUDE + variant.body + TAIL;
      const options = {
        strict: true, allowJs: false, checkJs: false, declaration: true, declarationMap: true, sourceMap: true,
        skipDefaultLibCheck: true, noErrorTruncation: true, newLine: 0, outDir: "/project/out", target, module: 99,
        useDefineForClassFields: define, removeComments: false, ...(variant.compilerOptions ?? {}),
      };
      for (const key of Object.keys(options)) if (options[key] === undefined) delete options[key];
      const files = [{ path: "/project/main.ts", text }, ...(variant.extraFiles ?? [])];
      const row = { case_id: caseId, family: variant.family, variant: variant.variant,
        roots: files.map(file => file.path), files, options };
      cases.push(row);
    }
  }
}
const families = {};
for (const variant of variants) (families[variant.family] ??= []).push(variant.variant);
const artifact = { version: 1,
  description: "A41-BINDING (C02) witnesses: generated-name collision domains of the standard-decorator transform — parsed-identifier census, checker globals, nested/sibling scopes, the FileLevel versus ReservedInNestedScopes `_classThis` domains, computed-key cache temps, name-generation ordering, and bundle/CommonJS publication",
  variants: variants.length, cases_per_variant: targets.length * modes.length, families, cases };
fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
console.log(JSON.stringify({ destination, variants: variants.length, cases: cases.length,
  families: Object.fromEntries(Object.entries(families).map(([k, v]) => [k, v.length])) }));
