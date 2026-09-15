// Expands the A6-41-SUPER witness families into a concrete input manifest.
// Every family/variant row of the handoff table becomes one case per option
// combination; the manifest fixes all IDs and the planned count before any
// observation runs.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const extra = process.argv.includes("--extra");
const followup = process.argv.includes("--followup");
const followup2 = process.argv.includes("--followup2");
const followup3 = process.argv.includes("--followup3");
assert.ok([extra, followup, followup2, followup3].filter(Boolean).length <= 1, "one manifest per invocation");
const destination = path.join(root, followup3
  ? "crates/compiler/tests/fixtures/decorator-super-followup3-inputs.json"
  : followup2
  ? "crates/compiler/tests/fixtures/decorator-super-followup2-inputs.json"
  : followup
  ? "crates/compiler/tests/fixtures/decorator-super-followup-inputs.json"
  : extra
  ? "crates/compiler/tests/fixtures/decorator-super-extra-inputs.json"
  : "crates/compiler/tests/fixtures/decorator-super-inputs.json");

const targets = [["es2015", 2], ["es2022", 9], ["esnext", 99]];
const modes = [["set", false], ["define", true]];

const PRELUDE = `const events: unknown[] = [];
function dec(value: any, context: any): any { return value; }
function key(): "x" { events.push("key"); return "x"; }
function mkey(): "m" { events.push("mkey"); return "m"; }
function tkey(): "t" { events.push("tkey"); return "t"; }
function rhs() { events.push("rhs"); return 3; }
function record(value: unknown) { events.push(value); }
`;
const BASE_HEAD = `class Base {
    static stored = 1;
    static get x() { events.push(["get", this.name]); return this.stored; }
    static set x(value: number) { events.push(["set", this.name, value]); this.stored = value; }
    static m(this: any, a: number) { events.push(["m", this.name, a]); return a; }
    static t(this: any, s: TemplateStringsArray, ...subs: unknown[]) { events.push(["t", this.name, s[0], subs]); return s; }
    im() { events.push("im"); return 0; }
`;
const BASE_EXTRA = {
  list: `    static list: any = null;\n`,
  big: `    static big: any = (globalThis as any).BigInt(1);\n`,
  falsy: `    static falsy: number = 0;\n    static get z() { events.push(["getz", this.name]); return this.falsy; }\n    static set z(value: number) { events.push(["setz", this.name, value]); this.falsy = value; }\n`,
};
function base(extras = []) {
  return BASE_HEAD + extras.map(name => BASE_EXTRA[name]).join("") + "}\n";
}
const TAIL = `record(Derived.stored);\nexport const tail = events.length;\n`;

// Each variant: [family, variant, class members (inside the decorated class),
// options]. Options: extras (Base members), header (class header override),
// prelude (extra prelude text), tail (override tail), compilerOptions.
const variants = [];
const add = (family, variant, members, options = {}) => variants.push({ family, variant, members, ...options });
const block = statements => `    static {\n${statements.map(s => `        ${s}`).join("\n")}\n    }\n`;

// read
add("read", "prop-static-block", block([`record(super.x);`]));
add("read", "elem-static-block", block([`record(super[key()]);`]));
add("read", "prop-static-field", `    static f = super.x;\n`);
add("read", "elem-static-field", `    static f = super[key()];\n`);
add("read", "getter-receiver-log", block([`record(super.x);`, `record(events.length);`]));

// call-tag
add("call-tag", "prop-call-discarded", block([`super.m(rhs());`]));
add("call-tag", "prop-call-used", block([`record(super.m(rhs()));`]));
add("call-tag", "elem-call-discarded", block([`super[mkey()](rhs());`]));
add("call-tag", "elem-call-used", block([`record(super[mkey()](rhs()));`]));
add("call-tag", "prop-tag-discarded", block([`super.t\`a\${rhs()}b\`;`]));
add("call-tag", "prop-tag-used", block([`record(super.t\`a\${rhs()}b\`);`]));
add("call-tag", "elem-tag-discarded", block([`super[tkey()]\`a\${rhs()}b\`;`]));
add("call-tag", "elem-tag-used", block([`record(super[tkey()]\`a\${rhs()}b\`);`]));
add("call-tag", "static-field-call", `    static f = super.m(rhs());\n`);

// assign
add("assign", "prop-discarded", block([`super.x = rhs();`]));
add("assign", "prop-used", block([`record(super.x = rhs());`]));
add("assign", "elem-discarded", block([`super[key()] = rhs();`]));
add("assign", "elem-used", block([`record(super[key()] = rhs());`]));
add("assign", "static-field-used", `    static f = super.x = rhs();\n`);
add("assign", "static-field-elem-used", `    static f = super[key()] = rhs();\n`);
add("assign", "chained-used", block([`record(super.x = super[key()] = rhs());`]));

// compound
add("compound", "prop-plus-discarded", block([`super.x += rhs();`]));
add("compound", "prop-plus-used", block([`record(super.x += rhs());`]));
add("compound", "elem-call-key-plus-discarded", block([`super[key()] += rhs();`]));
add("compound", "elem-call-key-plus-used", block([`record(super[key()] += rhs());`]));
add("compound", "elem-literal-key-minus-discarded", block([`super["x"] -= rhs();`]));
add("compound", "elem-literal-key-minus-used", block([`record(super["x"] -= rhs());`]));
add("compound", "elem-identifier-key-times-discarded", block([`const k = "x";`, `super[k] *= rhs();`]));
add("compound", "elem-identifier-key-times-used", block([`const k = "x";`, `record(super[k] *= rhs());`]));
add("compound", "prop-exponent-used", block([`record(super.x **= rhs());`]));
add("compound", "prop-bitwise-or-used", block([`record(super.x |= rhs());`]));
add("compound", "prop-shift-discarded", block([`super.x <<= rhs();`]));
add("compound", "static-field-plus", `    static f = super.x += rhs();\n`);

// logical
add("logical", "prop-or-discarded-truthy", block([`super.x ||= rhs();`]));
add("logical", "prop-or-used-truthy", block([`record(super.x ||= rhs());`]));
add("logical", "prop-or-discarded-falsy", block([`super.z ||= rhs();`]), { extras: ["falsy"] });
add("logical", "prop-and-discarded-truthy", block([`super.x &&= rhs();`]));
add("logical", "prop-and-used-falsy", block([`record(super.z &&= rhs());`]), { extras: ["falsy"] });
add("logical", "prop-nullish-used", block([`record(super.x ??= rhs());`]));
add("logical", "elem-nullish-discarded", block([`super[key()] ??= rhs();`]));
add("logical", "elem-or-used", block([`record(super[key()] ||= rhs());`]));

// update
add("update", "prop-postfix-discarded", block([`super.x++;`]));
add("update", "prop-postfix-used", block([`record(super.x++);`]));
add("update", "prop-prefix-used", block([`record(++super.x);`]));
add("update", "prop-prefix-discarded", block([`--super.x;`]));
add("update", "elem-postfix-used", block([`record(super[key()]++);`]));
add("update", "elem-prefix-discarded", block([`++super[key()];`]));
add("update", "elem-prefix-used", block([`record(--super[key()]);`]));
add("update", "paren-operand-used", block([`record((super.x)++);`]));
add("update", "bigint-postfix-used", block([`record(super.big++);`]), { extras: ["big"] });
add("update", "bigint-prefix-discarded", block([`--super.big;`]), { extras: ["big"] });
add("update", "static-field-postfix", `    static f = super.x++;\n`);

// discard
add("discard", "double-paren-assign", block([`((super.x = rhs()));`]));
add("discard", "comma-left-assign", block([`(super.x = rhs(), record(0));`]));
add("discard", "comma-right-assign-discarded", block([`(record(0), super.x = rhs());`]));
add("discard", "comma-right-assign-used", block([`record((record(0), super.x = rhs()));`]));
add("discard", "comma-nested-update", block([`(super.x++, super.x++, record(super.x++));`]));
add("discard", "for-initializer", block([`for (super.x = rhs(); false;) {}`]));
add("discard", "for-incrementor", block([`for (let i = 0; i < 1; i++, super.x++) {}`]));
add("discard", "for-condition-used", block([`for (; super.x++ < 3;) {}`]));
add("discard", "arrow-expression-body", block([`record((() => super.x++)());`]));
add("discard", "arrow-block-body", block([`(() => { super.x++; })();`]));
add("discard", "as-expression-discarded", block([`(super.x = rhs()) as number;`]));
add("discard", "satisfies-used", block([`record((super.x = rhs()) satisfies number);`]));
add("discard", "non-null-discarded", block([`(super.x = rhs())!;`]));

// array-target
add("array-target", "simple", block([`[super.x] = [rhs()];`]));
add("array-target", "default", block([`[super.x = rhs()] = [];`]));
add("array-target", "hole", block([`[, super.x] = [0, rhs()];`]));
add("array-target", "rest", block([`[...super.list] = [rhs()];`]), { extras: ["list"] });
add("array-target", "nested-array", block([`[[super.x]] = [[rhs()]];`]));
add("array-target", "effectful-element-key", block([`[super[key()]] = [rhs()];`]));
add("array-target", "object-in-array", block([`[{ y: super.x }] = [{ y: rhs() }];`]));
add("array-target", "used-value", block([`record([super.x] = [rhs()]);`]));

// object-target
add("object-target", "simple", block([`({ y: super.x } = { y: rhs() });`]));
add("object-target", "default", block([`({ y: super.x = rhs() } = obj());`]), { prelude: `function obj(): { y?: number } { events.push("obj"); return {}; }\n` });
add("object-target", "computed-key", block([`({ [key()]: super.x } = { x: rhs() });`]));
add("object-target", "rest", block([`({ ...super.list } = { a: rhs() });`]), { extras: ["list"] });
add("object-target", "nested-object", block([`({ y: { z: super.x } } = { y: { z: rhs() } });`]));
add("object-target", "nested-array", block([`({ y: [super.x] } = { y: [rhs()] });`]));
add("object-target", "elem-target", block([`({ y: super[key()] } = { y: rhs() });`]));
add("object-target", "used-value", block([`record({ y: super.x } = { y: rhs() });`]));

// receiver
add("receiver", "static-arrow", block([`const f = () => super.x;`, `record(f());`]));
add("receiver", "static-field-arrow", `    static f = () => super.x;\n    static { record(Derived.f()); }\n`);
add("receiver", "object-method-own-super", block([`const o = { m() { return super.toString; } };`, `record(typeof o.m());`]));
add("receiver", "function-own-this", block([`function f(this: any) { return this; }`, `record(f.call(Base) === Base);`, `record(super.x);`]));
add("receiver", "instance-field-super", `    y = super.im();\n`);
add("receiver", "instance-method-super", `    im2() { return super.im(); }\n    static { record(new Derived().im2()); }\n`);
add("receiver", "static-method-super", `    static sm() { return super.x; }\n    static { record(Derived.sm()); }\n`);
add("receiver", "nested-class-heritage", block([`class Inner extends (super.x, Base) {}`, `record(Inner.name);`]));
add("receiver", "nested-class-computed-name", block([`class Inner { [super.x]() { return 1; } }`, `record(Object.getOwnPropertyNames(Inner.prototype));`]));
add("receiver", "nested-class-body", block([`class Inner extends Base { static { record(super.x); } }`]));
add("receiver", "nested-decorated-class-body", block([`@dec class Inner extends Base { static { record(super.x); } }`]));
add("receiver", "nested-class-static-field", block([`class Inner extends Base { static f = super.x; }`, `record(Inner.f);`]));

// phase-order
add("phase-order", "decorator-and-heritage-nested", `    @dec static n() {}\n    [kk] = 1;\n    constructor() { super(); events.push("ctor"); }\n    static { record(super.x); }\n`, {
  header: `@wrap(@dec class A { static a = 1; })\nexport class Derived extends (@dec class B extends Base { static b = 2; }) {\n`,
  prelude: `function wrap(c: any) { events.push(["wrap", c.name]); return dec; }\nconst kk = "kk";\n`,
});
add("phase-order", "decorator-nested-private-static", `    static { record(super.x); }\n`, {
  header: `@wrap(@dec class A { static #p = 1; static a = A.#p; })\nclass Derived extends (@dec class B extends Base { static #q = 2; static b = B.#q; }) {\n`,
  prelude: `function wrap(c: any) { events.push(["wrap", c.name]); return dec; }\n`,
});
add("phase-order", "member-decorator-outer-this", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { return @dec class extends Base { @(this.dec) static n() {} static { record(super.x); } }; } };\nconst Derived = holder.make();\n`,
  wrapped: false,
});
add("phase-order", "member-decorator-pending-and-super", `    @dec static n() { return super.x; }\n    @dec static p = super.x;\n    static { record(super.x); }\n`);
add("phase-order", "constructor-nested-anonymous", `    constructor() { super(); const c = @dec class extends Base {}; events.push(c.name); }\n    static { const d = @dec class extends Base { static { record(super.x); } }; record(d.name); }\n    static { record(new Derived().constructor === Derived); }\n`);
add("phase-order", "computed-name-cache-and-super", `    static [kk] = super.x;\n    @dec static [mkey()]() {}\n    static { record(super.x); }\n`, { prelude: `const kk = "kk";\n` });

// handoff
add("handoff", "class-replacement", block([`record(super.x);`, `record(this === Derived);`]), {
  header: `function replace(value: any, context: any): any { return class extends value { static tag = "replaced"; }; }\n@replace\nexport class Derived extends Base {\n`,
});
add("handoff", "static-private-field", `    static #p = 1;\n    static { record(super.x + Derived.#p); }\n`);
add("handoff", "static-private-accessor", `    static accessor #q = 1;\n    static { record(super.x + Derived.#q); }\n`);
add("handoff", "static-public-accessor", `    static accessor q = super.x;\n    static { record(Derived.q); }\n`);
add("handoff", "member-only-decorator", `    @dec static n() {}\n    static { record(super.x); super.x = rhs(); super.x++; }\n`, { header: `export class Derived extends Base {\n` });
add("handoff", "undecorated-control", `    static { record(super.x); super.x = rhs(); super.x++; record(super[key()] += rhs()); }\n`, { header: `export class Derived extends Base {\n` });
add("handoff", "legacy-control", `    static { record(super.x); super.x = rhs(); super.x++; record(super[key()] += rhs()); }\n`, {
  header: `@legacy\nexport class Derived extends Base {\n`,
  prelude: `function legacy(target: any) { return target; }\n`,
  compilerOptions: { experimentalDecorators: true },
});
add("handoff", "export-default-anonymous", block([`record(super.x);`, `super.x = rhs();`]), {
  header: `@dec\nexport default class extends Base {\n`, tail: `export const tail = events.length;\n`,
});
add("handoff", "class-expression", `record(Derived.name);\n`, {
  header: `const Derived = @dec class extends Base { static { record(super.x); record(super.x = rhs()); } };\n`, wrapped: false,
});
add("handoff", "no-super-decorated-control", block([`record(this.name);`]), { header: `@dec\nexport class Derived {\n`, tail: `export const tail = events.length;\n` });

// fault
add("fault", "no-emit-on-error", block([`record(super.x);`, `super.x = "not a number";`]), { compilerOptions: { noEmitOnError: true } });
add("fault", "type-error-still-emits", block([`record(super.x);`, `super.x = "not a number";`]));
add("fault", "write-callback-failure", block([`record(super.x);`, `super.x = rhs();`]), { writeFailureIndex: 0 });

// Extra controls (separate fixture so the frozen 672-case set stays replayable
// by the binary built at the restored base): `_outerThis` versus the hoisted
// `var` of a computed-name cache temp, and the anonymous-heritage comma
// wrapping of `_classSuper` (transformClassLike safeExtendsExpression).
const extraVariants = [];
const addExtra = (family, variant, members, options = {}) => extraVariants.push({ family, variant, members, ...options });
addExtra("phase-order", "outer-this-and-cache-temp", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { return @dec class extends Base { @(this.dec) static [mkey()]() {} static { record(super.x); } }; } };\nconst Derived = holder.make();\n`,
  wrapped: false,
});
addExtra("handoff", "heritage-anonymous-class", block([`record(super.x);`, `super.x = rhs();`]), {
  header: `@dec\nexport class Derived extends class { static stored = 1; static get x() { events.push(["get", this.name]); return this.stored; } static set x(value: number) { events.push(["set", this.name, value]); this.stored = value; } } {\n`,
});
addExtra("handoff", "heritage-anonymous-function", block([`record(super.m2(rhs()));`]), {
  header: `@dec\nexport class Derived extends (function () {} as any as { new (): object; m2(a: number): number }) {\n`,
  prelude: `(Function.prototype as any).m2 = function (this: any, a: number) { events.push(["m2", a]); return a; };\n`,
  tail: `export const tail = events.length;\n`,
});
addExtra("handoff", "heritage-named-class", block([`record(super.x);`, `super.x = rhs();`]), {
  header: `@dec\nexport class Derived extends class Named { static stored = 1; static get x() { events.push(["get", this.name]); return this.stored; } static set x(value: number) { events.push(["set", this.name, value]); this.stored = value; } } {\n`,
});

// Pending-expression interleavings around decorated computed fields (tsc probe
// shapes: class-field lowering folds the field's key evaluation into the next
// computed name, or evaluates it before the class when none follows).
addExtra("phase-order", "decorated-field-then-undecorated-computed-method", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { return @dec class extends Base { @(this.dec) [kk] = 1; @(this.dec) n() {} @(this.dec) static p() {} static [mk]() { return super.x; } static { record(super.x); } }; } };\nconst Derived = holder.make();\n`,
  prelude: `const kk = "kk"; const mk = "mk";\n`, wrapped: false,
});
addExtra("phase-order", "decorated-field-then-decorated-computed-method", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { return @dec class extends Base { @(this.dec) [kk] = 1; @(this.dec) static [mk]() { return super.x; } static { record(super.x); } }; } };\nconst Derived = holder.make();\n`,
  prelude: `const kk = "kk"; const mk = "mk";\n`, wrapped: false,
});
addExtra("phase-order", "decorated-field-no-later-computed-name", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { return @dec class extends Base { @(this.dec) [kk] = 1; @(this.dec) static n() { return super.x; } static { record(super.x); } }; } };\nconst Derived = holder.make();\n`,
  prelude: `const kk = "kk";\n`, wrapped: false,
});


// Follow-up witnesses (2026-09-15, third fixture so the frozen primary and
// extra sets stay replayable by their earlier binaries):
//   param-default — a super update/compound inside an arrow parameter default
//     hoists its temp while the arrow's environment is in parameters
//     (visitParameterList: VariablesHoistedInParameters →
//     addDefaultValueAssignmentsIfNeeded moves the default into the body);
//   unicode-name — unicode-escaped identifier property/member/class names,
//     where createStringLiteralFromNode keeps the identifier's source spelling
//     for the Reflect key, the decorator context name/has key and __setFunctionName;
//   phase-order — the class-declaration shape of a decorated computed field
//     whose decorator uses lexical `this` with no later computed name.
const followupVariants = [];
const addFollowup = (family, variant, members, options = {}) => followupVariants.push({ family, variant, members, ...options });
const tailCall = expression => `record(${expression});\nrecord(Derived.stored);\nexport const tail = events.length;\n`;
addFollowup("param-default", "prop-postfix-used-expression-body", `    static f = (a = super.x++) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "prop-prefix-block-body", `    static f = (a = ++super.x) => { return a; };\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "elem-postfix-used", `    static f = (a = super[key()]++) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "compound-used", `    static f = (a = (super.x += rhs())) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "binding-pattern-default", `    static f = ({ a } = { a: super.x++ }) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "second-parameter-default", `    static f = (a: number, b = super.x++) => a + b;\n`, { tail: tailCall("Derived.f(1)") });
addFollowup("param-default", "static-block-iife", block([`record(((a = super.x++) => a)());`]));
addFollowup("param-default", "read-only-control", `    static f = (a = super.x) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "discarded-postfix-in-comma", `    static f = (a = (super.x++, rhs())) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("param-default", "nested-arrow-in-default", `    static f = (a = (() => super.x++)()) => a;\n`, { tail: tailCall("Derived.f()") });
addFollowup("unicode-name", "read", block([`record(super.\\u0078);`]));
addFollowup("unicode-name", "assign-discarded", block([`super.\\u0078 = rhs();`]));
addFollowup("unicode-name", "assign-used", block([`record(super.\\u0078 = rhs());`]));
addFollowup("unicode-name", "compound", block([`super.\\u0078 += rhs();`]));
addFollowup("unicode-name", "update-postfix-used", block([`record(super.\\u0078++);`]));
addFollowup("unicode-name", "call", block([`record(super.\\u006d(rhs()));`]));
addFollowup("unicode-name", "tag", block([`record(super.\\u0074\`s\`);`]));
addFollowup("unicode-name", "object-target", block([`({ a: super.\\u0078 } = { a: rhs() });`]));
addFollowup("unicode-name", "array-target", block([`[super.\\u0078] = [rhs()];`]));
addFollowup("unicode-name", "logical", block([`super.\\u0078 ??= rhs();`]));
addFollowup("unicode-name", "extended-escape-read", block([`record(super.\\u{78});`]));
addFollowup("unicode-name", "member-name-escape", `    @dec static \\u006d2() { return super.\\u0078; }\n` + block([`record(super.\\u0078);`]), { tail: tailCall("Derived.m2()") });
addFollowup("unicode-name", "class-name-escape", block([`record(super.\\u0078);`]), { header: `@dec\nexport class \\u0044erived extends Base {\n` });
addFollowup("unicode-name", "member-name-single-quoted", `    @dec static 'm 2'() { return super.\\u0078; }\n` + block([`record(super.\\u0078);`]), { tail: tailCall(`Derived["m 2"]()`) });
addFollowup("unicode-name", "member-name-numeric", `    @dec static 1() { return super.\\u0078; }\n` + block([`record(super.\\u0078);`]), { tail: tailCall("Derived[1]()") });
addFollowup("phase-order", "decorated-field-no-later-computed-name-declaration", `record(Derived.name);\n`, {
  header: `const holder = { dec, make() { @dec class Derived extends Base { @(this.dec) [kk] = 1; @(this.dec) static n() { return super.x; } static { record(super.x); } } return Derived; } };\nconst Derived = holder.make();\n`,
  prelude: `const kk = "kk";\n`, wrapped: false,
});

// Second follow-up (2026-09-15, fourth fixture): named evaluation of an
// anonymous decorated class expression in every source transformESDecorators
// handles (parameter default, binding element default, variable declaration,
// assignment / logical assignment, property assignment with identifier /
// string / numeric / computed names, export default) including escaped and
// parenthesized forms, and unicode-escaped private member names
// (createStringLiteralFromNode of a PrivateIdentifier, the printer's private
// name spelling).
const followup2Variants = [];
const addFollowup2 = (family, variant, members, options = {}) => followup2Variants.push({ family, variant, members, ...options });
const inner = `@dec class extends Base { static { record(super.x); } }`;
const innerMember = `class extends Base { @dec static m3() {} static { record(super.x); } }`;
addFollowup2("named-evaluation", "param-default-decorated-class", `    static f = (a = ${inner}) => a;\n`, { tail: tailCall("Derived.f().name") });
addFollowup2("named-evaluation", "param-default-member-decorated-class", `    static f = (a = ${innerMember}) => a;\n`, { tail: tailCall("Derived.f().name") });
addFollowup2("named-evaluation", "param-default-undecorated-control", `    static f = (a = class extends Base { static { record(super.x); } }) => a;\n`, { tail: tailCall("Derived.f().name") });
addFollowup2("named-evaluation", "param-default-escaped-name", `    static f = (\\u0061 = ${inner}) => a;\n`, { tail: tailCall("Derived.f().name") });
addFollowup2("named-evaluation", "param-default-hoist-and-named", `    static f = (a = super.x++, b = ${inner}) => [a, b.name];\n`, { tail: tailCall("Derived.f()") });
addFollowup2("named-evaluation", "param-default-parenthesized", `    static f = (a = (${inner})) => a;\n`, { tail: tailCall("Derived.f().name") });
addFollowup2("named-evaluation", "variable-plain-control", block([`const D2 = ${inner};`, `record(D2.name);`]));
addFollowup2("named-evaluation", "variable-escaped-name", block([`const \\u0044\\u0032 = ${inner};`, `record(D2.name);`]));
addFollowup2("named-evaluation", "variable-parenthesized", block([`const D2 = (${inner});`, `record(D2.name);`]));
addFollowup2("named-evaluation", "assignment-plain", block([`let D2: any;`, `D2 = ${inner};`, `record(D2.name);`]));
addFollowup2("named-evaluation", "assignment-escaped-name", block([`let D2: any;`, `\\u0044\\u0032 = ${inner};`, `record(D2.name);`]));
addFollowup2("named-evaluation", "assignment-logical", block([`let D2: any;`, `D2 ??= ${inner};`, `record(D2.name);`]));
addFollowup2("named-evaluation", "property-plain", block([`const holder = { p: ${inner} };`, `record(holder.p.name);`]));
addFollowup2("named-evaluation", "property-escaped-name", block([`const holder = { \\u0070: ${inner} };`, `record(holder.p.name);`]));
addFollowup2("named-evaluation", "property-string-name", block([`const holder = { 'p q': ${inner} };`, `record(holder["p q"].name);`]));
addFollowup2("named-evaluation", "property-numeric-name", block([`const holder = { 1: ${inner} };`, `record(holder[1].name);`]));
addFollowup2("named-evaluation", "property-computed-literal", block([`const holder = { ["p"]: ${inner} };`, `record(holder.p.name);`]));
addFollowup2("named-evaluation", "property-computed-expression", block([`const holder = { [key()]: ${inner} };`, `record(holder.x.name);`]));
addFollowup2("named-evaluation", "binding-element-default", block([`const { b = ${inner} } = {} as { b?: any };`, `record(b.name);`]));
addFollowup2("named-evaluation", "binding-element-escaped-name", block([`const { \\u0062 = ${inner} } = {} as { b?: any };`, `record(b.name);`]));
addFollowup2("named-evaluation", "export-default-expression", block([`record(super.x);`]), {
  tail: `record(Derived.stored);\nexport default (${inner});\nexport const tail = events.length;\n`,
});
addFollowup2("private-name", "private-method-escape", `    @dec static #\\u006d() { return super.\\u0078; }\n` + block([`record(this.#\\u006d());`]));
addFollowup2("private-name", "private-field-escape", `    @dec static #\\u0078 = 1;\n` + block([`record(this.#\\u0078);`, `record(super.x);`]));
addFollowup2("private-name", "private-getter-escape", `    @dec static get #\\u0067() { return super.\\u0078; }\n` + block([`record(this.#\\u0067);`]));
addFollowup2("private-name", "private-setter-escape", `    @dec static set #\\u0073(v: number) { super.\\u0078 = v; }\n` + block([`this.#\\u0073 = rhs();`]));
addFollowup2("private-name", "private-accessor-escape", `    @dec static accessor #\\u0061 = 1;\n` + block([`record(this.#\\u0061);`, `record(super.x);`]));
addFollowup2("private-name", "private-plain-control", `    @dec static #m() { return super.x; }\n` + block([`record(this.#m());`]));

// Third follow-up (2026-09-15, fifth fixture): emit-helper request order of
// the class-fields transform below ES2022. tsc drops relocated static blocks
// and static field initializers from the member pass and visits them after
// the members and the constructor (addPropertyOrClassStaticBlockStatements),
// so `__classPrivateFieldIn` / `__classPrivateFieldGet` requested there
// follow every helper a private method body or an instance initializer
// requests, while two relocated statics keep their own order. Plain and
// decorated classes; the decorated shapes carry an undecorated private
// method next to a decorated public one (the decoration block precedes it).
const followup3Variants = [];
const addFollowup3 = (family, variant, members, options = {}) => followup3Variants.push({ family, variant, members, ...options });
const plainHeader = `export class Derived extends Base {\n`;
addFollowup3("helper-order", "plain-static-in-then-method-get",
  block([`record(#x in this);`]) + `    static #m() { return this.#x; }\n    static #x = 1;\n` + block([`record(this.#m());`]), { header: plainHeader });
addFollowup3("helper-order", "plain-static-blocks-in-then-get",
  `    static #x = 1;\n` + block([`record(#x in this);`]) + block([`record(this.#x);`]), { header: plainHeader });
addFollowup3("helper-order", "plain-static-field-get-then-block-in",
  `    static #x = 1;\n    static f = this.#x;\n` + block([`record(#x in this);`]), { header: plainHeader, tail: tailCall("Derived.f") });
addFollowup3("helper-order", "plain-instance-get-then-static-in",
  `    static #x = 1;\n    y = Derived.#x;\n` + block([`record(#x in this);`]), { header: plainHeader, tail: tailCall("new Derived().y") });
addFollowup3("helper-order", "plain-nested-class-in-static-block",
  `    static #m() { return #x in this; }\n    static #x = 1;\n    static {\n        class Inner { static #y = 1; static #n() { return this.#y; } static { record(this.#n()); } }\n        record(Inner.name);\n        record(this.#m());\n    }\n`, { header: plainHeader });
addFollowup3("helper-order", "decorated-static-in-then-method-get",
  `    @dec static m() { return 1; }\n` + block([`record(#x in this);`]) + `    static #p() { return this.#x; }\n    static #x = 1;\n` + block([`record(this.#p());`]));
addFollowup3("helper-order", "decorated-instance-then-private-static-method",
  `    @dec static m() { return 1; }\n    y = 1;\n    static #p() { return 2; }\n` + block([`record(this.#p());`]), { tail: tailCall("new Derived().y") });
addFollowup3("helper-order", "decorated-instance-get-then-static-in",
  `    @dec static m() { return 1; }\n    static #x = 1;\n    y = Derived.#x;\n` + block([`record(#x in this);`]), { tail: tailCall("new Derived().y") });

const active = (followup3 ? followup3Variants : followup2 ? followup2Variants : followup ? followupVariants : extra ? extraVariants : variants).filter(v => !v.skip);
const ids = new Set();
const cases = [];
for (const [targetName, target] of targets) {
  for (const [modeName, define] of modes) {
    for (const variant of active) {
      const caseId = `${followup3 ? "decorator-super-followup3" : followup2 ? "decorator-super-followup2" : followup ? "decorator-super-followup" : extra ? "decorator-super-extra" : "decorator-super"}/${targetName}/${modeName}/${variant.family}/${variant.variant}`;
      assert.ok(!ids.has(caseId), caseId);
      ids.add(caseId);
      const header = variant.header ?? `@dec\nexport class Derived extends Base {\n`;
      const body = variant.wrapped === false
        ? header + variant.members
        : header + variant.members + `}\n`;
      const text = PRELUDE + (variant.prelude ?? "") + base(variant.extras ?? []) + body + (variant.tail ?? TAIL);
      const options = {
        strict: true, allowJs: false, checkJs: false, declaration: true, declarationMap: true, sourceMap: true,
        skipDefaultLibCheck: true, noErrorTruncation: true, newLine: 0, outDir: "/project/out", target, module: 99,
        useDefineForClassFields: define, removeComments: false, ...(variant.compilerOptions ?? {}),
      };
      const row = { case_id: caseId, family: variant.family, variant: variant.variant,
        roots: ["/project/main.ts"], files: [{ path: "/project/main.ts", text }], options };
      if (variant.writeFailureIndex !== undefined) row.write_failure_index = variant.writeFailureIndex;
      cases.push(row);
    }
  }
}
const families = {};
for (const variant of active) (families[variant.family] ??= []).push(variant.variant);
const artifact = { version: 1,
  description: followup3
    ? "A6-41-SUPER third follow-up: emit-helper request order of relocated static blocks and static field initializers below ES2022 (private in/get/set helpers) in plain and decorated classes"
    : followup2
    ? "A6-41-SUPER second follow-up: named evaluation of anonymous decorated class expressions in every transformESDecorators source (with escaped and parenthesized forms) and unicode-escaped private member names"
    : followup
    ? "A6-41-SUPER follow-up witnesses: arrow parameter defaults hoisting a super-update temp, unicode-escaped identifier names, and the class-declaration lexical-this computed-field shape"
    : extra
    ? "A6-41-SUPER extra controls: _outerThis versus hoisted cache temp ordering and anonymous/named heritage of a decorated class"
    : "A6-41-SUPER witnesses: standard-decorator static super read/call/tag/assign/compound/logical/update/discard/destructuring/receiver/phase-order/handoff/fault forms across target and useDefineForClassFields",
  variants: active.length, cases_per_variant: targets.length * modes.length, families, cases };
fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
console.log(JSON.stringify({ destination, variants: active.length, cases: cases.length, families: Object.fromEntries(Object.entries(families).map(([k, v]) => [k, v.length])) }));
