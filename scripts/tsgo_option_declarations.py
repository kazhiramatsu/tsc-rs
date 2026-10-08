#!/usr/bin/env python3
"""Generate the command-line option declarations of TypeScript 7.1 for tsc-rs.

usage: tsgo_option_declarations.py [--profile <name>] [--check]

tsgo declares its options as Go values (internal/tsoptions: declscompiler.go,
declsbuild.go, declswatch.go, commandlineoption.go, enummaps.go) with the
enum constants of internal/core (compileroptions.go, watchoptions.go). The
help, --init and --showConfig read their names, short names, kinds,
categories, descriptions, default values, help visibility, enum maps and
deprecated keys. This script reads the vendored copies of those files
(vendor/typescript-native/<profile>/upstream/tsc/internal/...) and writes
crates/compiler/src/options/gen.rs; --check compares instead of writing.
Diagnostic messages are referenced by their tsc_diagnostics::gen names,
which are checked against crates/diagnostics/src/gen.rs.
"""

import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROFILE = "7.1.0-dev-19dadef8"
OUTPUT = ROOT / "crates/compiler/src/options/gen.rs"
GEN = ROOT / "crates/diagnostics/src/gen.rs"
SOURCES = [
    "tsc/internal/tsoptions/declscompiler.go",
    "tsc/internal/tsoptions/declsbuild.go",
    "tsc/internal/tsoptions/declswatch.go",
    "tsc/internal/tsoptions/commandlineoption.go",
    "tsc/internal/tsoptions/enummaps.go",
    "tsc/internal/core/compileroptions.go",
    "tsc/internal/core/watchoptions.go",
]
KINDS = {
    "CommandLineOptionTypeString": "String",
    "CommandLineOptionTypeNumber": "Number",
    "CommandLineOptionTypeBoolean": "Boolean",
    "CommandLineOptionTypeObject": "Object",
    "CommandLineOptionTypeList": "List",
    "CommandLineOptionTypeEnum": "Enum",
    '"string"': "String",
    '"number"': "Number",
    '"boolean"': "Boolean",
    '"object"': "Object",
    '"list"': "List",
    '"enum"': "Enum",
}


def strip_comments(text):
    out = []
    for line in text.splitlines():
        in_string = False
        cut = len(line)
        index = 0
        while index < len(line):
            char = line[index]
            if char == '"' and (index == 0 or line[index - 1] != "\\"):
                in_string = not in_string
            elif char == "`":
                in_string = not in_string
            elif not in_string and line.startswith("//", index):
                cut = index
                break
            index += 1
        out.append(line[:cut])
    return "\n".join(out)


def block(text, start):
    """The text between the brace at or after `start` and its match."""
    open_index = text.index("{", start)
    depth = 0
    in_string = False
    for index in range(open_index, len(text)):
        char = text[index]
        if char == '"' and text[index - 1] != "\\":
            in_string = not in_string
        elif in_string:
            continue
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[open_index + 1 : index], index + 1
    raise SystemExit("unbalanced braces")


def elements(body):
    """The top-level `{...}` literals (or `&Name`) of a slice body."""
    items = []
    index = 0
    while index < len(body):
        char = body[index]
        if char == "{":
            inner, end = block(body, index)
            items.append(("literal", inner))
            index = end
        elif body.startswith("&", index):
            match = re.match(r"&(\w+)", body[index:])
            items.append(("reference", match.group(1)))
            index += match.end()
        else:
            index += 1
    return items


def fields(literal):
    """`Name: value` pairs of a struct literal (values without nesting)."""
    result = {}
    for match in re.finditer(r"(\w+):\s*([^,\n]+(?:\([^)]*\))?),?\s*(?:\n|$)", literal):
        result[match.group(1)] = match.group(2).strip().rstrip(",")
    return result


def go_string(value):
    if value.startswith('"'):
        return bytes(value[1:-1], "utf-8").decode("unicode_escape")
    if value.startswith("`"):
        return value[1:-1]
    return None


class Constants:
    """The numeric value of each core enum constant."""

    def __init__(self, texts):
        self.values = {}
        pending = {}
        for text in texts:
            for match in re.finditer(r"^\s*(\w+)\s+\w+\s*=\s*(\w+)", text, re.M):
                name, value = match.groups()
                if value.lstrip("-").isdigit():
                    self.values[name] = int(value)
                else:
                    pending[name] = value
        for name, alias in pending.items():
            if alias in self.values:
                self.values[name] = self.values[alias]

    def __getitem__(self, name):
        return self.values[name.removeprefix("core.")]


def tsgo_identifier(text):
    """tsgo diagnostics/generate.go `convertPropertyName`'s variable name."""
    out = []
    for char in text:
        if char == "*":
            out.append("_Asterisk")
        elif char == "/":
            out.append("_Slash")
        elif char == ":":
            out.append("_Colon")
        elif char.isalpha() or char.isdigit():
            out.append(char)
        else:
            out.append("_")
    name = re.sub(r"_+", "_", "".join(out))
    name = re.sub(r"^_+(\D)", r"\1", name)
    name = re.sub(r"_$", "", name)
    if not name[:1].isupper():
        name = ("X" if name.startswith("_") else "X_") + name
    return name


def message_names(profile):
    """tsgo's identifier of each message -> its tsc_diagnostics::gen name,
    through the message code (gen.rs names differ from tsgo's in places)."""
    import json

    catalog = json.loads(
        (ROOT / "vendor/typescript-native" / profile / "upstream/tsc/internal/diagnostics/diagnosticMessages.json").read_text()
    )
    by_code = dict(
        (int(code), name) for code, name in re.findall(r"^\s*\((\d+), &(\w+)\),", GEN.read_text(), re.M)
    )
    names = {}
    for text, entry in catalog.items():
        code = entry["code"]
        if code in by_code:
            names[tsgo_identifier(text)] = by_code[code]
    return names


def message(value, names):
    if not value.startswith("diagnostics."):
        return None
    identifier = value.removeprefix("diagnostics.")
    if identifier not in names:
        raise SystemExit(f"no tsc_diagnostics::gen message for {value}")
    return names[identifier]


def enum_maps(text, constants):
    maps = {}
    for match in re.finditer(r"var (\w+) = collections\.NewOrderedMapFromList", text):
        body, _ = block(text, match.end())
        entries = []
        for entry in re.finditer(r'\{Key:\s*"([^"]+)",\s*Value:\s*([^}]+?)\}', body):
            key, value = entry.groups()
            value = value.strip()
            if value.startswith('"'):
                entries.append((key, ("text", go_string(value))))
            else:
                entries.append((key, ("number", constants[value])))
        maps[match.group(1)] = entries
    return maps


def declaration(literal, names, constants):
    raw = fields(literal)
    decl = {
        "name": go_string(raw["Name"]),
        "short_name": go_string(raw["ShortName"]) if "ShortName" in raw else None,
        "kind": KINDS[raw["Kind"]],
        "is_file_path": raw.get("IsFilePath") == "true",
        "is_command_line_only": raw.get("IsCommandLineOnly") == "true",
        "show_in_simplified_help_view": raw.get("ShowInSimplifiedHelpView") == "true",
        "description": message(raw["Description"], names) if "Description" in raw else None,
        "category": message(raw["Category"], names) if "Category" in raw else None,
    }
    default = raw.get("DefaultValueDescription")
    if default is None or default == "nil":
        decl["default"] = "DefaultValue::None"
    elif default == "core.TSUnknown":
        decl["default"] = "DefaultValue::Unknown"
    elif default in ("true", "false"):
        decl["default"] = f"DefaultValue::Bool({default})"
    elif default.lstrip("-").isdigit():
        decl["default"] = f"DefaultValue::Number({default})"
    elif default.startswith("diagnostics."):
        decl["default"] = f"DefaultValue::Message(&gen::{message(default, names)})"
    elif default.startswith("core."):
        decl["default"] = f"DefaultValue::Enum({constants[default]})"
    else:
        text = go_string(default)
        if text is None:
            raise SystemExit(f"unknown default {default!r}")
        decl["default"] = f"DefaultValue::Text({rust_string(text)})"
    return decl


def rust_string(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def render_declaration(decl, indent):
    pad = " " * indent
    lines = [f"{pad}OptionDeclaration {{"]
    lines.append(f"{pad}    name: {rust_string(decl['name'])},")
    short = decl["short_name"]
    lines.append(f"{pad}    short_name: {'Some(' + rust_string(short) + ')' if short else 'None'},")
    lines.append(f"{pad}    kind: OptionKind::{decl['kind']},")
    for flag in ("is_file_path", "is_command_line_only", "show_in_simplified_help_view"):
        lines.append(f"{pad}    {flag}: {'true' if decl[flag] else 'false'},")
    for key in ("description", "category"):
        value = decl[key]
        lines.append(f"{pad}    {key}: {'Some(&gen::' + value + ')' if value else 'None'},")
    lines.append(f"{pad}    default: {decl['default']},")
    lines.append(f"{pad}}}")
    return "\n".join(lines)


def generate(profile):
    upstream = ROOT / "vendor/typescript-native" / profile / "upstream"
    texts = {source: strip_comments((upstream / source).read_text()) for source in SOURCES}
    constants = Constants([texts["tsc/internal/core/compileroptions.go"], texts["tsc/internal/core/watchoptions.go"]])
    names = message_names(profile)
    compiler = texts["tsc/internal/tsoptions/declscompiler.go"]
    build = texts["tsc/internal/tsoptions/declsbuild.go"]
    watch = texts["tsc/internal/tsoptions/declswatch.go"]
    option = texts["tsc/internal/tsoptions/commandlineoption.go"]

    def slice_literals(text, variable):
        start = text.index(f"var {variable} = []*CommandLineOption")
        body, _ = block(text, start + len(f"var {variable} = []*CommandLineOption"))
        return elements(body)

    common = [declaration(item, names, constants) for kind, item in slice_literals(compiler, "commonOptionsWithBuild")]
    for_compiler = [declaration(item, names, constants) for kind, item in slice_literals(compiler, "optionsForCompiler")]
    build_option_body, _ = block(build, build.index("var TscBuildOption = CommandLineOption"))
    build_option = declaration(build_option_body, names, constants)
    for_build = [declaration(item, names, constants) for kind, item in slice_literals(build, "OptionsForBuild") if kind == "literal"]
    for_watch = [declaration(item, names, constants) for kind, item in slice_literals(watch, "OptionsForWatch")]

    element_body, _ = block(option, option.index("var commandLineOptionElements"))
    element_decls = []
    for match in re.finditer(r'"(\w+)":\s*\{', element_body):
        inner, _ = block(element_body, match.end() - 1)
        decl = declaration(inner, names, constants)
        element_decls.append((match.group(1), decl))

    maps = enum_maps(texts["tsc/internal/tsoptions/enummaps.go"], constants)
    enum_body, _ = block(option, option.index("var commandLineOptionEnumMap"))
    option_maps = re.findall(r'"(\w+)":\s*(\w+),', enum_body)
    deprecated_body, _ = block(option, option.index("var commandLineOptionDeprecated"))
    deprecated = {
        name: re.findall(r'"([^"]+)"', items)
        for name, items in re.findall(r'"(\w+)":\s*collections\.NewSetFromItems\(([^)]*)\)', deprecated_body)
    }

    out = []
    out.append("// @generated by scripts/tsgo_option_declarations.py. Do not edit by hand.")
    out.append(f"// Source: vendor/typescript-native/{profile}/upstream/tsc/internal/tsoptions")
    out.append("// (declscompiler.go, declsbuild.go, declswatch.go, commandlineoption.go,")
    out.append("// enummaps.go) and tsc/internal/core (compileroptions.go, watchoptions.go).")
    out.append("")
    out.append("use tsc_diagnostics::gen;")
    out.append("")
    out.append("use super::{DefaultValue, EnumValue, OptionDeclaration, OptionKind};")
    out.append("")

    def emit_list(name, decls, doc):
        out.append(f"/// {doc}")
        out.append(f"pub(crate) static {name}: &[OptionDeclaration] = &[")
        for decl in decls:
            out.append(render_declaration(decl, 4) + ",")
        out.append("];")
        out.append("")

    emit_list("COMMON_OPTIONS_WITH_BUILD", common, "tsgo `commonOptionsWithBuild`.")
    emit_list("OPTIONS_FOR_COMPILER", for_compiler, "tsgo `optionsForCompiler`.")
    out.append("/// tsgo `TscBuildOption`.")
    out.append("pub(crate) static BUILD_OPTION: OptionDeclaration = " + render_declaration(build_option, 0) + ";")
    out.append("")
    emit_list("OPTIONS_FOR_BUILD", for_build, "tsgo `OptionsForBuild` after `TscBuildOption`.")
    emit_list("OPTIONS_FOR_WATCH", for_watch, "tsgo `OptionsForWatch`.")
    out.append("/// tsgo `commandLineOptionElements`: the element declaration of a list option.")
    out.append("pub(crate) fn elements(option: &str) -> Option<&'static OptionDeclaration> {")
    out.append("    match option {")
    for name, decl in element_decls:
        out.append(f"        {rust_string(name)} => Some(&{render_declaration(decl, 8).lstrip()}),")
    out.append("        _ => None,")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("/// tsgo `commandLineOptionEnumMap`: the ordered keys and values of an enum option.")
    out.append("pub(crate) fn enum_map(option: &str) -> Option<&'static [(&'static str, EnumValue)]> {")
    out.append("    match option {")
    for name, variable in option_maps:
        out.append(f"        {rust_string(name)} => Some(&[")
        for key, (kind, value) in maps[variable]:
            rendered = f"EnumValue::Number({value})" if kind == "number" else f"EnumValue::Text({rust_string(value)})"
            out.append(f"            ({rust_string(key)}, {rendered}),")
        out.append("        ]),")
    out.append("        _ => None,")
    out.append("    }")
    out.append("}")
    out.append("")
    struct_start = texts["tsc/internal/core/compileroptions.go"].index("type CompilerOptions struct")
    struct_body, _ = block(texts["tsc/internal/core/compileroptions.go"], struct_start + len("type CompilerOptions struct"))
    field_names = re.findall(r'`json:"(\w+)[,"]', struct_body)
    out.append("/// tsgo `core.CompilerOptions`'s fields in declaration order, by their JSON")
    out.append("/// names (`--showConfig` writes the options in this order).")
    out.append("pub(crate) static COMPILER_OPTION_FIELDS: &[&str] = &[")
    for name in field_names:
        out.append(f"    {rust_string(name)},")
    out.append("];")
    out.append("")
    out.append("/// tsgo `commandLineOptionDeprecated`: the enum keys help leaves out.")
    out.append("pub(crate) fn deprecated_keys(option: &str) -> &'static [&'static str] {")
    out.append("    match option {")
    for name, keys in deprecated.items():
        out.append(f"        {rust_string(name)} => &[{', '.join(rust_string(key) for key in keys)}],")
    out.append("        _ => &[],")
    out.append("    }")
    out.append("}")
    return "\n".join(out) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generated = generate(args.profile)
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_text() != generated:
            raise SystemExit(f"{OUTPUT.relative_to(ROOT)} differs from the generated declarations")
        print(f"{OUTPUT.relative_to(ROOT)} matches")
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(generated)
    print(f"{OUTPUT.relative_to(ROOT)}: {len(generated)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
