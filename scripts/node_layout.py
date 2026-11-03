#!/usr/bin/env python3
"""Report each node kind's layout today and a proposed 32-byte placement.

usage: node_layout.py [--counts FILE] [--counts-full FILE] [--report FILE] [--proposal FILE]

Reads the field schema and today's layout (crates/syntax/nodes.schema.json,
nodes.layout.json, through scripts/generate_nodes.py) and the JSDoc hosts of
the parser (crates/syntax/src/parser.rs: can_have_jsdoc minus the kinds
parser_calls_with_jsdoc_for_kind excludes), and prints, for every kind:

- today: the payload fields with their sizes (ids 4, bool 1, SyntaxKind 2,
  String / JsString 24, EscapedName 4, Option<JSDocComment> est. 40), the
  payload size and whether NodeData boxes it;
- proposed (a 32-byte record: a 20-byte header and three 4-byte payload
  words): every text becomes a 4-byte text id, the bool and SyntaxKind
  fields share one word, a JSDocComment goes to the tail. A kind of at most
  three words is inline; a larger one keeps three hot fields inline and
  moves the rest to the tail only when one of them is present ([tail, hot0,
  hot1] then, the tail holding hot2 and the cold fields). js_doc takes a
  spare word when the kind has at most two words, else the tail.

The hot fields (M1's rule): the required fields first, in schema order, then
the optional fields by their presence on the counted program (schema order
among equals and without counts). The fixed preference list below is kept as
the comparison.

--counts / --counts-full: kind counts and field-presence masks of one
program (an analysis-only count over TypeScript's parser, with the command
line's JSDoc mode and with ParseAll), for the hot-field order and the record
and tail estimates.
--report writes the markdown report, --proposal the placement as JSON.
"""

import argparse
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import generate_nodes  # noqa: E402

ROOT = generate_nodes.ROOT
PARSER = ROOT / "crates/syntax/src/parser.rs"

# The comparison rule: the fields kept inline first, in this order, when a
# kind needs more than three words.
HOT_PREFERENCE = [
    "name", "expression", "left", "right", "initializer", "type", "body", "statements", "members",
    "elements", "parameters", "arguments", "type_arguments", "declarations", "declaration_list",
    "then_statement", "else_statement", "condition", "operand", "tag", "template",
]
HEADER_BYTES = 20
RECORD_BYTES = 32
WORD = 4
COMMENT_TAIL_WORDS = 4  # JSDocComment::Text: text id, pos, end, text_end (Nodes: one array id)
MIB = 1024 * 1024

# tsc-rs's own VS Code numbers (TSRS_MEMORY_REPORT, node-report.md §2): the
# twelve most common kinds and the total.
VSCODE_TOTAL = 17_193_142
VSCODE_RECORD_BYTES_TODAY = 48
VSCODE_TOP12 = {
    "Identifier": 5_945_718,
    "PropertyAccessExpression": 1_532_717,
    "CallExpression": 989_643,
    "StringLiteral": 750_361,
    "PropertyAssignment": 552_590,
    "ExpressionStatement": 503_659,
    "TypeReference": 398_655,
    "Block": 345_699,
    "VariableDeclaration": 310_817,
    "VariableDeclarationList": 306_329,
    "BinaryExpression": 299_445,
    "ThisKeyword": 296_529,
}


def plain(name):
    return name[2:] if name.startswith("r#") else name


def jsdoc_hosts():
    """The kinds the parser attaches JSDoc to (parser_calls_with_jsdoc_for_kind)."""
    text = PARSER.read_text()

    def kinds_in(function):
        body = re.search(rf"fn {function}\(kind: SyntaxKind\) -> bool \{{(.*?)\n\}}", text, re.S)
        if not body:
            raise SystemExit(f"parser.rs: {function} not found")
        return body.group(1)

    hosts = set(re.findall(r"SyntaxKind::(\w+)", kinds_in("can_have_jsdoc")))
    excluded = set(re.findall(r"SyntaxKind::(\w+)", kinds_in("parser_calls_with_jsdoc_for_kind")))
    return hosts, hosts - excluded


class Slot:
    """One proposed payload word: a field, or the packed scalar word."""

    def __init__(self, name, fields, required):
        self.name = name
        self.fields = fields
        self.required = required


def placement(kind, hosts, counts, rule):
    """The proposed placement of `kind` under `rule` ("m1" or "preference"):
    its words (slots), the inline or hot / cold split, the tail-only fields
    and the js_doc place."""
    slots, scalars, comments = [], [], []
    for field in kind.fields:
        if field.ty == "JSDocComment":
            comments.append(field)
        elif field.ty in ("Bool", "SyntaxKind"):
            scalars.append(field)
        else:
            slots.append(Slot(plain(field.rust_name), [field], not field.optional))
    if scalars:
        names = "+".join(plain(field.rust_name) for field in scalars)
        slots.append(Slot(f"[{names}]", scalars, any(f.ty == "SyntaxKind" and not f.optional for f in scalars)))
    host = kind.name in hosts
    words = len(slots)
    if words <= 3:
        hot, cold = slots, []
    else:
        if rule == "preference":
            preferred = sorted(
                (slot for slot in slots if slot.name in HOT_PREFERENCE), key=lambda s: HOT_PREFERENCE.index(s.name)
            )
            rest = [slot for slot in slots if slot not in preferred]
            order = preferred + [s for s in rest if s.required] + [s for s in rest if not s.required]
        else:
            order = sorted(slots, key=m1_order(kind, counts))
        hot = order[:3]
        cold = [slot for slot in slots if slot not in hot]
        hot = [slot for slot in slots if slot in hot]
    js_doc = None
    if host:
        js_doc = "spare word" if words <= 2 else "tail"
    tail_words = max(0, words - 2) + (1 if js_doc == "tail" else 0) + COMMENT_TAIL_WORDS * len(comments)
    return {
        "slots": slots, "hot": hot, "cold": cold, "comments": comments, "words": words,
        "inline": words <= 3, "js_doc": js_doc, "tail_words": tail_words,
    }


def needs_tail(kind, place, present, has_js_doc):
    """Whether a node with these present fields needs a tail."""
    if any(present.get(field.ts_name) for field in place["comments"]):
        return True
    if has_js_doc and place["js_doc"] == "tail":
        return True
    return any(any(present.get(field.ts_name) for field in slot.fields) for slot in place["cold"])


def tail_estimate(kind, place, counts):
    """(nodes with a tail, tail bytes) over a counted program."""
    entry = counts.get(kind.name) if counts else None
    if not entry:
        return 0, 0
    nodes = 0
    for mask, count in entry["masks"].items():
        present = {field.ts_name: mask[index] == "1" for index, field in enumerate(kind.fields)}
        if needs_tail(kind, place, present, mask[-1] == "1"):
            nodes += count
    return nodes, nodes * place["tail_words"] * WORD


def m1_order(kind, counts):
    """M1's hot-field order: required slots in schema order, then optional
    slots by presence on the counted program, schema order among equals. A
    slot is required when its field is required in the schema (the scalar
    word: when it holds a required SyntaxKind, which is never zero)."""
    fields = (counts or {}).get(kind.name, {}).get("fields", {})

    def key(slot):
        index = kind.fields.index(slot.fields[0])
        if slot.required:
            return (0, 0, index)
        present = max((fields.get(field.ts_name, 0) for field in slot.fields), default=0)
        return (1, -present, index)

    return key


def field_text(field):
    size, _ = generate_nodes.field_size(field)
    est = " est." if field.ty == "JSDocComment" else ""
    return f"{plain(field.rust_name)} {size}{est}"


def load_counts(path):
    if not path:
        return None, None
    data = json.loads(Path(path).read_text())
    return data["kinds"], data


def size_classes(kinds):
    classes = {"0": 0, "1-4": 0, "5-8": 0, "9-12": 0, "13-16": 0, "17-20": 0, ">20": 0}
    for kind in kinds:
        size, _ = generate_nodes.payload_size(kind)
        for label, low, high in (("0", 0, 0), ("1-4", 1, 4), ("5-8", 5, 8), ("9-12", 9, 12),
                                 ("13-16", 13, 16), ("17-20", 17, 20), (">20", 21, 1 << 30)):
            if low <= size <= high:
                classes[label] += 1
    return classes


def unused_today(kind, layout):
    """NodeData bytes a node of this kind leaves unused today (tag counted as used)."""
    data_bytes = layout.record["nodeDataBytes"]
    if kind is None:
        return data_bytes - 1
    if kind.boxed:
        return data_bytes - 1 - 8
    size, _ = generate_nodes.payload_size(kind)
    return data_bytes - 1 - size


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--counts")
    parser.add_argument("--counts-full")
    parser.add_argument("--report")
    parser.add_argument("--proposal")
    args = parser.parse_args()

    kinds, layout = generate_nodes.load()
    by_name = {kind.name: kind for kind in kinds}
    can_have, hosts = jsdoc_hosts()
    counts, counts_meta = load_counts(args.counts)
    full, full_meta = load_counts(args.counts_full)

    places = {kind.name: placement(kind, hosts, counts, "m1") for kind in kinds}
    by_preference = {kind.name: placement(kind, hosts, counts, "preference") for kind in kinds}

    out = []
    w = out.append
    w("# Node layout: today's records and a 32-byte placement per kind")
    w("")
    w("Generated by `scripts/node_layout.py` from `crates/syntax/nodes.schema.json`, `crates/syntax/nodes.layout.json`")
    w("and the parser's JSDoc hosts (`crates/syntax/src/parser.rs`: `can_have_jsdoc` minus the kinds")
    w("`parser_calls_with_jsdoc_for_kind` excludes). Field sizes today: ids and `Option<Id>` 4, bool 1, `SyntaxKind` 2,")
    w("`String` / `JsString` 24, `EscapedName` 4, `Option<JSDocComment>` est. 40; a payload is the sum of its fields rounded")
    w("up to its alignment (rustc reorders fields, and every size is a multiple of its alignment, so there is no inner")
    w("padding). Proposed: a 32 B record = 20 B header (kind, literal_flags, flags, pos, end, parent) + three 4-byte words;")
    w("every text is a 4-byte text id, the bool / SyntaxKind fields of a kind share one word, a `JSDocComment` is tail-only")
    w(f"({COMMENT_TAIL_WORDS} words: text id, pos, end, text_end), a kind of at most three words is inline, a larger one keeps")
    w("three hot fields inline and spills to a fixed-shape tail (`[tail, hot0, hot1]` + tail `[hot2, cold..., js_doc?,")
    w("comment?]`) only when a cold field, a tail-placed js_doc or a comment is present; js_doc takes a spare inline word")
    w("when the kind has at most two words, else a tail word.")
    w("")
    w("**Hot-field rule (M1):** the required fields first, in schema order, then the optional fields by their presence")
    w("on VS Code (schema order among equals and for kinds VS Code lacks); the scalar word counts as required when it holds")
    w("a required `SyntaxKind`. ConditionalExpression and ConditionalType have more than three required words and always")
    w("carry a tail (accepted). The fixed preference list (name, expression, left, right, initializer, type, body, …) is")
    w("kept as the comparison row.")
    w("")
    if counts_meta:
        w(f"VS Code counts: TypeScript {counts_meta['typescript']}'s parser over `{Path(counts_meta['config']).name}`")
        w(f"({counts_meta['files']:,} files, {counts_meta['total']:,} nodes, JSDoc mode {counts_meta['jsdocMode']}; tsc-rs's")
        w(f"memory report counts {VSCODE_TOTAL:,}, {VSCODE_TOTAL / counts_meta['total']:.3f}×: speculation garbage, the 7.1")
        w("tree's differences), from `node-layout/count_kinds.js` next to this report, with per-node field-presence masks;")
        w("the counts are in `node-layout/vscode-counts.json` and `vscode-counts-parseall.json`. `count_kinds.js` is an")
        w("analysis-only script (Node and the bench corpus's own TypeScript package; not in the repository, no part of any")
        w("build or check). The twelve `memreport` counts are tsc-rs's own (node-report.md §2).")
        if full_meta:
            w(f"The ParseAll run (the language service's mode) has {full_meta['total']:,} nodes.")
        w("")

    headline_at = len(out)
    headline = []

    # ---- claims
    classes = size_classes(kinds)
    fit12 = sum(1 for kind in kinds if generate_nodes.payload_size(kind)[0] <= 12)
    boxed = sum(1 for kind in kinds if kind.boxed)
    at20 = sorted(kind.name for kind in kinds if generate_nodes.payload_size(kind)[0] == 20)
    unused = {name: unused_today(by_name.get(name), layout) for name in VSCODE_TOP12}
    unused_bytes = sum(unused[name] * count for name, count in VSCODE_TOP12.items())
    w("## 1. node-report.md's claims, checked")
    w("")
    w(f"- Payload size classes today (195 kinds): {', '.join(f'{k} B: {v}' for k, v in classes.items())}.")
    w(f"  **{fit12} of {len(kinds)} payloads fit in 12 B** (plus `Token`): the claim \"118 of 195\" holds.")
    w(f"- Boxed today: {boxed} kinds (claim 52). Payloads of exactly 20 B: {len(at20)} ({', '.join(at20)}).")
    w("- Unused `NodeData` bytes in the twelve most common kinds (24 B minus the 1 B tag and the inline payload, or minus")
    w("  tag and pointer for a boxed payload, i.e. padding counted as unused): "
      + ", ".join(f"{name} {unused[name]} B" for name in VSCODE_TOP12) + ".")
    unused_strict = sum(
        max(0, unused[name] - (7 if by_name.get(name) and by_name[name].boxed else 3)) * count
        for name, count in VSCODE_TOP12.items()
    )
    w(f"  × the memreport counts = {unused_bytes:,} B = **{unused_bytes / MIB:.1f} MiB**: the claim \"181 MiB\" holds")
    w(f"  (it counts alignment padding as unused; without the 3 B / 7 B of tag padding the figure is {unused_strict / MIB:.1f} MiB).")
    inline_kinds = sum(1 for kind in kinds if places[kind.name]["inline"])
    w(f"- Proposed placement: **{inline_kinds} of {len(kinds)} kinds are inline** (at most three words; all tokens too);"
      f" {len(kinds) - inline_kinds} need hot / cold.")
    word_hist = {}
    for kind in kinds:
        word_hist[places[kind.name]["words"]] = word_hist.get(places[kind.name]["words"], 0) + 1
    w("  Words per kind: " + ", ".join(f"{k}: {word_hist[k]}" for k in sorted(word_hist)) + ".")
    spare = sum(1 for kind in kinds if places[kind.name]["js_doc"] == "spare word")
    tailed = sum(1 for kind in kinds if places[kind.name]["js_doc"] == "tail")
    token_hosts = sorted(host for host in hosts if host not in by_name)
    w(f"- JSDoc hosts (parser): {len(hosts)} kinds ({len(can_have)} in `can_have_jsdoc`); js_doc in a spare word:"
      f" {spare + len(token_hosts)} ({len(token_hosts)} of them payload-less tokens: {', '.join(token_hosts)}),"
      f" in the tail: {tailed}.")
    always = sorted(
        kind.name for kind in kinds
        if not places[kind.name]["inline"] and sum(1 for slot in places[kind.name]["slots"] if slot.required) > 3
    )
    w(f"- Kinds with more than three required words, so a tail on every node whatever the hot choice: {len(always)}"
      f" ({', '.join(always)}).")
    preferred_always = sorted(
        kind.name for kind in kinds
        if any(slot.required for slot in by_preference[kind.name]["cold"]) and kind.name not in always
    )
    w(f"- Comparison: kinds where the preference list leaves a required field cold (a tail on every node, which the M1"
      f" rule avoids): {', '.join(preferred_always)}.")
    w("")

    # ---- estimates
    w("## 2. VS Code estimate")
    w("")
    records = VSCODE_TOTAL * RECORD_BYTES
    today = VSCODE_TOTAL * VSCODE_RECORD_BYTES_TODAY
    w(f"- Records: {VSCODE_TOTAL:,} × {RECORD_BYTES} B = {records / MIB:.1f} MiB (today × 48 B = {today / MIB:.1f} MiB;"
      f" {(records - today) / MIB:+.1f} MiB). Exact, given the count.")
    if counts:
        scale = VSCODE_TOTAL / counts_meta["total"]
        rows = []
        for label, plan, source in (
            ("**M1 rule** (required first, then occupancy), CLI JSDoc mode", places, counts),
            ("**M1 rule**, ParseAll (language service)", places, full),
            ("comparison: preference list, CLI JSDoc mode", by_preference, counts),
        ):
            if not source:
                continue
            nodes = total = 0
            for kind in kinds:
                n, b = tail_estimate(kind, plan[kind.name], source)
                nodes += n
                total += b
            rows.append((label, nodes, total))
        always_nodes = always_bytes = 0
        for kind in kinds:
            place = places[kind.name]
            entry = counts.get(kind.name)
            if entry and not place["inline"]:
                always_nodes += entry["count"]
                always_bytes += entry["count"] * place["tail_words"] * WORD
        rows.append(("always spill the non-hot fields (simple rule, CLI)", always_nodes, always_bytes))
        w("- Tails (TypeScript parse counts; scaled by the memreport ratio in the last column):")
        w("")
        w("| rule | nodes with a tail | tail MiB | scaled MiB | records + tails, scaled MiB |")
        w("|---|---:|---:|---:|---:|")
        for label, nodes, total in rows:
            w(f"| {label} | {nodes:,} | {total / MIB:.1f} | {total * scale / MIB:.1f} |"
              f" {(records + total * scale) / MIB:.1f} |")
        w("")
        w(f"  Against today's 48 B records ({today / MIB:.1f} MiB, plus the uncounted `Box` heap) the CLI saving under the M1"
          f" rule is ≈ {(today - records - rows[0][2] * scale) / MIB:.0f} MiB before the `Box` heap (≈"
          f" {(today - records - rows[2][2] * scale) / MIB:.0f} MiB with the preference list).")
        headline.append(
            f"- Records {records / MIB:.1f} MiB (−{(today - records) / MIB:.1f}); tails under the M1 rule"
            f" {rows[0][2] * scale / MIB:.1f} MiB in the CLI's JSDoc mode and {rows[1][2] * scale / MIB:.1f} MiB in ParseAll,"
            f" against {rows[2][2] * scale / MIB:.1f} MiB with the preference list and {rows[3][2] * scale / MIB:.1f} MiB if"
            " every non-hot field always spilled (VS Code, scaled to tsc-rs's count)."
        )
        headline.append(
            "- The preference list would leave a required field cold in "
            + ", ".join(preferred_always)
            + " (every node then has a tail) and pick TypeParameter's error-recovery `expression`; the M1 rule avoids both."
            " ConditionalExpression and ConditionalType have more than three required words and always carry a tail."
        )
        w("  node-report.md §7.6 estimated 5–23 MiB of tails under the rare-tail rule (top twelve ≈ 0) and 31–46 MiB under")
        w("  the simple rule; both ranges hold on these counts.")
        w("")
        w("The twelve most common kinds (memreport counts; today's used bytes of the 48 B record = 24 header + 1 tag +")
        w("inline payload, or + 8 for a `Box` pointer):")
        w("")
        w("| kind | memreport | TS parse | used / 48 today | words | placement | tail nodes (CLI) | of which js_doc |")
        w("|---|---:|---:|---:|---:|---|---:|---:|")
        for name, memcount in VSCODE_TOP12.items():
            kind = by_name.get(name)
            entry = counts.get(name, {})
            if kind is None:
                w(f"| {name} (Token) | {memcount:,} | {entry.get('count', 0):,} | 25 | 0 | inline | 0 | 0 |")
                continue
            place = places[name]
            used = 24 + 1 + (8 if kind.boxed else generate_nodes.payload_size(kind)[0])
            where = "inline" if place["inline"] else ", ".join(s.name for s in place["hot"]) + " \\| " + ", ".join(
                s.name for s in place["cold"])
            n, _ = tail_estimate(kind, place, counts)
            doc_only = sum(c for m, c in entry.get("masks", {}).items() if m[-1] == "1") if place["js_doc"] == "tail" else 0
            w(f"| {name} | {memcount:,} | {entry.get('count', 0):,} | {used} | {place['words']} | {where} | {n:,} |"
              f" {doc_only:,} |")
        w("")
        w("Kinds with the largest tail bytes under the M1 rule (CLI mode), with the preference list for comparison:")
        w("")
        w("| kind | count | words | hot (M1) | tail nodes | tail KiB | hot (preference list) | tail nodes | tail KiB |")
        w("|---|---:|---:|---|---:|---:|---|---:|---:|")
        ranked = []
        for kind in kinds:
            n1, b1 = tail_estimate(kind, places[kind.name], counts)
            n2, b2 = tail_estimate(kind, by_preference[kind.name], counts)
            if b1 or b2:
                ranked.append((max(b1, b2), b1, kind, n1, n2, b2))
        ranked.sort(key=lambda row: (-row[1], -row[0]))
        for _, b1, kind, n1, n2, b2 in ranked[:25]:
            count = counts.get(kind.name, {}).get("count", 0)
            hot1 = ", ".join(slot.name for slot in places[kind.name]["hot"]) if not places[kind.name]["inline"] else "inline"
            hot2 = ", ".join(slot.name for slot in by_preference[kind.name]["hot"]) if not by_preference[kind.name]["inline"] else "inline"
            w(f"| {kind.name} | {count:,} | {places[kind.name]['words']} | {hot1} | {n1:,} | {b1 / 1024:.0f} |"
              f" {hot2} | {n2:,} | {b2 / 1024:.0f} |")
        w("")

    # ---- per kind
    w("## 3. Per kind")
    w("")
    w("Columns: VS Code count (TypeScript parse; memreport count in brackets for the top twelve); today's fields with")
    w("their bytes, the payload size and whether it is boxed; proposed words; placement (`inline`, or `hot | cold`, the")
    w("cold fields spilling with a tail); js_doc; comment fields (tail-only); tail words when a tail exists; VS Code")
    w("nodes with a tail under the M1 rule (CLI mode).")
    w("")
    w("| kind | VS Code | fields today (B) | payload B | boxed | words | placement | js_doc | tail-only | tail words | tail nodes |")
    w("|---|---:|---|---:|---|---:|---|---|---|---:|---:|")
    token_count = 0
    if counts:
        token_count = sum(entry["count"] for name, entry in counts.items() if name not in by_name)
    w(f"| Token (all kinds without a payload) | {token_count:,} [ThisKeyword {VSCODE_TOP12['ThisKeyword']:,}] | — | 0 | no | 0 | inline | — | — | 0 | 0 |")
    proposal = {
        "record": {"bytes": RECORD_BYTES, "headerBytes": HEADER_BYTES, "words": 3},
        "hotRule": "required fields first in schema order, then optional fields by VS Code presence"
        " (schema order among equals); the scalar word is required when it holds a required SyntaxKind",
        "counts": counts_meta and {k: counts_meta[k] for k in ("typescript", "config", "files", "total", "jsdocMode")},
        "kinds": [],
    }
    for kind in kinds:
        place = places[kind.name]
        size, _ = generate_nodes.payload_size(kind)
        count = counts.get(kind.name, {}).get("count", 0) if counts else 0
        memreport = f" [{VSCODE_TOP12[kind.name]:,}]" if kind.name in VSCODE_TOP12 else ""
        fields = ", ".join(field_text(field) for field in kind.fields) or "—"
        if place["inline"]:
            where = "inline: " + (", ".join(slot.name for slot in place["slots"]) or "—")
        else:
            where = ", ".join(s.name for s in place["hot"]) + " \\| " + ", ".join(s.name for s in place["cold"])
        tail_only = ", ".join(plain(field.rust_name) for field in place["comments"]) or "—"
        n, _ = tail_estimate(kind, place, counts)
        w(f"| {kind.name} | {count:,}{memreport} | {fields} | {size} | {'yes' if kind.boxed else 'no'} |"
          f" {place['words']} | {where} | {place['js_doc'] or '—'} | {tail_only} |"
          f" {place['tail_words'] if (not place['inline'] or place['js_doc'] == 'tail' or place['comments']) else 0} | {n:,} |")
        preference = by_preference[kind.name]
        proposal["kinds"].append({
            "kind": kind.name,
            "todayPayloadBytes": size,
            "todayBoxed": kind.boxed,
            "words": place["words"],
            "inline": place["inline"],
            "slots": [{"name": slot.name, "fields": [f.ts_name for f in slot.fields], "required": slot.required}
                      for slot in place["slots"]],
            "hot": [slot.name for slot in place["hot"]],
            "cold": [slot.name for slot in place["cold"]],
            "hotByPreferenceList": [slot.name for slot in preference["hot"]],
            "tailOnly": [field.ts_name for field in place["comments"]],
            "jsDoc": place["js_doc"],
            "tailWords": place["tail_words"],
            "vscode": {
                "count": count,
                "tailNodes": n,
                "tailNodesByPreferenceList": tail_estimate(kind, preference, counts)[0],
            },
        })
    w("")
    headline = [
        "## 0. Headline",
        "",
        f"- node-report.md's \"118 of 195 payloads fit in 12 B\" ({fit12}) and \"the 12 most common kinds leave 181 MiB"
        f" unused\" ({unused_bytes / MIB:.1f} MiB, padding counted as unused; {unused_strict / MIB:.1f} MiB without it) hold.",
        f"- {inline_kinds} of {len(kinds)} kinds (and every token) fit three words once texts are ids; every one of the twelve"
        " most common kinds is inline or needs a tail on under 0.2% of its nodes.",
    ] + headline + [""]
    out[headline_at:headline_at] = headline
    text = "\n".join(out) + "\n"
    if args.report:
        Path(args.report).write_text(text)
    else:
        sys.stdout.write(text)
    if args.proposal:
        Path(args.proposal).write_text(json.dumps(proposal, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
