#!/usr/bin/env python3
"""Generate the language service's user preferences from tsgo's schema.

    scripts/generate_lsp_preferences.py [--check]

tsgo describes its user preferences in tools/userPreferences.schema.json
(vendored) and generates their Go types, parsing and serialization with
tools/gen-preferences. This script writes the same in Rust
(crates/ls/src/lsutil/user_preferences_generated.rs): a struct per
definition (an `allOf` base is a field named after it, first), a field per
property in name order, the enumerations' parsers and serializers, the
raw-name parsing (`apply_raw_preferences`), the configuration-path parsing
(`with_config`) and the configuration form (`to_config`). The handwritten
types and helpers they use are in user_preferences.rs and
format_code_options.rs. --check fails when the file is out of date.

Types follow tsgo's: a boolean is a tristate (`Option<bool>`) unless the
schema gives the Go type `bool`, an integer `i64` (Go's int), a string list
`Option<Vec<String>>` (Go keeps a nil list apart from an empty one), and an
enumeration a newtype over its Go underlying type with tsgo's constants.
"""

import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "vendor/typescript-native/7.1.0-dev-aa814927/upstream/tools/userPreferences.schema.json"
OUTPUT = ROOT / "crates/ls/src/lsutil/user_preferences_generated.rs"
# Types defined outside the language service (tsgo's package prefixes).
EXTERNAL = {"modulespecifiers": "tsc_checker"}


def fail(message):
    sys.exit(f"generate_lsp_preferences: {message}")


def go_name(name):
    """tsgo `goName`: each `-`-separated part capitalized."""
    return "".join(part[:1].upper() + part[1:] for part in name.split("-") if part)


def snake(name):
    words = re.findall(r"[A-Z]+(?=[A-Z][a-z]|\d|$)|[A-Z]?[a-z]+|\d+|[A-Z]+", name)
    return "_".join(word.lower() for word in words)


def screaming(value):
    return snake(go_name(value)).upper()


class Property:
    def __init__(self, name, raw):
        self.name = name
        self.type = raw.get("type", "")
        self.description = raw.get("description", "")
        self.enum = raw.get("enum", [])
        self.items = raw.get("items")
        self.go_type = raw.get("x-go-type", "")
        self.config = raw.get("x-config", "")
        self.fallback_config = raw.get("x-fallback-config", "")
        self.go_underlying = raw.get("x-go-underlying", "")
        self.default = raw.get("default", "")
        self.accept_number = raw.get("x-accept-number", False)
        self.config_parser = raw.get("x-config-parser", "")
        self.validator = raw.get("x-validator", "")
        known = {"type", "description", "enum", "items", "x-go-type", "x-config",
                 "x-fallback-config", "x-go-underlying", "default", "x-accept-number",
                 "x-config-parser", "x-validator"}
        unknown = set(raw) - known
        if unknown:
            fail(f"{name}: unknown schema keys {sorted(unknown)}")
        if self.enum and not self.go_type:
            self.go_type = go_name(name)

    def go_kind(self):
        """tsgo `goKind`: the Go underlying kind."""
        if self.go_underlying:
            return self.go_underlying
        return {"boolean": "bool", "integer": "int"}.get(self.type, self.type)

    def is_tristate(self):
        return self.type == "boolean" and not self.go_type

    def is_string_list(self):
        return (self.type == "array" and self.items is not None
                and self.items.get("type") == "string" and not self.items.get("enum"))

    def enum_type(self):
        """The enumeration's Rust type name (tsgo's Go type without its package)."""
        return self.go_type.split(".")[-1]

    def serializes_enum(self):
        """tsgo `serializeEnum`: a non-string enumeration written by name."""
        return bool(self.enum) and self.go_kind() != "string" and not self.accept_number

    def rust_type(self):
        if self.enum:
            return self.enum_type()
        if self.go_type == "bool":
            return "bool"
        if self.go_type:
            fail(f"{self.name}: unsupported Go type {self.go_type}")
        if self.type == "boolean":
            return "Option<bool>"
        if self.type == "integer":
            return "i64"
        if self.type == "string":
            return "String"
        if self.is_string_list():
            return "Option<Vec<String>>"
        fail(f"{self.name}: unsupported type {self.type}")

    def validate(self):
        """tsgo `validateParsing`."""
        if self.go_underlying not in ("", "string", "int", "bool"):
            fail(f"{self.name}: unsupported Go underlying type {self.go_underlying}")
        if self.fallback_config and not self.config:
            fail(f"{self.name}: fallback config paths require a primary config path")
        if self.config_parser and (not self.config or self.fallback_config):
            fail(f"{self.name}: custom config parsers require a primary path without fallbacks")
        for tag in [self.config, *self.fallback_config.split(";")]:
            if not tag:
                continue
            path, _, modifier = tag.partition(",")
            if not path or modifier not in ("", "invert"):
                fail(f"{self.name}: invalid configuration mapping {tag!r}")
            if modifier == "invert" and self.type != "boolean":
                fail(f"{self.name}: only boolean preferences can invert configuration values")
        if not self.enum:
            if self.default or self.accept_number:
                fail(f"{self.name}: enum parsing metadata requires enum values")
            return
        if self.accept_number and self.go_kind() != "int":
            fail(f"{self.name}: numeric parsing is only supported for integer enums")
        if self.default and self.default not in self.enum:
            fail(f"{self.name}: enum default must have a wire value")
        if self.go_kind() != "string" and not self.default:
            fail(f"{self.name}: non-string Go enums require an explicit default")
        lowered = [value.lower() for value in self.enum]
        if len(set(lowered)) != len(lowered):
            fail(f"{self.name}: ambiguous case-insensitive enum values")


class Schema:
    def __init__(self, raw):
        if set(raw) - {"$schema", "$ref", "$comment", "$defs"}:
            fail("unknown top-level schema keys")
        self.ref = raw["$ref"]
        self.defs = {}
        for name, definition in raw["$defs"].items():
            if definition.get("type") != "object":
                fail(f"{name}: preference definitions must be objects")
            bases = [self.resolve(ref["$ref"], raw) for ref in definition.get("allOf", [])]
            properties = {key: Property(key, value)
                          for key, value in definition.get("properties", {}).items()}
            for prop in properties.values():
                prop.validate()
            self.defs[name] = (definition.get("description", ""), bases, properties)

    @staticmethod
    def resolve(ref, raw):
        if not ref.startswith("#/$defs/") or ref[len("#/$defs/"):] not in raw["$defs"]:
            fail(f"unsupported preference schema reference {ref!r}")
        return ref[len("#/$defs/"):]

    def flattened(self, name, prefix=""):
        """tsgo `flattenedFields`: (wire name, Rust selector, property), bases first."""
        _, bases, properties = self.defs[name]
        fields = []
        for base in bases:
            fields.extend(self.flattened(base, f"{prefix}{snake(base)}."))
        for key in sorted(properties):
            fields.append((key, prefix + snake(key), properties[key]))
        return fields


class Writer:
    def __init__(self, schema):
        self.schema = schema
        self.lines = []

    def line(self, text=""):
        self.lines.append(text)

    def doc(self, text, indent=""):
        # Rust's doc comments take spaces for the schema's tabs.
        for line in text.split("\n") if text else []:
            self.line(f"{indent}/// {line.replace(chr(9), '    ')}".rstrip())

    def assignment(self, target, prop):
        """tsgo `writeAssignment`: a raw JSON value into the field."""
        if prop.enum:
            self.line(f"{target} = parse_preference_{snake(prop.enum_type())}(value);")
        elif prop.is_tristate():
            self.line(f"{target} = parse_preference_tristate(value);")
        elif prop.go_kind() == "int":
            self.line("if let Some(number) = value.as_f64() {")
            self.line(f"{target} = number as i64;")
            self.line("}")
        elif prop.is_string_list():
            self.line("if let Some(values) = value.as_array() {")
            self.line(f"{target} = Some(values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect());")
            self.line("}")
        elif prop.go_kind() == "string":
            self.line("if let Some(value) = value.as_str() {")
            self.line(f"{target} = value.to_owned();")
            self.line("}")
        elif prop.go_kind() == "bool":
            self.line("if let Some(value) = value.as_bool() {")
            self.line(f"{target} = value;")
            self.line("}")
        else:
            fail(f"{prop.name}: no assignment")

    def present(self, value, prop):
        """tsgo `presentCondition`."""
        if prop.serializes_enum():
            return None
        if prop.is_tristate():
            return f"{value}.is_some()"
        if prop.is_string_list():
            return f"{value}.is_some()"
        kind = prop.go_kind()
        inner = f"{value}.0" if prop.enum else value
        if kind == "string":
            return f"!{inner}.is_empty()"
        if kind == "int":
            return f"{inner} != 0"
        return None

    def serialized(self, value, prop):
        if prop.is_tristate():
            return f"Value::Bool({value} == Some(true))"
        if prop.serializes_enum():
            return f"Value::from(serialize_preference_{snake(prop.enum_type())}({value}))"
        kind = prop.go_kind()
        if prop.is_string_list():
            return f"Value::from({value}.clone().unwrap_or_default())"
        if kind == "string":
            return f"Value::from({value}.0)" if prop.enum else f"Value::from({value}.clone())"
        if kind == "int":
            return f"Value::from({value}.0)" if prop.enum else f"Value::from({value})"
        return f"Value::Bool({value})"

    def write(self):
        schema = self.schema
        root = Schema.resolve(schema.ref, {"$defs": schema.defs})
        if root != "UserPreferences":
            fail("the schema must define UserPreferences as its root")
        fields = schema.flattened(root)
        self.line("// Code generated by scripts/generate_lsp_preferences.py from tsgo's")
        self.line("// tools/userPreferences.schema.json; DO NOT EDIT.")
        self.line()
        self.line("use serde_json::{Map, Value};")
        external = sorted({(EXTERNAL[prop.go_type.split('.')[0]], prop.enum_type())
                           for _, _, prop in fields if "." in prop.go_type})
        for crate, name in external:
            self.line(f"use {crate}::{name};")
        self.line()
        self.line("use super::format_code_options::*;")
        self.line("use super::user_preferences::*;")
        for name in sorted(schema.defs):
            description, bases, properties = schema.defs[name]
            self.line()
            self.doc(description)
            self.line("#[derive(Clone, Debug, Default, PartialEq)]")
            self.line(f"pub struct {name} {{")
            for base in bases:
                self.line(f"    pub {snake(base)}: {base},")
            for key in sorted(properties):
                prop = properties[key]
                self.doc(prop.description, "    ")
                self.line(f"    pub {snake(key)}: {prop.rust_type()},")
            self.line("}")
        enums = {}
        for _, _, prop in fields:
            if not prop.enum:
                continue
            previous = enums.get(prop.enum_type())
            if previous and (previous.enum != prop.enum or previous.default != prop.default
                             or previous.accept_number != prop.accept_number
                             or previous.go_underlying != prop.go_underlying):
                fail(f"inconsistent parsing metadata for {prop.enum_type()}")
            enums[prop.enum_type()] = prop
        for name in sorted(enums):
            prop = enums[name]
            self.line()
            self.line(f"/// tsgo `parsePreference{name}`: a name in any case, else the default.")
            self.line(f"fn parse_preference_{snake(name)}(value: &Value) -> {name} {{")
            self.line("if let Some(value) = value.as_str() {")
            self.line("match value.to_lowercase().as_str() {")
            for literal in prop.enum:
                self.line(f'"{literal.lower()}" => return {name}::{screaming(literal)},')
            self.line("_ => {}")
            self.line("}")
            self.line("}")
            if prop.accept_number:
                self.line("if let Some(number) = value.as_f64() {")
                self.line(f"return {name}(number as i64);")
                self.line("}")
            if prop.default:
                self.line(f"{name}::{screaming(prop.default)}")
            else:
                self.line(f"{name}::default()")
            self.line("}")
            if prop.serializes_enum():
                self.line()
                self.line(f"/// tsgo `serializePreference{name}`.")
                self.line(f"fn serialize_preference_{snake(name)}(value: {name}) -> &'static str {{")
                self.line("match value {")
                for literal in prop.enum:
                    self.line(f'{name}::{screaming(literal)} => "{literal}",')
                # Both values of a boolean enumeration are listed.
                if not (prop.go_kind() == "bool" and len(prop.enum) == 2):
                    self.line(f'_ => "{prop.default}",')
                self.line("}")
                self.line("}")
        self.line()
        self.line(f"impl {root} {{")
        self.line("    /// tsgo `applyRawPreferences`: the preferences given by their own names.")
        self.line("    fn apply_raw_preferences(&mut self, settings: &Map<String, Value>) {")
        self.line("for (name, value) in settings {")
        self.line("if value.is_null() {")
        self.line("continue;")
        self.line("}")
        self.line("match name.as_str() {")
        for key, selector, prop in fields:
            self.line(f'"{key}" => {{')
            self.assignment(f"self.{selector}", prop)
            self.line("}")
        self.line("_ => {}")
        self.line("}")
        self.line("}")
        self.line("}")
        self.line()
        self.line("    /// tsgo `withConfig`: the preferences a configuration gives, by their names")
        self.line("    /// (and under `unstable`), then at their configuration paths.")
        self.line("    pub(crate) fn with_config(mut self, config: &Map<String, Value>) -> Self {")
        self.line("self.apply_raw_preferences(config);")
        self.line('if let Some(Value::Object(unstable)) = config.get("unstable") {')
        self.line("self.apply_raw_preferences(unstable);")
        self.line("}")
        for key, selector, prop in fields:
            if not prop.config:
                continue
            paths = [prop.config, *(prop.fallback_config.split(";") if prop.fallback_config else [])]
            quoted = ", ".join(f'"{path}"' for path in paths)
            self.line(f"if let Some(value) = get_preference_config_value(config, &[{quoted}]) {{")
            if prop.config_parser:
                self.line(f"self.{selector} = {snake(prop.config_parser)}(&value);")
            else:
                self.line("if !value.is_null() {")
                self.line("let value = &value;")
                self.assignment(f"self.{selector}", prop)
                self.line("}")
            self.line("}")
        for key, selector, prop in fields:
            if prop.validator:
                self.line(f"self.{selector} = {snake(prop.validator)}(&self.{selector});")
        self.line("self")
        self.line("}")
        self.line()
        self.line("    /// tsgo `MarshalJSONTo`: the preferences at their configuration paths")
        self.line("    /// (`unstable.<name>` for one without), with the keys sorted.")
        self.line("    pub fn to_config(&self) -> Value {")
        self.line("let mut config = Map::new();")
        for key, selector, prop in fields:
            target = prop.config or f"unstable.{key}"
            path, _, modifier = target.partition(",")
            value = f"self.{selector}"
            serialized = self.serialized(value, prop)
            if modifier == "invert":
                serialized = f"Value::Bool({value} != Some(true))"
            condition = self.present(value, prop)
            if condition:
                self.line(f"if {condition} {{")
            self.line(f'set_nested_value(&mut config, "{path}", {serialized});')
            if condition:
                self.line("}")
        self.line("sort_keys(Value::Object(config))")
        self.line("}")
        self.line("}")
        return "\n".join(self.lines) + "\n"


def rustfmt(text):
    result = subprocess.run(["rustfmt", "--edition", "2021", "--emit", "stdout"],
                            input=text, capture_output=True, text=True)
    if result.returncode != 0:
        fail(f"rustfmt failed:\n{result.stderr}")
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    schema = Schema(json.loads(SCHEMA.read_text()))
    output = rustfmt(Writer(schema).write())
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != output:
            sys.exit(f"{OUTPUT.relative_to(ROOT)} is out of date; run scripts/generate_lsp_preferences.py")
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(output)
    print(f"wrote {OUTPUT.relative_to(ROOT)}: {len(schema.defs)} definitions")


if __name__ == "__main__":
    main()
