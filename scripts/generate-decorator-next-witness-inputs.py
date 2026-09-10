#!/usr/bin/env python3
"""Generates the H2.8a decorator-next witness input fixtures (diagnostic-free inputs).

Each group is written to crates/compiler/tests/fixtures/decorator-<group>-inputs.json
following the decorator-receiver-context-inputs.json schema: every source text is
crossed with ES2015/ES2022/ESNext x set/define exactly like that fixture.
"""
import json, sys, pathlib

ROOT = pathlib.Path(sys.argv[1])
TARGETS = [("es2015", 2), ("es2022", 9), ("esnext", 99)]
MODES = [("set", False), ("define", True)]
DEC = 'function dec(value: any, context: any): any { return value; }\n'
TAIL = 'export const tail = 1;\n'
KEYS = 'const keys = { a: "a", b: "b", c: "c", s: "s", x: "x" } as const;\n'
KEY = 'function key(): "k" { return "k"; }\n'

def L(*lines):
    return "\n".join(lines) + "\n"

def options(target, define):
    return {"strict": True, "allowJs": False, "checkJs": False, "declaration": True,
            "declarationMap": True, "sourceMap": True, "skipDefaultLibCheck": True,
            "noErrorTruncation": True, "newLine": 0, "outDir": "/project/out",
            "target": target, "module": 99, "useDefineForClassFields": define,
            "removeComments": False}

ORDER = {
 "pending-into-undecorated-names": DEC + KEYS + L(
    '@dec export class Box {',
    '    @dec method() {}',
    '    [keys.b]() { return this; }',
    '    @dec static sm() {}',
    '    get [keys.c]() { return 1; }',
    '    @dec field = 1;',
    '    [keys.a] = 2;',
    '    @dec static sf = 3;',
    '    static [keys.s] = 4;',
    '}') + TAIL,
 "decorated-computed-fields": DEC + KEYS + L(
    '@dec export class Box {',
    '    static k = "k";',
    '    @dec [keys.a] = 1;',
    '    [keys.b]() { return this; }',
    '    @dec static [keys.s] = 2;',
    '    @dec accessor [keys.x] = 3;',
    '    @dec method() {}',
    '    constructor() { }',
    '}') + TAIL,
 "heritage-safe-extends": DEC + L(
    'class Base { static base = 0; }',
    '@dec export class A extends (class { static base = 1; }) { static self = this; }',
    '@dec export class B extends (function () {} as any) { static self = this; }',
    '@dec export class C extends ((() => {}) as any) { static self = this; }',
    '@dec export class D extends (Base) { static self = this; }',
    '@dec export class E extends (class Named { static base = 2; }) { static self = this; }') + TAIL,
 "outer-this-static-block": DEC + L(
    'export function make(this: any): any {',
    '    @dec class Outer {',
    '        @((this as any).dec) method() {}',
    '        static inner = @dec class { @((this as any).dec) m() {} };',
    '    }',
    '    return Outer;',
    '}') + TAIL,
 "outer-this-computed-name": DEC + KEYS + L(
    'export function make(this: any): any {',
    '    @dec class Outer {',
    '        @((this as any).dec) method() {}',
    '        @((this as any).dec) static [keys.a] = 1;',
    '        @((this as any).dec) [keys.b]() {}',
    '    }',
    '    return Outer;',
    '}') + TAIL,
 "outer-this-nested-numbering": DEC + KEYS + L(
    'export function make(this: any): any {',
    '    @((this as any).reg.dec) class Outer extends (@dec class { @((this as any).reg.dec) h() {} }) {',
    '        @((this as any).reg.dec) m() {}',
    '        static inner = @dec class { @((this as any).reg.dec) i() {} };',
    '        static [keys.a] = @dec class { @((this as any).reg.dec) j() {} };',
    '    }',
    '    return Outer;',
    '}') + TAIL,
 "class-decorator-before-heritage": DEC + KEY + L(
    'export function make(this: any): any {',
    '    @((this as any).reg.dec(@dec class { @((this as any).reg.dec) [key()]() {} }))',
    '    class Box extends ((this as any).reg.base(@dec class { @((this as any).reg.dec) [key()]() {} })) {',
    '        @((this as any).reg.dec) [key()]() {}',
    '    }',
    '    return Box;',
    '}') + TAIL,
 "two-phase-constructor": DEC + KEYS + L(
    'const registry = { dec };',
    '@dec export class Box {',
    '    constructor(readonly x = @dec class { @(registry.dec) m() {} }) { this.y = this.x; }',
    '    y: any;',
    '    @(registry.dec) [keys.a] = 1;',
    '    @(registry.dec) static [keys.s]() {}',
    '    static self = this;',
    '    @(registry.dec) last() {}',
    '}') + TAIL,
 "nested-class-in-computed-name": DEC + L(
    'function k(value: any): "k" { return "k"; }',
    '@dec export class Outer {',
    '    @dec m() {}',
    '    static [k(@dec class { @dec n() {} })]() {}',
    '    get [k(@dec class { @dec o() {} })]() { return 1; }',
    '    @dec p() {}',
    '}') + TAIL,
 "decorated-class-expression-positions": DEC + KEY + L(
    'export const a = @dec class { @dec m() {} };',
    'export function f() { return @dec class { @dec [key()]() {} }; }',
    'export const o = { p: @dec class { @dec m() {} }, q() { return @dec class { @dec m() {} }; } };',
    'export default @dec class { @dec m() {} static self = this; };') + TAIL,
}

SUPER = {
 "super-read-call-tag": DEC + KEY + L(
    'class Base { static x: any = 1; static k: any = "k"; static m(...args: any[]): any { return args; } static tag(s: TemplateStringsArray, ...v: any[]): any { return v; } static [k: string]: any; }',
    '@dec export class Box extends Base {',
    '    static read: any = [super.x, super["x"], super[key()], super[this.k], super.m(1), super[key()](2), super.tag`t${3}`, super[key()]`u`];',
    '    static { super.x; super.m(super.x, super[key()]); }',
    '}') + TAIL,
 "super-assignment-used": DEC + KEY + L(
    'class Base { static x: any = 1; static [k: string]: any; }',
    '@dec export class Box extends Base {',
    '    static assign: any = [super.x = 1, super[key()] = 2, super.x += 3, super[key()] ??= 4, super.x ||= 5, super.x++, ++super.x, super[key()]--, --super[key()]];',
    '    static used: any = { a: super.x = 6, b: super[key()] += 7 };',
    '}') + TAIL,
 "super-assignment-discarded": DEC + KEY + L(
    'class Base { static x: any = 1; static [k: string]: any; }',
    '@dec export class Box extends Base {',
    '    static { super.x = 5; super[key()] = 6; super.x += 7; super[key()] ||= 8; super.x &&= 9; super.x++; --super[key()]; super[key()]++; }',
    '    static { for (super.x = 0; super.x < 2; super.x++) { } for (super[key()] = 0; super[key()] < 1; super[key()]++) { } (super.x, super.x = 1); (super.x += 1); }',
    '}') + TAIL,
 "super-destructuring": DEC + KEY + L(
    'class Base { static x: any = 1; static rest: any; static [k: string]: any; }',
    '@dec export class Box extends Base {',
    '    static destructure: any = [[super.x, super[key()], ...super.rest] = [1, 2, 3], { a: super.x, b: super[key()] = 9, ...super.rest } = { a: 1 }];',
    '    static { [super.x, [super[key()]]] = [1, [2]]; ({ c: super.x, d: super[key()] } = { c: 1, d: 2 }); [super.x = 3, ...super.rest] = [undefined]; }',
    '}') + TAIL,
 "super-lexical-boundaries": DEC + L(
    'class Base { static x: any = 1; get x(): any { return 2; } }',
    '@dec export class Box extends Base {',
    '    static lexical: any = [() => super.x, class Inner extends Base { static { super.x; } static i = super.x; }, (() => { super.x = 2; return () => super.x; })()];',
    '    instance: any = super.x;',
    '    method() { return super.x; }',
    '    static method() { return super.x; }',
    '    constructor() { super(); super.x; }',
    '    static { (() => { super.x = 3; })(); }',
    '}') + TAIL,
 "super-member-decorated-only": DEC + KEY + L(
    'class Base { static x: any = 1; static [k: string]: any; }',
    'export class Box extends Base {',
    '    @dec m() {}',
    '    static read: any = [super.x, super[key()], super.x = 2, super.x++];',
    '    static { super.x = 3; super[key()]++; }',
    '}') + TAIL,
 "super-undecorated-control": KEY + L(
    'class Base { static x: any = 1; static [k: string]: any; }',
    'export class Box extends Base {',
    '    static read: any = [super.x, super[key()], super.x = 2, super.x++];',
    '    static { super.x = 3; super[key()]++; }',
    '}') + TAIL,
}

NAMES = {
 "source-identifier-collisions": DEC + L(
    'const _classThis = 1, _metadata = 2, _classSuper = 3, _classDecorators = 4, _outerThis = 5, _a = 6;',
    'class Base {}',
    'export function make(this: any): any {',
    '    @dec class Box extends Base {',
    '        static #p = 1;',
    '        @((this as any).dec) m() {}',
    '        static inner = @dec class { static #q = 2; @((this as any).dec) n() {} static self = this; };',
    '        static self = this;',
    '    }',
    '    return Box;',
    '}') + TAIL,
 "sibling-classes-reuse": DEC + L(
    'export function make(this: any): any {',
    '    @dec class A { static #p = 1; @((this as any).dec) m() {} static self = this; }',
    '    @dec class B { static #q = 2; @((this as any).dec) n() {} static self = this; }',
    '    return [A, B];',
    '}') + TAIL,
 "computed-temp-shared-binding": DEC + KEY + KEYS + L(
    'const _a = 1;',
    '@dec export class Box {',
    '    @dec accessor [keys.a] = _a;',
    '    @dec [key()]() {}',
    '    static [keys.b] = @dec class {};',
    '}') + TAIL,
 "cross-file-global-names": DEC + KEY + L(
    'class Base {}',
    'export function make(this: any): any {',
    '    @dec class Box extends Base { @((this as any).reg.dec) m() {} @dec [key()]() {} static inner = @dec class { @((this as any).reg.dec) n() {} }; }',
    '    return Box;',
    '}') + TAIL,
}
CROSS = {"cross-file-global-names": [{"path": "/project/globals.ts",
    "text": "var _classThis = 1; var _metadata = 2; var _classSuper = 3; var _a = 4; var _outerThis = 5; var _m_decorators = 6; var class_1 = 7;\n"}]}

def build(group, table, extra_files=None):
    cases = []
    for name, text in table.items():
        for target_name, target in TARGETS:
            for mode_name, define in MODES:
                files = [{"path": "/project/main.ts", "text": text}]
                if extra_files and name in extra_files:
                    files = extra_files[name] + files
                cases.append({"case_id": f"decorator-{group}/{target_name}/esnext/{mode_name}/{name}",
                              "roots": [f["path"] for f in files], "files": files,
                              "options": options(target, define)})
    return cases

groups = {
  "transform-order": ("Decorator, heritage and member visit order: pending expression injection, outerThis, two-phase members and nested class state.", build("transform-order", ORDER)),
  "super-paths": ("Decorated-class super read/call/tag/assignment/update/destructuring paths in static initializers and static blocks.", build("super-paths", SUPER)),
  "name-owners": ("Generated-name ownership: FileLevel versus reserved names, sibling/nested reuse, cross-file globals and computed-name cache bindings.", build("name-owners", NAMES, CROSS)),
}
for group, (description, cases) in groups.items():
    path = ROOT / f"crates/compiler/tests/fixtures/decorator-{group}-inputs.json"
    path.write_text(json.dumps({"version": 1, "description": description, "cases": cases}, indent=2) + "\n")
    print(group, len(cases), path)
