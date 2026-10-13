#!/usr/bin/env python3
"""Generate the language server protocol's types for tsc-rs.

usage: generate_lsp_protocol.py [--profile <name>] [--check]

tsgo's protocol types (internal/lsp/lsproto/lsp_generated.go) are generated
upstream from the LSP meta model and tsgo's additions
(tools/scripts/lsp/generate.mts). This script reads the vendored copy and
writes the same types in Rust (crates/lsproto/src/generated.rs); --check
compares instead of writing. The output goes through rustfmt.

The Go file is the contract: its structs, their json tags (`omitzero` is an
optional field, `lsp:"required"` a required one, `lsp:"nullable"` an optional
field that may be null), its unions (one pointer per member) with the decoding
each union's `UnmarshalJSONFrom` performs, its literal types, its enumerations,
its methods and its request and notification tables. A construct this script
does not recognize is an error, so a change of tsgo's generator is noticed.

Encoding follows Go's: the fields in declaration order, optional fields
omitted when absent, a union as its member. Decoding follows tsgo's
`unmarshalStruct` and union decoders (crates/lsproto/src/codec.rs has the
shared rules and tsgo's error messages).
"""

import argparse
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROFILE = "7.1.0-dev-aa814927"
OUTPUT = ROOT / "crates/lsproto/src/generated.rs"
SOURCE = "upstream/tsc/internal/lsp/lsproto/lsp_generated.go"

# Go types the handwritten part of lsproto declares (lsp.go): their Rust
# names.
HANDWRITTEN = {
    "DocumentUri": "DocumentUri",
    "URI": "Uri",
    "Null": "Null",
    "NoParams": "NoParams",
    "Method": "Method",
}

PRIMITIVES = {
    "string": "String",
    "int32": "i32",
    "uint32": "u32",
    "float64": "f64",
    "bool": "bool",
    "any": "Value",
}

RUST_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
}


def fail(message):
    raise SystemExit(f"generate_lsp_protocol.py: {message}")


def snake(name):
    """`workDoneToken` -> `work_done_token`, `URI` -> `uri`."""
    out = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    out = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", out).lower()
    return f"r#{out}" if out in RUST_KEYWORDS else out


def screaming(name):
    out = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    out = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", out)
    out = out.replace(".", "_").replace("-", "_").replace("/", "_")
    out = out.upper()
    if out[0].isdigit():
        out = "_" + out
    return out


# ---------------------------------------------------------------------------
# Go types


class GoType:
    """A parsed Go type: kind in {named, prim, ptr, slice, map, array, empty}."""

    def __init__(self, kind, elem=None, name=None, key=None, size=None):
        self.kind, self.elem, self.name, self.key, self.size = kind, elem, name, key, size

    def __repr__(self):
        return f"GoType({self.kind}, {self.name or ''}, {self.elem!r})"


def parse_go_type(text):
    text = text.strip()
    if text.startswith("*"):
        return GoType("ptr", parse_go_type(text[1:]))
    if text.startswith("[]"):
        return GoType("slice", parse_go_type(text[2:]))
    match = re.match(r"\[(\d+)\](.*)", text)
    if match:
        return GoType("array", parse_go_type(match.group(2)), size=int(match.group(1)))
    if text.startswith("map["):
        depth, index = 0, 3
        while True:
            char = text[index]
            if char == "[":
                depth += 1
            elif char == "]":
                depth -= 1
                if depth == 0:
                    break
            index += 1
        return GoType("map", parse_go_type(text[index + 1:]), key=parse_go_type(text[4:index]))
    if text == "struct{}":
        return GoType("empty")
    if text in PRIMITIVES:
        return GoType("prim", name=text)
    if re.fullmatch(r"\w+", text):
        return GoType("named", name=text)
    fail(f"unsupported Go type {text!r}")


# ---------------------------------------------------------------------------
# Parsing the generated Go file


class Field:
    def __init__(self, go_name, go_type, json_name, omitzero, required, nullable, doc):
        self.go_name, self.go_type, self.json_name = go_name, go_type, json_name
        self.omitzero, self.required, self.nullable, self.doc = omitzero, required, nullable, doc


class Model:
    def __init__(self):
        self.structs = {}  # name -> (doc, [Field])
        self.order = []  # (kind, name) in source order
        self.unions = {}  # name -> (doc, [(member, GoType)])
        self.union_decoders = {}  # name -> strategy
        self.literals = {}  # name -> (doc, JSON text)
        self.enums = {}  # name -> (doc, base, [(const, member, value, doc)])
        self.aliases = {}  # name -> (doc, target text)
        self.methods = []  # (const name, method string, doc)
        self.requests = []  # (info name, params, response, method const, doc)
        self.notifications = []  # (info name, params, method const, doc)
        self.text_document_uri = []  # struct names
        self.text_document_position = []
        self.resolved = set()
        self.resolved_structs = {}  # name -> (doc, [Field])
        self.resolvers = []  # (source struct, resolved struct, [(go field, form)])
        self.registration_methods = []  # (member, method string)


def doc_lines(lines, index):
    """The `//` comment lines right above line `index`, without the slashes."""
    out = []
    i = index - 1
    while i >= 0 and lines[i].startswith("//"):
        out.append(lines[i][2:].removeprefix(" "))
        i -= 1
    return list(reversed(out))


def parse(source):
    model = Model()
    lines = source.split("\n")
    funcs = {}
    i = 0
    while i < len(lines):
        line = lines[i]
        m = re.match(r"^func \((\w) \*?(\w+)\) (\w+)\(", line)
        if m:
            end = i + 1
            while lines[end] != "}":
                end += 1
            funcs.setdefault(m.group(2), {})[m.group(3)] = (m.group(1), lines[i + 1:end])
            i = end + 1
            continue
        m = re.match(r"^type (\w+) struct \{$", line)
        if m:
            name = m.group(1)
            doc = doc_lines(lines, i)
            end = i + 1
            body = []
            while lines[end] != "}":
                body.append(end)
                end += 1
            model.structs[name] = (doc, [(j, lines[j]) for j in body])
            model.order.append(("struct", name))
            i = end + 1
            continue
        m = re.match(r"^type (\w+) struct\{\}$", line)
        if m:
            model.structs[m.group(1)] = (doc_lines(lines, i), [])
            model.order.append(("struct", m.group(1)))
            i += 1
            continue
        m = re.match(r"^type (\w+) = (\S+)$", line)
        if m:
            model.aliases[m.group(1)] = (doc_lines(lines, i), m.group(2))
            model.order.append(("alias", m.group(1)))
            i += 1
            continue
        m = re.match(r"^type (\w+) (string|int32|uint32)$", line)
        if m:
            model.enums[m.group(1)] = (doc_lines(lines, i), m.group(2), [])
            model.order.append(("enum", m.group(1)))
            i += 1
            continue
        if line == "const (":
            end = i + 1
            while lines[end] != ")":
                end += 1
            for j in range(i + 1, end):
                cm = re.match(r'^\t(\w+) +(\w+) += (.+)$', lines[j])
                if not cm:
                    continue
                const, typ, value = cm.groups()
                doc = []
                k = j - 1
                while lines[k].startswith("\t//"):
                    doc.append(lines[k][3:].removeprefix(" "))
                    k -= 1
                doc.reverse()
                if typ == "Method":
                    model.methods.append((const, value.strip('"'), doc))
                elif typ in model.enums:
                    member = const[len(typ):] if const.startswith(typ) else const
                    model.enums[typ][2].append((const, member, value, doc))
                else:
                    fail(f"constant {const} of unknown type {typ}")
            i = end + 1
            continue
        m = re.match(r"^var (\w+) = RequestInfo\[(\S+), (\S+)\]\{Method: (\w+)\}$", line)
        if m:
            model.requests.append((*m.groups(), doc_lines(lines, i)))
            i += 1
            continue
        m = re.match(r"^var (\w+) = NotificationInfo\[(\S+)\]\{Method: (\w+)\}$", line)
        if m:
            model.notifications.append((*m.groups(), doc_lines(lines, i)))
            i += 1
            continue
        i += 1

    # Classify the structs.
    for name, methods in funcs.items():
        for method, store in (("TextDocumentURI", model.text_document_uri),
                              ("TextDocumentPosition", model.text_document_position)):
            if method in methods:
                body = methods[method][1]
                am = re.fullmatch(r"\treturn s\.([\w.]+)", body[0]) if len(body) == 1 else None
                if not am:
                    fail(f"{name}.{method}: unexpected body {body!r}")
                store.append((name, am.group(1).split(".")))
        for method in ("resolve", "Resolve"):
            if method in methods:
                body = methods[method][1]
                forms = []
                for text in body:
                    fm = re.match(r"^\t\t(\w+): +(.+),$", text)
                    if not fm:
                        continue
                    value = fm.group(2)
                    if value == f"derefOr(v.{fm.group(1)})":
                        forms.append((fm.group(1), "deref"))
                    elif value == f"v.{fm.group(1)}.resolve()":
                        forms.append((fm.group(1), "resolve"))
                    elif value == f"v.{fm.group(1)}":
                        forms.append((fm.group(1), "copy"))
                    else:
                        fail(f"{name}.{method}: unexpected field {text!r}")
                model.resolvers.append((name, method, forms))
    plain = {}
    for name, (doc, body) in list(model.structs.items()):
        methods = funcs.get(name, {})
        marshal = methods.get("MarshalJSONTo")
        unmarshal = methods.get("UnmarshalJSONFrom")
        if name.startswith("Resolved"):
            fields = []
            for j, text in body:
                if not text.strip() or text.strip().startswith("//"):
                    continue
                fm = re.match(r'^\t(\w+) +(.+?) +`json:"([^"]*)"(?: lsp:"([^"]*)")?`$', text)
                if not fm:
                    fail(f"struct {name}: unexpected field line {text!r}")
                go_name, go_type, tag, lsp = fm.groups()
                fields.append(Field(go_name, parse_go_type(go_type), tag.split(",")[0], False, True,
                                    False, []))
            model.resolved_structs[name] = (doc, fields)
            continue
        if marshal and marshal[1][0].strip().startswith("return enc.WriteValue(json.Value(`"):
            literal = re.search(r"json\.Value\(`(.*)`\)\)", marshal[1][0]).group(1)
            model.literals[name] = (doc, literal)
            continue
        if marshal and marshal[1][0].strip().startswith("return marshalUnion("):
            members = []
            for _, text in body:
                fm = re.match(r"^\t(\w+) +(\S+)$", text)
                if not fm:
                    fail(f"union {name}: unexpected member line {text!r}")
                members.append((fm.group(1), parse_go_type(fm.group(2))))
            nullable = marshal[1][0].strip().endswith("true)")
            model.unions[name] = (doc, members, nullable)
            model.union_decoders[name] = parse_union_decoder(name, unmarshal[1], members)
            continue
        if name == "RegisterOptions":
            continue
        if name == "Registration":
            for text in marshal[1]:
                rm = re.match(r"^\tcase s\.RegisterOptions\.(\w+) != nil:$", text)
                if rm:
                    model.registration_methods.append([rm.group(1), None])
                rm = re.match(r'^\t\tmethod = json\.Value\(`"(.+)"`\)$', text)
                if rm:
                    model.registration_methods[-1][1] = rm.group(1)
            continue
        if unmarshal and unmarshal[1] != ["\treturn unmarshalStruct(s, dec)"]:
            fail(f"struct {name} has a custom decoder")
        fields = []
        for j, text in body:
            if not text.strip() or text.strip().startswith("//"):
                continue
            fm = re.match(r'^\t(\w+) +(.+?) +`json:"([^"]*)"(?: lsp:"([^"]*)")?`$', text)
            if not fm:
                fail(f"struct {name}: unexpected field line {text!r}")
            go_name, go_type, tag, lsp = fm.groups()
            parts = tag.split(",")
            markers = (lsp or "").split(",")
            doc = []
            k = j - 1
            while lines[k].startswith("\t//"):
                doc.append(lines[k][3:].removeprefix(" "))
                k -= 1
            doc.reverse()
            fields.append(Field(go_name, parse_go_type(go_type), parts[0], "omitzero" in parts[1:],
                                "required" in markers, "nullable" in markers, doc))
        plain[name] = (doc, fields)
    model.plain = plain
    if [k for k in model.structs if k == "RegisterOptions"]:
        register_options = model.structs["RegisterOptions"][1]
        model.register_options = []
        for _, text in register_options:
            fm = re.match(r"^\t(\w+) +\*(\w+)$", text)
            if fm:
                model.register_options.append((fm.group(1), fm.group(2)))
    return model


def parse_union_decoder(name, body, members):
    """The decoding of union `name` as a strategy tree (see emit_union_decode)."""
    text = "\n".join(body)
    member_names = {m for m, _ in members}

    def member_of(fragment):
        mm = re.search(r"o\.(\w+) = ", fragment)
        if not mm or mm.group(1) not in member_names:
            fail(f"union {name}: no member in {fragment!r}")
        return mm.group(1)

    def object_strategy(block):
        """The strategy for an object (or array) value inside `block`."""
        if "scanDiscriminatedStruct" in block:
            dm = re.search(r'scanDiscriminatedStruct\(dec, "(\w+)", "(\w+)"\)', block)
            arms = re.findall(r"case `(.+?)`:\n\s*return unmarshalDiscriminatedArm\(state, &o\.(\w+)\)", block)
            if not dm or not arms:
                fail(f"union {name}: unrecognized discriminator decoder")
            dflt = re.search(r"default:\n\s*return unmarshalDiscriminatedArm\(state, &o\.(\w+)\)", block)
            if not dflt and "return state.invalidDiscriminator()" not in block:
                fail(f"union {name}: unrecognized discriminator default")
            return ("discriminator", dm.group(1), dm.group(2), arms, dflt.group(1) if dflt else None)
        if "jsonObjectHasKey" in block:
            km = re.search(r"switch jsonObjectHasKey\(data, (.+?)\) \{", block)
            keys = re.findall(r'"([^"]*)"', km.group(1))
            cases = []
            for cm in re.finditer(r"case (\d+): // \w+\n\s*o\.(\w+) = ", block):
                cases.append((int(cm.group(1)), cm.group(2)))
            default = None
            after = block[block.index("switch jsonObjectHasKey"):]
            dm = re.search(r"default:\n\s*o\.(\w+) = new\(", after)
            if dm:
                default = dm.group(1)
            return ("haskey", keys, cases, default)
        trials = re.findall(r"if err := json\.Unmarshal\(data, &v(\w+)\); err == nil \{\n\s*o\.(\w+) = &v\1", block)
        if trials:
            return ("trial", [member for _, member in trials])
        mm = re.search(r"o\.(\w+) = new\(", block)
        if mm:
            return ("member", mm.group(1))
        fail(f"union {name}: unrecognized object decoding {block!r}")

    if "switch dec.PeekKind()" in text or "switch kind := dec.PeekKind(); kind" in text:
        start = text.index("switch")
        cases = []
        # Split the top-level switch into its cases (one tab of indent).
        parts = re.split(r"\n\tcase |\n\tdefault:", text[start:])
        labels = re.findall(r"\n\t(case [^\n]*:|default:)", text[start:])
        for label, block in zip(labels, parts[1:]):
            if label == "default:":
                continue
            kinds = re.findall(r"'(.)'", label)
            if "o." not in block and "ReadToken" in block:
                cases.append((kinds, ("null",)))
            elif "new(kind == 't')" in block:
                cases.append((kinds, ("bool", member_of(block))))
            elif "data, err := dec.ReadValue()" in block or "scanDiscriminatedStruct" in block:
                cases.append((kinds, object_strategy(block)))
            else:
                cases.append((kinds, ("member", member_of(block))))
        return ("kinds", cases)
    if name == "LSPAnyOrNull":
        return ("anyornull",)
    return ("value", object_strategy(text))


# ---------------------------------------------------------------------------
# Rust emission


class Emitter:
    def __init__(self, model):
        self.model = model
        self.out = []
        self.boxed = set()  # (container, field or member)

    def line(self, text=""):
        self.out.append(text)

    def doc(self, lines, indent=""):
        for text in lines:
            text = text.rstrip()
            self.line(f"{indent}///{(' ' + text) if text else ''}")

    # -- type mapping --------------------------------------------------------

    def named(self, name):
        if name in HANDWRITTEN:
            return HANDWRITTEN[name]
        return name

    def rust(self, go, container=None, member=None):
        """The Rust type of a Go value type (pointers are the caller's)."""
        if go.kind == "prim":
            return PRIMITIVES[go.name]
        if go.kind == "named":
            name = self.named(go.name)
            if (container, member) in self.boxed:
                return f"Box<{name}>"
            return name
        if go.kind == "ptr":
            return self.rust(go.elem, container, member)
        if go.kind == "slice":
            return f"Vec<{self.rust(go.elem)}>"
        if go.kind == "map":
            return f"BTreeMap<{self.rust(go.key)}, {self.rust(go.elem)}>"
        if go.kind == "array":
            return f"[{self.rust(go.elem)}; {go.size}]"
        if go.kind == "empty":
            return "EmptyObject"
        fail(f"no Rust type for {go!r}")

    def by_value_names(self, go):
        """The named types a Go type holds by value (not behind a slice/map)."""
        if go.kind == "named":
            return [go.name]
        if go.kind == "ptr":
            return self.by_value_names(go.elem)
        return []

    def compute_boxes(self):
        edges = {}
        for name, (_, fields) in self.model.plain.items():
            edges[name] = [(f.go_name, t) for f in fields for t in self.by_value_names(f.go_type)]
        for name, (_, members, _) in self.model.unions.items():
            edges[name] = [(m, t) for m, go in members for t in self.by_value_names(go)]
        for alias, (_, target) in self.model.aliases.items():
            edges.setdefault(alias, [("", target.lstrip("*"))])

        def reaches(start, goal):
            seen, stack = set(), [start]
            while stack:
                node = stack.pop()
                if node == goal:
                    return True
                if node in seen:
                    continue
                seen.add(node)
                stack.extend(t for _, t in edges.get(node, []))
            return False

        for name, items in edges.items():
            for member, target in items:
                if member and reaches(target, name):
                    self.boxed.add((name, member))

    # -- items ---------------------------------------------------------------

    def emit(self):
        self.compute_boxes()
        self.line("// @generated by scripts/generate_lsp_protocol.py from the vendored")
        self.line("// internal/lsp/lsproto/lsp_generated.go. Do not edit.")
        self.line()
        self.line("#![allow(clippy::all, clippy::pedantic, missing_docs, non_camel_case_types)]")
        self.line()
        self.line("use std::borrow::Cow;")
        self.line("use std::collections::BTreeMap;")
        self.line()
        self.line("use serde::Serialize;")
        self.line("use serde_json::Value;")
        self.line()
        self.line("use super::codec::*;")
        self.line("use super::{DocumentUri, EmptyObject, NoParams, Null, Uri};")
        for kind, name in self.model.order:
            if kind == "struct":
                if name in self.model.resolved_structs:
                    self.emit_resolved(name)
                elif name in self.model.plain:
                    self.emit_struct(name)
                elif name in self.model.unions:
                    self.emit_union(name)
                elif name in self.model.literals:
                    self.emit_literal(name)
                elif name == "Registration":
                    self.emit_registration()
            elif kind == "enum":
                self.emit_enum(name)
            elif kind == "alias":
                self.emit_alias(name)
        self.emit_methods()
        self.emit_requests()
        self.emit_accessors()
        self.emit_resolvers()
        return "\n".join(self.out) + "\n"

    def field_rust_type(self, owner, field):
        inner = self.rust(field.go_type, owner, field.go_name)
        if self.optional(field):
            return f"Option<{inner}>"
        return inner

    @staticmethod
    def optional(field):
        """An `omitzero` field that is nil when absent (a Go pointer, slice or
        map); an `omitzero` scalar is its zero value when absent instead."""
        return field.omitzero and field.go_type.kind in ("ptr", "slice", "map")

    def zero_omitted(self, field):
        """An `omitzero` scalar, omitted when it is its zero value."""
        if not field.omitzero or self.optional(field):
            return False
        go = field.go_type
        if go.kind == "prim" or (go.kind == "named" and go.name in self.model.enums):
            return True
        fail(f"omitzero field {field.go_name} of type {go!r} has no zero test")

    def emit_struct(self, name):
        doc, fields = self.model.plain[name]
        self.line()
        self.doc(doc)
        self.line("#[derive(Clone, Debug, Default, PartialEq, Serialize)]")
        self.line(f"pub struct {name} {{")
        for field in fields:
            self.doc(field.doc, "    ")
            attrs = [f'rename = "{field.json_name}"']
            if self.optional(field):
                attrs.append('skip_serializing_if = "Option::is_none"')
            elif self.zero_omitted(field):
                attrs.append('skip_serializing_if = "IsZero::is_zero"')
            self.line(f"    #[serde({', '.join(attrs)})]")
            self.line(f"    pub {snake(field.json_name)}: {self.field_rust_type(name, field)},")
        self.line("}")
        self.line()
        self.line(f"impl Decode for {name} {{")
        self.line("    fn decode(value: &Value) -> Result<Self, DecodeError> {")
        if not fields:
            self.line("        expect_object(value)?;")
            self.line("        Ok(Self {})")
            self.line("    }")
            self.line("}")
            return
        self.line("        let object = expect_object(value)?;")
        for field in fields:
            self.line(f"        let mut {snake(field.json_name).removeprefix('r#')}_ = None;")
        self.line("        for (name, value) in object {")
        self.line("            match name.as_str() {")
        for field in fields:
            var = snake(field.json_name).removeprefix("r#") + "_"
            decode = self.decode_expr(name, field, "value")
            nilable = field.go_type.kind in ("ptr", "slice", "map")
            if nilable and field.nullable:
                self.line(f'                "{field.json_name}" => {{')
                self.line(f"                    {var} = if value.is_null() {{ Some(None) }} else {{ Some(Some({decode})) }};")
                self.line("                }")
            elif nilable:
                self.line(f'                "{field.json_name}" => {{')
                self.line(f'                    reject_null("{field.json_name}", value)?;')
                self.line(f"                    {var} = Some(Some({decode}));")
                self.line("                }")
            else:
                self.line(f'                "{field.json_name}" => {var} = Some(Some({decode})),')
        self.line("                _ => {}")
        self.line("            }")
        self.line("        }")
        required = [f for f in fields if f.required]
        if required:
            checks = ", ".join(f'("{f.json_name}", {snake(f.json_name).removeprefix("r#")}_.is_some())' for f in required)
            self.line(f"        check_required(&[{checks}])?;")
        self.line("        Ok(Self {")
        for field in fields:
            var = snake(field.json_name).removeprefix("r#") + "_"
            if self.optional(field):
                self.line(f"            {snake(field.json_name)}: {var}.flatten(),")
            elif field.required:
                self.line(f"            {snake(field.json_name)}: {var}.flatten().unwrap_or_default(),")
            else:
                self.line(f"            {snake(field.json_name)}: {var}.flatten().unwrap_or_default(),")
        self.line("        })")
        self.line("    }")
        self.line("}")

    def decode_expr(self, owner, field, value):
        """Decoding `value` as the field's Rust type (without the Option)."""
        go = field.go_type
        inner = self.rust(go, owner, field.go_name)
        base = inner[4:-1] if inner.startswith("Box<") else inner
        expr = f"<{base} as Decode>::decode({value})?"
        if inner.startswith("Box<"):
            return f"Box::new({expr})"
        return expr

    def emit_union(self, name):
        doc, members, nullable = self.model.unions[name]
        self.line()
        self.doc(doc)
        self.line("#[derive(Clone, Debug, PartialEq)]")
        self.line(f"pub enum {name} {{")
        for member, go in members:
            self.line(f"    {member}({self.rust(go, name, member)}),")
        if nullable:
            self.line("    Null,")
        self.line("}")
        self.line()
        self.line(f"impl Default for {name} {{")
        self.line("    fn default() -> Self {")
        if nullable:
            self.line("        Self::Null")
        else:
            first, go = members[0]
            self.line(f"        Self::{first}(Default::default())")
        self.line("    }")
        self.line("}")
        self.line()
        self.line(f"impl Serialize for {name} {{")
        self.line("    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {")
        self.line("        match self {")
        for member, _ in members:
            self.line(f"            Self::{member}(value) => value.serialize(serializer),")
        if nullable:
            self.line("            Self::Null => serializer.serialize_unit(),")
        self.line("        }")
        self.line("    }")
        self.line("}")
        self.line()
        self.line(f"impl Decode for {name} {{")
        self.line("    fn decode(value: &Value) -> Result<Self, DecodeError> {")
        strategy = self.model.union_decoders[name]
        self.emit_union_decode(name, dict(members), strategy, "        ")
        self.line("    }")
        self.line("}")

    def member_decode(self, union, members, member, value="value"):
        go = members[member]
        inner = self.rust(go, union, member)
        if inner.startswith("Box<"):
            base = inner[4:-1]
            return f"Self::{member}(Box::new(<{base} as Decode>::decode({value})?))"
        return f"Self::{member}(<{inner} as Decode>::decode({value})?)"

    def emit_union_decode(self, union, members, strategy, indent):
        kind = strategy[0]
        if kind == "kinds":
            self.line(f"{indent}match json_kind(value) {{")
            for kinds, action in strategy[1]:
                pattern = " | ".join(f"'{k}'" if k != "'" else "'\\''" for k in kinds)
                if action[0] == "null":
                    self.line(f"{indent}    {pattern} => Ok(Self::Null),")
                elif action[0] == "bool":
                    self.line(f"{indent}    {pattern} => Ok(Self::{action[1]}(value.as_bool().unwrap_or_default())),")
                elif action[0] == "member":
                    self.line(f"{indent}    {pattern} => Ok({self.member_decode(union, members, action[1])}),")
                else:
                    self.line(f"{indent}    {pattern} => {{")
                    self.emit_object_decode(union, members, action, indent + "        ")
                    self.line(f"{indent}    }}")
            self.line(f'{indent}    kind => Err(err_invalid_kind("{union}", kind)),')
            self.line(f"{indent}}}")
        elif kind == "anyornull":
            self.line(f"{indent}if value.is_null() {{")
            self.line(f"{indent}    return Ok(Self::Null);")
            self.line(f"{indent}}}")
            self.line(f"{indent}Ok(Self::LSPAny(value.clone()))")
        elif kind == "value":
            self.emit_object_decode(union, members, strategy[1], indent)
        else:
            fail(f"union {union}: unknown strategy {kind}")

    def emit_object_decode(self, union, members, strategy, indent):
        kind = strategy[0]
        if kind == "member":
            self.line(f"{indent}Ok({self.member_decode(union, members, strategy[1])})")
        elif kind == "haskey":
            _, keys, cases, default = strategy
            keys_text = ", ".join(f'"{k}"' for k in keys)
            self.line(f"{indent}match json_object_has_key(value, &[{keys_text}]) {{")
            for index, member in cases:
                self.line(f"{indent}    Some({index}) => Ok({self.member_decode(union, members, member)}),")
            if default:
                self.line(f"{indent}    _ => Ok({self.member_decode(union, members, default)}),")
            else:
                self.line(f'{indent}    _ => Err(err_invalid_value("{union}", value)),')
            self.line(f"{indent}}}")
        elif kind == "trial":
            for member in strategy[1]:
                go = members[member]
                inner = self.rust(go, union, member)
                base = inner[4:-1] if inner.startswith("Box<") else inner
                wrap = "Box::new(decoded)" if inner.startswith("Box<") else "decoded"
                self.line(f"{indent}if let Ok(decoded) = <{base} as Decode>::decode(value) {{")
                self.line(f"{indent}    return Ok(Self::{member}({wrap}));")
                self.line(f"{indent}}}")
            self.line(f'{indent}Err(err_invalid_value("{union}", value))')
        elif kind == "discriminator":
            _, type_name, field, arms, default = strategy
            self.line(f'{indent}let discriminator = discriminator_of(value, "{field}")?;')
            self.line(f"{indent}match discriminator.as_deref() {{")
            for raw, member in arms:
                escaped = raw.replace("\\", "\\\\").replace('"', '\\"')
                self.line(f'{indent}    Some("{escaped}") => Ok({self.member_decode(union, members, member)}),')
            if default:
                self.line(f"{indent}    _ => Ok({self.member_decode(union, members, default)}),")
            else:
                self.line(f'{indent}    other => Err(err_invalid_discriminator("{type_name}", "{field}", other)),')
            self.line(f"{indent}}}")
        else:
            fail(f"union {union}: unknown object strategy {kind}")

    def emit_literal(self, name):
        doc, literal = self.model.literals[name]
        self.line()
        self.doc(doc)
        self.line("#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]")
        self.line(f"pub struct {name};")
        self.line()
        self.line(f"impl {name} {{")
        escaped = literal.replace("\\", "\\\\").replace('"', '\\"')
        self.line(f'    pub const JSON: &\'static str = "{escaped}";')
        self.line("}")
        self.line()
        self.line(f"impl Serialize for {name} {{")
        self.line("    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {")
        self.line("        literal_serialize(Self::JSON, serializer)")
        self.line("    }")
        self.line("}")
        self.line()
        self.line(f"impl Decode for {name} {{")
        self.line("    fn decode(value: &Value) -> Result<Self, DecodeError> {")
        self.line(f'        expect_literal(value, "{name}", Self::JSON)?;')
        self.line("        Ok(Self)")
        self.line("    }")
        self.line("}")

    def emit_enum(self, name):
        doc, base, consts = self.model.enums[name]
        self.line()
        self.doc(doc)
        if base == "string":
            self.line("#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]")
            self.line("#[serde(transparent)]")
            self.line(f"pub struct {name}(pub Cow<'static, str>);")
        else:
            rust = PRIMITIVES[base]
            self.line("#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]")
            self.line("#[serde(transparent)]")
            self.line(f"pub struct {name}(pub {rust});")
        self.line()
        self.line(f"impl {name} {{")
        seen = set()
        for const, member, value, cdoc in consts:
            ident = screaming(member)
            if ident in seen:
                ident = screaming(const)
            seen.add(ident)
            self.doc(cdoc, "    ")
            if base == "string":
                self.line(f"    pub const {ident}: Self = Self(Cow::Borrowed({value}));")
            else:
                self.line(f"    pub const {ident}: Self = Self({value});")
        if base != "string":
            self.line()
            self.line("    /// tsgo's `String()`: the member's name, if the value is one.")
            self.line("    pub fn name(self) -> Option<&'static str> {")
            self.line("        match self.0 {")
            names = {}
            for const, member, value, _ in consts:
                names.setdefault(value, member)
            for value, member in names.items():
                self.line(f'            {value} => Some("{member}"),')
            self.line("            _ => None,")
            self.line("        }")
            self.line("    }")
        self.line("}")
        if base != "string":
            self.line()
            self.line(f"/// tsgo's `String()` (stringer): the member's name, else `{name}(value)`.")
            self.line(f"impl std::fmt::Display for {name} {{")
            self.line("    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {")
            self.line("        match self.name() {")
            self.line("            Some(name) => formatter.write_str(name),")
            self.line(f'            None => write!(formatter, "{name}({{}})", self.0),')
            self.line("        }")
            self.line("    }")
            self.line("}")
        self.line()
        self.line(f"impl IsZero for {name} {{")
        self.line("    fn is_zero(&self) -> bool {")
        self.line("        self.0.is_zero()")
        self.line("    }")
        self.line("}")
        self.line()
        self.line(f"impl Decode for {name} {{")
        self.line("    fn decode(value: &Value) -> Result<Self, DecodeError> {")
        if base == "string":
            self.line("        Ok(Self(Cow::Owned(<String as Decode>::decode(value)?)))")
        else:
            self.line(f"        Ok(Self(<{PRIMITIVES[base]} as Decode>::decode(value)?))")
        self.line("    }")
        self.line("}")

    def emit_alias(self, name):
        doc, target = self.model.aliases[name]
        self.line()
        self.doc(doc)
        self.line(f"pub type {name} = {self.rust(parse_go_type(target))};")

    def emit_registration(self):
        options = dict(self.model.register_options)
        self.line()
        self.line("/// General parameters to register for a notification or to register a provider.")
        self.line("#[derive(Clone, Debug, Default, PartialEq)]")
        self.line("pub struct Registration {")
        self.line("    /// The id used to register the request. The id can be used to deregister")
        self.line("    /// the request again.")
        self.line("    pub id: String,")
        self.line("    /// Options necessary for the registration. Determines the method.")
        self.line("    pub register_options: Option<RegisterOptions>,")
        self.line("}")
        self.line()
        self.line("/// The options of a capability registration; the variant determines the method.")
        self.line("#[derive(Clone, Debug, PartialEq)]")
        self.line("pub enum RegisterOptions {")
        for member, _ in self.model.registration_methods:
            self.line(f"    {member}({options[member]}),")
        self.line("}")
        self.line()
        self.line("impl RegisterOptions {")
        self.line("    /// The method the options register.")
        self.line("    pub fn method(&self) -> &'static str {")
        self.line("        match self {")
        for member, method in self.model.registration_methods:
            self.line(f'            Self::{member}(_) => "{method}",')
        self.line("        }")
        self.line("    }")
        self.line()
        self.line("    fn decode_for(method: &str, value: &Value) -> Result<Self, DecodeError> {")
        self.line("        Ok(match method {")
        for member, method in self.model.registration_methods:
            self.line(f'            "{method}" => Self::{member}(<{options[member]} as Decode>::decode(value)?),')
        self.line('            _ => return Err(DecodeError(format!("unknown registration method: {method}"))),')
        self.line("        })")
        self.line("    }")
        self.line("}")
        self.line()
        self.line("impl Serialize for Registration {")
        self.line("    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {")
        self.line("        use serde::ser::{Error, SerializeStruct};")
        self.line("        let options = self")
        self.line("            .register_options")
        self.line("            .as_ref()")
        self.line('            .ok_or_else(|| S::Error::custom("RegisterOptions must be set"))?;')
        self.line('        let mut state = serializer.serialize_struct("Registration", 3)?;')
        self.line('        state.serialize_field("id", &self.id)?;')
        self.line('        state.serialize_field("method", options.method())?;')
        self.line("        match options {")
        for member, _ in self.model.registration_methods:
            self.line(f'            RegisterOptions::{member}(value) => state.serialize_field("registerOptions", value)?,')
        self.line("        }")
        self.line("        state.end()")
        self.line("    }")
        self.line("}")
        self.line()
        self.line("impl Decode for Registration {")
        self.line("    fn decode(value: &Value) -> Result<Self, DecodeError> {")
        self.line("        let object = expect_object(value)?;")
        self.line("        let mut id = None;")
        self.line("        let mut method = None;")
        self.line("        let mut register_options = None;")
        self.line("        for (name, value) in object {")
        self.line("            match name.as_str() {")
        self.line('                "id" => id = Some(<String as Decode>::decode(value)?),')
        self.line('                "method" => method = Some(<String as Decode>::decode(value)?),')
        self.line('                "registerOptions" => register_options = Some(value),')
        self.line("                _ => {}")
        self.line("            }")
        self.line("        }")
        self.line('        check_required(&[("id", id.is_some()), ("method", method.is_some())])?;')
        self.line("        let method = method.unwrap_or_default();")
        self.line("        let Some(options) = register_options else {")
        self.line('            return Err(DecodeError(format!("missing registerOptions for method: {method}")));')
        self.line("        };")
        self.line("        Ok(Self {")
        self.line("            id: id.unwrap_or_default(),")
        self.line("            register_options: Some(RegisterOptions::decode_for(&method, options)?),")
        self.line("        })")
        self.line("    }")
        self.line("}")

    def resolved_union(self, field):
        """A resolved union field: Go's zero union (no member) is `None`."""
        return field.go_type.kind == "named" and field.go_type.name in self.model.unions

    def emit_resolved(self, name):
        """A resolved struct, written as tsgo's `omitzero` fields."""
        doc, fields = self.model.resolved_structs[name]
        self.line()
        self.doc(doc)
        self.line("#[derive(Clone, Debug, Default, PartialEq, Serialize)]")
        self.line(f"pub struct {name} {{")
        for field in fields:
            rust = self.field_rust_type(name, field)
            if self.resolved_union(field):
                rust = f"Option<{rust}>"
            self.line(f'    #[serde(rename = "{field.json_name}", skip_serializing_if = "IsZero::is_zero")]')
            self.line(f"    pub {snake(field.json_name)}: {rust},")
        self.line("}")
        self.line()
        self.line(f"impl IsZero for {name} {{")
        self.line("    fn is_zero(&self) -> bool {")
        self.line("        *self == Self::default()")
        self.line("    }")
        self.line("}")

    def emit_resolvers(self):
        for source, method, forms in self.model.resolvers:
            target = f"Resolved{source}"
            if target not in self.model.resolved_structs:
                fail(f"{source}.{method}: no {target}")
            source_fields = {f.go_name: f for f in self.model.plain[source][1]}
            target_fields = {f.go_name: f for f in self.model.resolved_structs[target][1]}
            self.line()
            self.line(f"impl {source} {{")
            self.line(f"    /// tsgo `{method}`: the capabilities with their defaults filled in.")
            self.line(f"    pub fn resolve(&self) -> {target} {{")
            self.line(f"        {target} {{")
            for go_name, form in forms:
                field = source_fields[go_name]
                name = snake(target_fields[go_name].json_name)
                member = f"self.{snake(field.json_name)}"
                if form == "deref" and self.resolved_union(target_fields[go_name]):
                    self.line(f"            {name}: {member}.clone(),")
                elif form == "deref":
                    self.line(f"            {name}: {member}.clone().unwrap_or_default(),")
                elif form == "resolve":
                    go = field.go_type
                    while go.kind == "ptr":
                        go = go.elem
                    if self.optional(field):
                        self.line(f"            {name}: {member}.as_ref().map({go.name}::resolve).unwrap_or_default(),")
                    else:
                        self.line(f"            {name}: {member}.resolve(),")
                else:
                    self.line(f"            {name}: {member}.clone(),")
            self.line("        }")
            self.line("    }")
            self.line("}")

    def emit_methods(self):
        self.line()
        self.line("/// The protocol's method names (tsgo's `Method` constants).")
        self.line("pub mod method {")
        for const, value, doc in self.model.methods:
            self.doc(doc, "    ")
            self.line(f'    pub const {screaming(const.removeprefix("Method"))}: &str = "{value}";')
        self.line("}")

    def emit_requests(self):
        methods = {const: value for const, value, _ in self.model.methods}
        self.line()
        self.line("/// tsgo's `RequestInfo`: a request's method, parameters and response.")
        self.line("pub trait Request {")
        self.line("    const METHOD: &'static str;")
        self.line("    type Params: Decode + Serialize;")
        self.line("    type Response: Decode + Serialize;")
        self.line("}")
        self.line()
        self.line("/// tsgo's `NotificationInfo`: a notification's method and parameters.")
        self.line("pub trait Notification {")
        self.line("    const METHOD: &'static str;")
        self.line("    type Params: Decode + Serialize;")
        self.line("}")
        for info, params, response, const, doc in self.model.requests:
            name = info
            self.line()
            self.doc(doc)
            self.line(f"pub struct {name};")
            self.line()
            self.line(f"impl Request for {name} {{")
            self.line(f'    const METHOD: &\'static str = "{methods[const]}";')
            self.line(f"    type Params = {self.rust(parse_go_type(params))};")
            self.line(f"    type Response = {self.rust(parse_go_type(response))};")
            self.line("}")
        for info, params, const, doc in self.model.notifications:
            name = info
            self.line()
            self.doc(doc)
            self.line(f"pub struct {name};")
            self.line()
            self.line(f"impl Notification for {name} {{")
            self.line(f'    const METHOD: &\'static str = "{methods[const]}";')
            self.line(f"    type Params = {self.rust(parse_go_type(params))};")
            self.line("}")

    def emit_accessors(self):
        self.line()
        self.line("/// tsgo's `TextDocumentURI()`: the document a request is about.")
        self.line("pub trait HasTextDocumentUri {")
        self.line("    fn text_document_uri(&self) -> &DocumentUri;")
        self.line("}")
        for name, path in sorted(self.model.text_document_uri):
            self.line()
            self.line(f"impl HasTextDocumentUri for {name} {{")
            self.line("    fn text_document_uri(&self) -> &DocumentUri {")
            self.line(f"        &self.{self.rust_path(name, path)}")
            self.line("    }")
            self.line("}")
        self.line()
        self.line("/// tsgo's `TextDocumentPosition()`: the position a request is about.")
        self.line("pub trait HasTextDocumentPosition {")
        self.line("    fn text_document_position(&self) -> Position;")
        self.line("}")
        for name, path in sorted(self.model.text_document_position):
            self.line()
            self.line(f"impl HasTextDocumentPosition for {name} {{")
            self.line("    fn text_document_position(&self) -> Position {")
            self.line(f"        self.{self.rust_path(name, path)}.clone()")
            self.line("    }")
            self.line("}")

    def rust_path(self, struct, path):
        """A Go field path (`TextDocument.Uri`) as the Rust field path."""
        out = []
        for go_name in path:
            fields = self.model.plain[struct][1]
            field = next((f for f in fields if f.go_name == go_name), None)
            if field is None:
                fail(f"{struct} has no field {go_name}")
            out.append(snake(field.json_name))
            go = field.go_type
            while go.kind == "ptr":
                go = go.elem
            struct = go.name
        return ".".join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    source_path = ROOT / "vendor/typescript-native" / args.profile / SOURCE
    model = parse(source_path.read_text())
    # rustfmt's layout, so that `cargo fmt` keeps the file as generated.
    output = subprocess.run(["rustfmt", "--edition", "2021", "--emit", "stdout"],
                            input=Emitter(model).emit(), capture_output=True, text=True, check=True).stdout
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != output:
            print(f"{OUTPUT.relative_to(ROOT)} is out of date; run scripts/generate_lsp_protocol.py",
                  file=sys.stderr)
            return 1
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(output)
    print(f"wrote {OUTPUT.relative_to(ROOT)}: {len(model.plain)} structs, {len(model.unions)} unions, "
          f"{len(model.literals)} literals, {len(model.enums)} enumerations, {len(model.requests)} requests, "
          f"{len(model.notifications)} notifications")
    return 0


if __name__ == "__main__":
    sys.exit(main())
