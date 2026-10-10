#!/usr/bin/env python3
"""Generate the API encoder's and decoder's kind and child tables for tsc-rs.

usage: generate_api_encoder.py [--profile <name>] [--check]

tsgo's API encodes a source file in its own kind numbers, with each node's
children in tsgo's visitor order (internal/api/encoder). This script reads
the vendored copies of tsgo's kind enum (internal/ast/kind_generated.go) and
of the encoder's generated tables (internal/api/encoder/encoder_generated.go:
getNodeDataType and getChildrenPropertyMask, whose bits list each kind's
child properties in visitor order), maps them onto tsc-rs's syntax tree
(crates/syntax/src/kind.rs and nodes.rs) and writes
crates/api/src/encoder/generated.rs and crates/api/src/decoder/generated.rs
(the same tables read the other way); --check compares instead of writing.

A tsgo property maps to the tsc-rs field of the same name in snake case
unless FIELD_RENAMES says otherwise. A property tsc-rs's tree does not have
must be listed in ABSENT (tsgo's JavaScript reparser or 6.0.3-era shapes
produce it), and a child field of a tsc-rs node that tsgo's node does not
have must be listed in PORT_ONLY; anything else is an error, so a change on
either side is noticed.
"""

import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROFILE = "7.1.0-dev-19dadef8"
OUTPUT = ROOT / "crates/api/src/encoder/generated.rs"
DECODER_OUTPUT = ROOT / "crates/api/src/decoder/generated.rs"

# tsc-rs kinds whose tsgo kind has another name.
KIND_RENAMES = {"EndOfFileToken": "EndOfFile", "JSDocTag": "JSDocUnknownTag"}

# (tsgo kind, tsgo property) -> tsc-rs field, where the names differ.
FIELD_RENAMES = {
    ("TypeParameter", "DefaultType"): "r#default",
    ("ImportAttributes", "Attributes"): "elements",
    ("JSDocAugmentsTag", "ClassName"): "class",
    ("JSDocImplementsTag", "ClassName"): "class",
    ("JSDocSeeTag", "NameExpression"): "name",
    # tsc's fullName is the visited name (the namespace chain or the
    # identifier); its `name` is the innermost identifier inside it.
    ("JSDocTypedefTag", "Name"): "full_name",
    ("JSDocCallbackTag", "Name"): "full_name",
}

# tsgo's single `?`/`!` token after a name; tsc keeps questionToken and
# exclamationToken apart (at most one is set).
POSTFIX_KINDS = {
    "MethodDeclaration": ("question_token", "exclamation_token"),
    "MethodSignature": ("question_token",),
    "PropertyAssignment": ("question_token", "exclamation_token"),
    "PropertyDeclaration": ("question_token", "exclamation_token"),
    "PropertySignature": ("question_token",),
    "ShorthandPropertyAssignment": ("question_token", "exclamation_token"),
}

# tsgo properties tsc-rs's tree does not have: tsgo's JavaScript reparser
# fills them (a reparsed type or modifiers), or tsgo merged two node types.
ABSENT = {
    ("BinaryExpression", "Modifiers"),
    ("BinaryExpression", "Type"),
    ("ExportAssignment", "Type"),
    ("PropertyAssignment", "Type"),
    ("ShorthandPropertyAssignment", "Type"),
    ("ForInStatement", "AwaitModifier"),
    ("DefaultClause", "Expression"),
    # tsgo's services build SyntaxLists; the parser never does.
    ("SyntaxList", "Children"),
}

# Child fields of tsc-rs nodes that tsgo's node does not have (6.0.3-era
# grammar-error carriers); they are not encoded.
PORT_ONLY = {
    ("Constructor", "name"),
    ("FunctionType", "modifiers"),
    ("IndexSignature", "type_parameters"),
    ("NewExpression", "question_dot_token"),
    # tsc's innermost name of fullName (the same node, reached through it).
    ("JSDocTypedefTag", "name"),
    ("JSDocCallbackTag", "name"),
}

# tsgo children kept in a plain slice, not a NodeList: each element is a
# direct child, and the mask bit says whether there is any.
SLICES = {("JSDocTypeLiteral", "JSDocPropertyTags"): "js_doc_property_tags"}

# Kinds tsgo's mask table leaves out (their data is not a children mask)
# whose children its hand-written VisitEachChild visits (ast.go).
EXTRA_PROPERTIES = {"SourceFile": ["Statements", "EndOfFileToken"]}

# tsgo kinds whose node tsc-rs never produces (tsgo's reparser and checker).
TSGO_ONLY_KINDS = {"JSImportDeclaration", "JSTypeAliasDeclaration", "SyntheticExpression",
                   "SyntheticReferenceExpression"}

# tsgo kinds the encoder names (KIND_<SCREAMING_SNAKE> constants).
NAMED_KINDS = ["Identifier", "StringLiteral", "NoSubstitutionTemplateLiteral", "TemplateTail"]

FORMS = {"Option<NodeId>": "node", "Option<NodeArrayId>": "list",
         "Option<JSDocComment>": "comment"}


def vendored(profile, path):
    return (ROOT / "vendor/typescript-native" / profile / "upstream/tsc" / path).read_text()


def tsgo_kinds(profile):
    text = vendored(profile, "internal/ast/kind_generated.go")
    block = re.search(r"const \(\s*KindUnknown Kind = iota\n(.*?)\n\)", text, re.S).group(1)
    names = ["Unknown"]
    for line in block.split("\n"):
        line = line.split("//")[0].strip()
        if not line:
            continue
        if "=" in line:
            break  # the aliases (KindFirstAssignment = ...) follow the members
        match = re.fullmatch(r"Kind(\w+)", line)
        if not match:
            raise SystemExit(f"kind_generated.go: unexpected line {line!r}")
        names.append(match.group(1))
    return names


def encoder_tables(profile):
    text = vendored(profile, "internal/api/encoder/encoder_generated.go")
    data_types = {}
    body = re.search(r"func getNodeDataType\(node \*ast\.Node\) uint32 \{(.*?)\n\}\n", text, re.S).group(1)
    for kinds, result in re.findall(r"case ((?:ast\.Kind\w+,?\s*)+):\s*return (NodeDataType\w+)", body):
        for kind in re.findall(r"ast\.Kind(\w+)", kinds):
            data_types[kind] = result
    properties = {}
    body = re.search(r"func getChildrenPropertyMask\(node \*ast\.Node\) uint8 \{(.*?)\n\}\n", text, re.S).group(1)
    for match in re.finditer(r"case ((?:ast\.Kind\w+,?\s*)+):\s*\n\s*n := node\.As\w+\(\)\s*\n\s*return (.*?)\n", body):
        names = []
        for index, (field, modifiers, slice_, shift) in enumerate(re.findall(
                r"boolToByte\((?:n\.(\w+)(?:\(\))? != nil|hasModifiers\(n\.(\w+)\(\)\)|len\(n\.(\w+)\) > 0)\) << (\d+)",
                match.group(2))):
            if int(shift) != index:
                raise SystemExit(f"getChildrenPropertyMask: bit {shift} out of order in {match.group(1)}")
            names.append(field or modifiers or slice_)
        for kind in re.findall(r"ast\.Kind(\w+)", match.group(1)):
            properties[kind] = names
    return data_types, properties


def port_kinds():
    text = (ROOT / "crates/syntax/src/kind.rs").read_text()
    block = re.search(r"pub enum SyntaxKind \{(.*?)\n\}", text, re.S).group(1)
    kinds = []
    for line in block.split("\n"):
        line = line.split("//")[0].strip().rstrip(",")
        if line and not line.startswith("#"):
            kinds.append(line.split("=")[0].strip())
    return kinds


def port_nodes():
    """NodeData variant -> (boxed, {field: form or type})."""
    text = (ROOT / "crates/syntax/src/nodes.rs").read_text()
    text = re.sub(r"^\s*///.*$", "", text, flags=re.M)
    structs = {}
    for name, body in re.findall(r"pub struct (\w+) \{([^}]*)\}", text):
        fields = {}
        for field, ty in re.findall(r"pub (r#\w+|\w+): ([^,\n]+),", body):
            fields[field] = FORMS.get(ty.strip(), ty.strip())
        structs[name] = fields
    block = re.search(r"pub enum NodeData \{(.*?)\n\}", text, re.S).group(1)
    variants = {}
    for variant, boxed, data in re.findall(r"(\w+)\((Box<)?(\w+)>?\)", block):
        variants[variant] = structs[data]
    return variants


def snake(name):
    out = re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()
    return "r#type" if out == "type" else out


def constant(name):
    """A kind name as a constant name (`JSDocText` -> `JSDOC_TEXT`)."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).upper()


def property_expr(kind, prop, fields, used):
    """The Property expression of tsgo property `prop`."""
    if prop == "PostfixToken":
        tokens = POSTFIX_KINDS[kind]
        used.update(tokens)
        expr = f"d.{tokens[0]}"
        for token in tokens[1:]:
            expr += f".or(d.{token})"
        return f"Property::Node({expr})"
    if (kind, prop) in SLICES:
        field = SLICES[(kind, prop)]
        used.add(field)
        return f"Property::Slice(d.{field})"
    field = FIELD_RENAMES.get((kind, prop), snake(prop))
    form = fields.get(field)
    if form is None:
        if (kind, prop) in ABSENT:
            # Never present, but it keeps its mask bit.
            return "Property::Node(None)"
        raise SystemExit(f"{kind}: tsgo property {prop} has no tsc-rs field ({field})")
    used.add(field)
    if prop == "Modifiers":
        if form != "list":
            raise SystemExit(f"{kind}: modifiers field is {form}")
        return f"Property::Modifiers(d.{field})"
    if form == "node":
        return f"Property::Node(d.{field})"
    if form == "list":
        return f"Property::List(d.{field})"
    if form == "comment":
        return f"Property::Comment(&d.{field})"
    raise SystemExit(f"{kind}: field {field} has type {form}")


def generate(profile):
    kinds = tsgo_kinds(profile)
    data_types, properties = encoder_tables(profile)
    properties.update(EXTRA_PROPERTIES)
    port = port_kinds()
    variants = port_nodes()
    index = {name: number for number, name in enumerate(kinds)}

    out = [
        "// @generated by scripts/generate_api_encoder.py from the vendored",
        "// internal/ast/kind_generated.go and internal/api/encoder/encoder_generated.go.",
        "// Do not edit by hand.",
        "",
        "use tsc_syntax::{Node, NodeData, SyntaxKind};",
        "",
        "use super::Property;",
        "",
        *[f"pub(crate) const KIND_{constant(name)}: u32 = {index[name]};" for name in NAMED_KINDS],
        "",
        "/// tsgo's `Kind.String()` of each kind number.",
        f"pub(crate) const KIND_NAMES: [&str; {len(kinds)}] = [",
        *[f'    "Kind{name}",' for name in kinds],
        "];",
        "",
        "/// tsgo's kind number of a tsc-rs kind (`None`: tsgo has no such kind).",
        "pub(crate) fn tsgo_kind(kind: SyntaxKind) -> Option<u32> {",
        "    Some(match kind {",
    ]
    for kind in port:
        name = KIND_RENAMES.get(kind, kind)
        if name in index:
            out.append(f"        SyntaxKind::{kind} => {index[name]},")
    out += ["        _ => return None,", "    })", "}", ""]

    by_type = {}
    for kind, result in data_types.items():
        by_type.setdefault(result, []).append(index[kind])
    out += [
        "/// tsgo `getNodeDataType` of a tsgo kind number.",
        "pub(crate) fn node_data_type(kind: u32) -> u32 {",
        "    match kind {",
    ]
    for result in sorted(by_type):
        numbers = " | ".join(str(number) for number in sorted(by_type[result]))
        out.append(f"        {numbers} => super::{snake(result).upper()},")
    out += ["        _ => super::NODE_DATA_TYPE_CHILDREN,", "    }", "}", ""]

    out += [
        "/// The child properties tsgo's encoder visits on `node`, in visitor order",
        "/// (`getChildrenPropertyMask`'s bits); a node with none calls nothing.",
        "pub(crate) fn for_each_property<'a>(node: &'a Node, mut f: impl FnMut(Property<'a>)) {",
        "    match &node.data {",
    ]
    for kind in port:
        name = KIND_RENAMES.get(kind, kind)
        if name not in properties or kind not in variants:
            continue
        fields = variants[kind]
        used = set()
        calls = [property_expr(name, prop, fields, used) for prop in properties[name]]
        for field, form in fields.items():
            if form in ("node", "list", "comment") and field not in used and (name, field) not in PORT_ONLY:
                raise SystemExit(f"{kind}: tsc-rs field {field} is not a tsgo property")
        if calls:
            binding = "d" if any("d." in call for call in calls) else "_"
            out.append(f"        NodeData::{kind}({binding}) => {{")
            out += [f"            f({call});" for call in calls]
            out.append("        }")
    out += ["        _ => {}", "    }", "}", ""]

    missing = [kind for kind in properties if kind not in TSGO_ONLY_KINDS
               and not any(KIND_RENAMES.get(port_kind, port_kind) == kind and port_kind in variants
                           for port_kind in port)]
    if missing:
        raise SystemExit(f"tsgo kinds with children and no tsc-rs node: {missing}")
    return "\n".join(out)


def decode_statement(kind, prop, bit, fields, used):
    """The statement filling the tsc-rs field of tsgo property `prop` (mask bit `bit`)."""
    if prop == "PostfixToken":
        tokens = POSTFIX_KINDS[kind]
        used.update(tokens)
        if len(tokens) == 1:
            return [f"d.{tokens[0]} = c.node({bit});"]
        return [
            f"if let Some((token, kind)) = c.token({bit}) {{",
            "    if kind == SyntaxKind::QuestionToken {",
            f"        d.{tokens[0]} = Some(token);",
            "    } else {",
            f"        d.{tokens[1]} = Some(token);",
            "    }",
            "}",
        ]
    if (kind, prop) in SLICES:
        field = SLICES[(kind, prop)]
        used.add(field)
        return [f"d.{field} = c.slice({bit});"]
    field = FIELD_RENAMES.get((kind, prop), snake(prop))
    form = fields.get(field)
    if form is None:
        if (kind, prop) in ABSENT:
            # tsc-rs's tree has no place for it.
            return [f"c.skip({bit});"]
        raise SystemExit(f"{kind}: tsgo property {prop} has no tsc-rs field ({field})")
    used.add(field)
    if form == "node":
        return [f"d.{field} = c.node({bit});"]
    if form == "list":
        return [f"d.{field} = c.list({bit});"]
    if form == "comment":
        return [f"d.{field} = c.comment({bit});"]
    raise SystemExit(f"{kind}: field {field} has type {form}")


def generate_decoder(profile):
    kinds = tsgo_kinds(profile)
    _, properties = encoder_tables(profile)
    port = port_kinds()
    variants = port_nodes()
    index = {name: number for number, name in enumerate(kinds)}

    out = [
        "// @generated by scripts/generate_api_encoder.py from the vendored",
        "// internal/ast/kind_generated.go and internal/api/encoder/encoder_generated.go.",
        "// Do not edit by hand.",
        "",
        "use tsc_syntax::{NodeData, SyntaxKind};",
        "",
        "use super::Children;",
        "",
        "/// The tsc-rs kind of a tsgo kind number (`None`: tsc-rs has no such kind).",
        "pub(crate) fn port_kind(kind: u32) -> Option<SyntaxKind> {",
        "    Some(match kind {",
    ]
    seen = {}
    for kind in port:
        name = KIND_RENAMES.get(kind, kind)
        if name not in index:
            continue
        if name in seen:
            raise SystemExit(f"tsgo kind {name} is both {seen[name]} and {kind}")
        seen[name] = kind
    for name, number in sorted(index.items(), key=lambda item: item[1]):
        if name in seen:
            out.append(f"        {number} => SyntaxKind::{seen[name]},")
    out += ["        _ => return None,", "    })", "}", ""]

    out += [
        "/// Fills the child fields of `data` from the children tsgo's encoder",
        "/// wrote in visitor order (`getChildrenPropertyMask`'s bits).",
        "pub(crate) fn decode_properties(data: &mut NodeData, c: &mut Children<'_>) {",
        "    match data {",
    ]
    for kind in port:
        name = KIND_RENAMES.get(kind, kind)
        if name not in properties or kind not in variants or name in EXTRA_PROPERTIES:
            continue
        fields = variants[kind]
        used = set()
        statements = []
        for bit, prop in enumerate(properties[name]):
            statements += decode_statement(name, prop, bit, fields, used)
        if not statements:
            continue
        binding = "d" if any("d." in statement for statement in statements) else "_"
        out.append(f"        NodeData::{kind}({binding}) => {{")
        out += [f"            {statement}" for statement in statements]
        out.append("        }")
    out += ["        _ => {}", "    }", "}", ""]
    return "\n".join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    outputs = [(OUTPUT, generate(args.profile)), (DECODER_OUTPUT, generate_decoder(args.profile))]
    if args.check:
        for output, text in outputs:
            if not output.exists() or output.read_text() != text:
                raise SystemExit(f"{output.relative_to(ROOT)} is not what the vendored sources generate")
            print(f"{output.relative_to(ROOT)} is up to date")
        return
    for output, text in outputs:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text)
        print(f"wrote {output.relative_to(ROOT)}")


if __name__ == "__main__":
    sys.exit(main())
