#!/usr/bin/env python3
"""EF8 output-axis coverage matrix (source/evidence audit, no runtime).

Scans the compiler control fixtures, the H2 qualification artifacts, the
H2.8a candidate matrix (joined to the vendored suite expansions for its
settings) and the PR-gate entry inventory, and writes
``axis-coverage.v1.json`` next to this script.

    python3 axis-matrix.py --write     # regenerate the artifact
    python3 axis-matrix.py --check     # exit 1 when the artifact would change
    python3 axis-matrix.py --summary   # print a compact human summary

Standard library only. Deterministic: every dict is emitted with sorted keys
and every list is sorted. Fixture ``.json.zst`` files are decoded with the
3.14 ``compression.zstd`` module; if that module is missing the script fails
closed instead of producing a different artifact.

Nothing here executes Rust, Node or TypeScript. Counts are counts of recorded
inputs/observations, never of Rust results.
"""

import argparse
import base64
import collections
import glob
import hashlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", "..", ".."))
OUT = os.path.join(HERE, "axis-coverage.v1.json")

COMPILER_FIXTURES = os.path.join(ROOT, "crates", "compiler", "tests", "fixtures")
EMITTER_FIXTURES = os.path.join(ROOT, "crates", "emitter", "tests", "fixtures")
COMPILER_TESTS = os.path.join(ROOT, "crates", "compiler", "tests")
RATCHETS = os.path.join(ROOT, "ratchets")
VENDOR = os.path.join(ROOT, "vendor", "typescript-6.0.3")
INVENTORY_DIR = os.path.join(
    ROOT, "docs", "design", "greenfield", "slices", "witness-coverage"
)

# Acceptance groups from docs/witness-testing.md "Hosted jobs".
ACCEPTANCE_GROUP = {
    "h2-1a": "early", "h2-1b": "early", "h2-1c": "early", "h2-1d": "early",
    "h2-1e": "early", "h2-2a": "early", "h2-2b": "early", "h2-2c": "early",
    "h2-2d": "early", "h2-3a": "early", "h2-3b": "early", "h2-3c": "early",
    "h2-3d": "early", "h2-4a": "early", "h2-4b": "early", "h2-5a": "early",
    "h2-5b": "early", "h2-5c": "early", "h2-5d": "early", "h2-5e": "early",
    "h2-5f": "early", "h2-5g": "wide", "h2-5h": "late", "h2-6a": "late",
    "h2-6b": "late", "h2-6c": "late", "h2-7b": "late", "h2-7c": "late",
    "h2-7de": "late",
}

# Curated: suites whose stated purpose is generated-name collision behaviour.
GENERATED_NAME_SOURCES = {
    "import-helpers.json": "helper import alias collisions",
    "alias-conflict-display.json": "alias conflict display names",
    "transformed-class-assigned-names.json": "transformed class assigned names",
    "system-generated-names.json": "System module generated names",
    "module-alias-underscores.json": "module alias underscore names",
    "bundle-module-identities.json": "bundle module identities",
    "decorator-binding-inputs.json": "decorator generated binding names",
    "static-this-super-auto-accessor-storage-names.json": "auto-accessor storage names",
    "cjs-default-reexport-names.json": "CommonJS default re-export names",
    "export-specifier-names.json": "export specifier names",
    "decorator-name-owners.json": "decorator name owners",
}

MODULE_ENUM = {
    0: "none", 1: "commonjs", 2: "amd", 3: "umd", 4: "system", 5: "es2015",
    6: "es2020", 7: "es2022", 99: "esnext", 100: "node16", 101: "node18", 102: "node20",
    199: "nodenext", 200: "preserve",
}
TARGET_ENUM = {
    0: "es3", 1: "es5", 2: "es2015", 3: "es2016", 4: "es2017", 5: "es2018",
    6: "es2019", 7: "es2020", 8: "es2021", 9: "es2022", 10: "es2023",
    11: "es2024", 99: "esnext", 100: "json",
}
NEWLINE_ENUM = {0: "crlf", 1: "lf"}
MODULE_ALIASES = {"es6": "es2015", "esnext": "esnext", "node16": "node16"}
TARGET_ALIASES = {"es6": "es2015", "es7": "es2016", "latest": "esnext"}

CANONICAL_OPTION = {}
for _name in (
    "sourceMap", "inlineSourceMap", "inlineSources", "emitBOM", "newLine",
    "removeComments", "outFile", "outDir", "rootDir", "declarationDir",
    "declarationMap", "noEmitOnError", "listEmittedFiles",
    "emitDeclarationOnly", "noEmit", "useCaseSensitiveFileNames", "mapRoot",
    "sourceRoot", "module", "target", "declaration", "composite",
    "isolatedModules", "verbatimModuleSyntax", "jsx", "esModuleInterop",
    "importHelpers", "allowJs", "checkJs", "resolveJsonModule",
    "experimentalDecorators", "useDefineForClassFields", "downlevelIteration",
    "moduleDetection", "preserveConstEnums", "stripInternal", "noCheck",
    "noEmitHelpers", "out", "listFiles", "moduleResolution",
):
    CANONICAL_OPTION[_name.lower()] = _name

PATH_OPTIONS = ("outFile", "outDir", "rootDir", "declarationDir", "mapRoot", "sourceRoot", "out")
BOOL_OPTIONS = (
    "emitBOM", "removeComments", "declaration", "declarationMap", "sourceMap",
    "inlineSourceMap", "inlineSources", "noEmitOnError", "listEmittedFiles",
    "emitDeclarationOnly", "noEmit", "useCaseSensitiveFileNames",
    "importHelpers", "composite", "isolatedModules", "verbatimModuleSyntax",
    "allowJs", "checkJs", "resolveJsonModule", "esModuleInterop",
    "experimentalDecorators", "useDefineForClassFields", "downlevelIteration",
    "preserveConstEnums", "stripInternal", "noCheck", "noEmitHelpers",
)

WRITE_KIND = {
    "javascript": "js", "declaration": "d.ts", "source-map": "js.map",
    "declaration-map": "d.ts.map", "mjs": "mjs", "cjs": "cjs",
    "json": "json", "build-info": "tsbuildinfo", "jsx": "jsx",
}

GENERATED_NAME_SHAPE = re.compile(r"(?<![\w$])(_[a-z]|_[a-z]_\d+|__[A-Za-z]+)(?![\w$])")
UNICODE_ESCAPE = re.compile(r"\\u(\{[0-9A-Fa-f]+\}|[0-9A-Fa-f]{4})")


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path):
    if path.endswith(".zst"):
        try:
            from compression import zstd  # Python 3.14
        except ImportError:  # pragma: no cover - fail closed, never drift
            sys.exit("axis-matrix: %s needs compression.zstd (Python >= 3.14)" % path)
        with open(path, "rb") as f:
            return json.loads(zstd.decompress(f.read()))
    with open(path, "rb") as f:
        return json.load(f)


def rel(path):
    return os.path.relpath(path, ROOT).replace(os.sep, "/")


def canonical_name(name):
    if not isinstance(name, str):
        return None
    return CANONICAL_OPTION.get(name.lower(), name)


def norm_enum(name, value):
    if name == "module":
        if isinstance(value, bool):
            return None
        if isinstance(value, int):
            return MODULE_ENUM.get(value, "module#%d" % value)
        v = str(value).strip().lower()
        return MODULE_ALIASES.get(v, v)
    if name == "target":
        if isinstance(value, bool):
            return None
        if isinstance(value, int):
            return TARGET_ENUM.get(value, "target#%d" % value)
        v = str(value).strip().lower()
        return TARGET_ALIASES.get(v, v)
    if name == "newLine":
        if isinstance(value, int) and not isinstance(value, bool):
            return NEWLINE_ENUM.get(value, "newLine#%d" % value)
        return str(value).strip().lower()
    if name in PATH_OPTIONS:
        if value is None:
            return "absent"
        if isinstance(value, str):
            return "set" if value.strip() != "" else "empty"
        return "set"
    if name in BOOL_OPTIONS:
        if isinstance(value, bool):
            return "true" if value else "false"
        v = str(value).strip().lower()
        if v in ("true", "false"):
            return v
        return "other:" + v
    if isinstance(value, (dict, list)):
        return "set"
    return str(value).strip().lower()[:40]


def normalize_options(raw):
    """raw: dict of option->value, or list of {name,value} settings."""
    out = {}
    items = []
    if isinstance(raw, dict):
        items = list(raw.items())
    elif isinstance(raw, list):
        for e in raw:
            if isinstance(e, dict) and "name" in e:
                items.append((e.get("name"), e.get("value")))
    for name, value in items:
        cname = canonical_name(name)
        if cname is None:
            continue
        nv = norm_enum(cname, value)
        if nv is not None:
            out[cname] = nv
    return out


def observation_of(case):
    """The complete-command observation; bundle-sink cases nest it under `call`."""
    obs = case.get("typescript_observation")
    if obs is None:
        obs = case.get("observation")
    if isinstance(obs, dict) and obs.get("writes") is None and isinstance(obs.get("call"), dict):
        merged = dict(obs["call"])
        for k in ("program_source_order", "standard_libraries"):
            if k in obs:
                merged[k] = obs[k]
        return merged
    return obs


def options_of(case, inp):
    """Explicit options merged over config-file compilerOptions (options win)."""
    raw = case.get("options")
    if raw is None:
        raw = inp.get("options") or inp.get("settings") or case.get("settings")
    if raw is None and isinstance(case.get("effective_map_options"), dict):
        raw = case["effective_map_options"]
    merged = {}
    cfg = case.get("config")
    if isinstance(cfg, str):
        try:
            cfg = json.loads(cfg)
        except ValueError:
            cfg = None
    if isinstance(cfg, dict) and isinstance(cfg.get("compilerOptions"), dict):
        merged.update(normalize_options(cfg["compilerOptions"]))
    merged.update(normalize_options(raw))
    return merged


def file_text(entry):
    if not isinstance(entry, dict):
        return None
    if isinstance(entry.get("text"), str):
        return entry["text"]
    for key in ("utf8_base64", "base64", "content_base64", "decoded_base64"):
        v = entry.get(key)
        if isinstance(v, str):
            try:
                return base64.b64decode(v).decode("utf-8", "replace")
            except Exception:
                return None
    return None


def unicode_flags(paths, texts):
    flags = set()
    for p in paths:
        if any(ord(ch) > 127 for ch in p):
            flags.add("non-ascii-path")
    saw_text = False
    for t in texts:
        if t is None:
            continue
        saw_text = True
        if any(ord(ch) > 127 for ch in t):
            flags.add("non-ascii-text")
        if any(ord(ch) > 0xFFFF for ch in t):
            flags.add("non-bmp-text")
        if UNICODE_ESCAPE.search(t):
            flags.add("unicode-escape-text")
    if not saw_text:
        flags.add("text-not-inline")
    elif not flags:
        flags.add("ascii-only")
    return flags


def products_of(writes):
    prods = set()
    bom = False
    callback_meta = set()
    for w in writes or []:
        if not isinstance(w, dict):
            continue
        kind = w.get("kind")
        path = w.get("path") or ""
        prod = WRITE_KIND.get(kind)
        if prod is None:
            low = path.lower()
            if low.endswith(".d.ts.map"):
                prod = "d.ts.map"
            elif low.endswith(".js.map") or low.endswith(".mjs.map") or low.endswith(".cjs.map"):
                prod = "js.map"
            elif low.endswith(".d.ts") or low.endswith(".d.mts") or low.endswith(".d.cts"):
                prod = "d.ts"
            elif low.endswith(".js"):
                prod = "js"
            else:
                prod = "other:%s" % (kind or os.path.splitext(path)[1] or "?")
        prods.add(prod)
        if w.get("write_byte_order_mark") is True:
            bom = True
        if w.get("source_files") is not None:
            callback_meta.add("callback-source-files")
        if w.get("data_present") is not None or "data_source_map_url_pos" in w:
            callback_meta.add("callback-data-metadata")
        if w.get("inline_source_map_payload"):
            prods.add("inline-source-map")
    return prods, bom, callback_meta


def outcome_flags(obs, case):
    flags = set()
    if not isinstance(obs, dict):
        return flags
    if obs.get("emit_refused") is True:
        flags.add("emit-refused")
    er = obs.get("emit_result")
    if isinstance(er, dict):
        if er.get("emit_skipped") is True or er.get("emitSkipped") is True:
            flags.add("emit-skipped")
        if er.get("emitted_files") not in (None, []):
            flags.add("emitted-files-result")
    ec = obs.get("exit_code")
    if isinstance(ec, int) and not isinstance(ec, bool):
        flags.add("exit-%d" % ec)
    if obs.get("status_writes"):
        flags.add("status-writes")
    if obs.get("exception") not in (None, False, ""):
        flags.add("direct-exception")
    fault = case.get("fault") if isinstance(case, dict) else None
    if isinstance(fault, str) and fault != "none":
        flags.add("fs-fault:" + fault)
    for rule in case.get("sink_rules") or [] if isinstance(case, dict) else []:
        if isinstance(rule, dict) and rule.get("action"):
            flags.add("sink:" + str(rule["action"]))
    for w in obs.get("writes") or []:
        if isinstance(w, dict):
            action = w.get("sink_action")
            if isinstance(action, str) and action not in ("write", "written", "ok"):
                flags.add("sink:" + action)
            if w.get("on_error_messages"):
                flags.add("sink:on-error")
    return flags


class Record(object):
    __slots__ = (
        "source", "route", "case_id", "opts", "products", "bom_written",
        "callback_meta", "roots_n", "files_n", "reversed_roots", "unicode",
        "host", "outcome",
    )


def make_record(source, route, case_id, opts, files, roots, obs, case):
    r = Record()
    r.source = source
    r.route = route
    r.case_id = case_id
    r.opts = opts
    writes = obs.get("writes") if isinstance(obs, dict) else None
    r.products, r.bom_written, r.callback_meta = products_of(writes)
    paths = []
    texts = []
    for f in files or []:
        if isinstance(f, dict):
            paths.append(str(f.get("path") or f.get("name") or ""))
            texts.append(file_text(f))
        elif isinstance(f, str):
            paths.append(f)
            texts.append(None)
    r.files_n = len(paths)
    root_paths = []
    for rt in roots or []:
        if isinstance(rt, str):
            root_paths.append(rt)
        elif isinstance(rt, dict):
            root_paths.append(str(rt.get("path") or rt.get("requested") or ""))
    r.roots_n = len(root_paths)
    r.reversed_roots = False
    if len(root_paths) >= 2:
        order = [p for p in paths if p in root_paths]
        if len(order) >= 2 and order != [p for p in root_paths if p in order]:
            r.reversed_roots = True
    r.unicode = unicode_flags(paths + root_paths, texts)
    ucs = None
    if isinstance(case, dict) and "use_case_sensitive_file_names" in case:
        ucs = case["use_case_sensitive_file_names"]
    elif opts.get("useCaseSensitiveFileNames") in ("true", "false"):
        ucs = opts["useCaseSensitiveFileNames"] == "true"
    r.host = "unspecified" if ucs is None else ("case-sensitive" if ucs else "case-insensitive")
    r.outcome = outcome_flags(obs, case)
    if r.bom_written:
        r.callback_meta.add("bom-written")
    return r


# --------------------------------------------------------------------------
# Test-source scan: module -> fixtures, module -> target, hosted mapping.
# --------------------------------------------------------------------------

def resolve_fixture_name(name):
    """Map a spelling found in a test source to an on-disk fixture file name."""
    base = name.split("/")[-1]
    for candidate in (base, base + ".zst"):
        if os.path.exists(os.path.join(COMPILER_FIXTURES, candidate)):
            return candidate
    if os.path.exists(os.path.join(EMITTER_FIXTURES, base)):
        return base
    return None


def fixtures_in(text):
    spelled = re.compile(r'"([A-Za-z0-9_./-]+\.json(?:\.zst)?)"')
    out = set()
    for m in spelled.finditer(text):
        resolved = resolve_fixture_name(m.group(1))
        if resolved is not None:
            out.add(resolved)
    return out


def scan_test_sources():
    path_re = re.compile(r'#\[path\s*=\s*"integration/([a-z0-9_]+)\.rs"\]')
    module_fixtures = {}
    module_target = {}
    for path in sorted(glob.glob(os.path.join(COMPILER_TESTS, "*.rs"))):
        target = os.path.basename(path)[:-3]
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            text = f.read()
        module_target.setdefault(target, set()).add(target)
        for m in path_re.finditer(text):
            module_target.setdefault(m.group(1), set()).add(target)
        fx = fixtures_in(text)
        if fx:
            module_fixtures.setdefault(target, set()).update(fx)
    for path in sorted(glob.glob(os.path.join(COMPILER_TESTS, "integration", "*.rs"))):
        module = os.path.basename(path)[:-3]
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            text = f.read()
        fx = fixtures_in(text)
        if fx:
            module_fixtures.setdefault(module, set()).update(fx)
        module_target.setdefault(module, set())
    return module_fixtures, module_target


def latest_inventory():
    best = None
    for path in glob.glob(os.path.join(INVENTORY_DIR, "inventory.v*.json")):
        m = re.search(r"inventory\.v(\d+)\.json$", path)
        if m:
            v = int(m.group(1))
            if best is None or v > best[0]:
                best = (v, path)
    return best


def hosted_index(inventory):
    """target -> {mode, filters(list of (module or None, fn)), suites}"""
    idx = {}
    for t in inventory.get("targets", []):
        if t.get("package") != "tsc-rs-compiler":
            continue
        filters = []
        suites = set()
        for c in t.get("commands") or []:
            flt = c.get("filter") or ""
            suites.add("%s/%s" % (c.get("group"), c.get("suite")))
            if "::" in flt:
                mod, fn = flt.split("::", 1)
                filters.append((mod, fn))
            else:
                filters.append((None, flt))
        idx[t["target"]] = {
            "mode": t.get("direct_mode"),
            "filters": filters,
            "suites": sorted(suites),
            "acceptance_groups": sorted((t.get("plan_for_target_source_change") or {}).get("acceptance") or []),
        }
    acceptance_modules = {}
    for ref in inventory.get("acceptance_source_references", []):
        acceptance_modules[ref.get("module")] = os.path.basename(ref.get("driver", ""))
    return idx, acceptance_modules


def fixture_hosting(fixture, module_fixtures, module_target, idx, acceptance_modules):
    entries = []
    for module, fxs in sorted(module_fixtures.items()):
        if fixture not in fxs:
            continue
        targets = module_target.get(module) or set()
        if module in acceptance_modules:
            entries.append({"module": module, "via": "acceptance:" + acceptance_modules[module]})
        for target in sorted(targets):
            info = idx.get(target)
            if info is None:
                entries.append({"module": module, "target": target, "via": "not-in-inventory"})
                continue
            if info["mode"] == "unfiltered-target-command":
                entries.append({"module": module, "target": target, "via": "witness:" + ",".join(info["suites"])})
            elif info["mode"] == "filtered-target-command":
                hit = [fn for (mod, fn) in info["filters"] if (mod == module) or (mod is None and module == target)]
                if hit:
                    entries.append({"module": module, "target": target, "via": "witness:" + ",".join(info["suites"]), "filters": sorted(hit)})
                else:
                    entries.append({"module": module, "target": target, "via": "no-hosted-filter"})
            else:
                entries.append({"module": module, "target": target, "via": "no-direct-target-command"})
    hosted = any(not e["via"].startswith("no") for e in entries)
    return hosted, entries


# --------------------------------------------------------------------------
# Loaders
# --------------------------------------------------------------------------

def load_fixture_records(inputs):
    records = []
    sources = {}
    files = sorted(glob.glob(os.path.join(COMPILER_FIXTURES, "*.json")) + glob.glob(os.path.join(COMPILER_FIXTURES, "*.json.zst")))
    # emitter fixtures that compiler suites load by name (checked below).
    for path in files:
        name = os.path.basename(path)
        data = load_json(path)
        inputs.append({"path": rel(path), "sha256": sha256_file(path)})
        cases = data.get("cases") if isinstance(data, dict) else None
        route = "unclassified"
        n = 0
        if isinstance(cases, list) and cases and isinstance(cases[0], dict):
            for c in cases:
                if not isinstance(c, dict):
                    continue
                obs = observation_of(c)
                inp = c.get("input") if isinstance(c.get("input"), dict) else {}
                opts = options_of(c, inp)
                cfiles = c.get("files") if c.get("files") is not None else inp.get("files")
                croots = c.get("roots") if c.get("roots") is not None else inp.get("roots")
                if isinstance(obs, dict) and obs.get("writes") is not None:
                    r_route = "complete-command"
                elif isinstance(obs, dict) and ("js_text" in obs or "calls" in obs):
                    r_route = "direct-or-api"
                elif obs is None and (c.get("options") is not None or c.get("config") is not None or inp):
                    r_route = "inputs-only"
                elif isinstance(obs, dict):
                    r_route = "observation-other"
                else:
                    r_route = "unclassified"
                rec = make_record(name, r_route, str(c.get("case_id") or c.get("id") or ""), opts, cfiles, croots, obs, c)
                records.append(rec)
                n += 1
            routes = collections.Counter(r.route for r in records[-n:]) if n else {}
            route = ",".join("%s:%d" % kv for kv in sorted(routes.items()))
        elif isinstance(data, dict) and data.get("route"):
            route = "manifest:" + str(data.get("route"))
            n = len(cases or [])
        sources[name] = {"kind": "compiler-fixture", "cases": n, "routes": route}
    # emitter fixtures loaded by compiler suites
    return records, sources


def load_emitter_fixture_records(names, inputs):
    records = []
    sources = {}
    for name in sorted(names):
        path = os.path.join(EMITTER_FIXTURES, name)
        if not os.path.exists(path):
            continue
        data = load_json(path)
        inputs.append({"path": rel(path), "sha256": sha256_file(path)})
        cases = data.get("cases") if isinstance(data, dict) else None
        n = 0
        if isinstance(cases, list):
            for c in cases:
                if not isinstance(c, dict):
                    continue
                obs = observation_of(c)
                route = "complete-command" if isinstance(obs, dict) and obs.get("writes") is not None else "direct-or-api"
                rec = make_record(name, route, str(c.get("case_id") or ""), options_of(c, {}), c.get("files"), c.get("roots"), obs, c)
                records.append(rec)
                n += 1
        sources[name] = {"kind": "emitter-fixture-loaded-by-compiler-suite", "cases": n}
    return records, sources


def load_qualification_records(inputs):
    records = []
    sources = {}
    for path in sorted(glob.glob(os.path.join(RATCHETS, "h2-*-qualification.v1.json"))):
        name = os.path.basename(path)
        profile = name[: -len("-qualification.v1.json")]
        data = load_json(path)
        inputs.append({"path": rel(path), "sha256": sha256_file(path)})
        n = 0
        with_writes = 0
        for c in data.get("cases") or []:
            if not isinstance(c, dict):
                continue
            inp = c.get("input") if isinstance(c.get("input"), dict) else {}
            settings = inp.get("settings")
            if settings is None:
                settings = c.get("settings")
            opts = normalize_options(settings)
            facets = c.get("option_facets")
            if isinstance(facets, dict):
                for k, v in facets.items():
                    if isinstance(v, dict) and v.get("state") not in (None, "absent"):
                        cname = canonical_name(k)
                        if cname and cname not in opts:
                            nv = norm_enum(cname, v.get("value"))
                            if nv is not None:
                                opts[cname] = nv
            obs = c.get("typescript_observation")
            if obs is None:
                obs = c.get("observation")
            route = "complete-command" if isinstance(obs, dict) and obs.get("writes") is not None else "settings-only"
            if route == "complete-command":
                with_writes += 1
            rec = make_record(name, route, str(c.get("case_id") or ""), opts, inp.get("files") or c.get("files"), inp.get("roots"), obs, c)
            records.append(rec)
            n += 1
        del data
        sources[name] = {
            "kind": "qualification-artifact",
            "cases": n,
            "cases_with_observed_writes": with_writes,
            "hosted": "acceptance:" + ACCEPTANCE_GROUP.get(profile, "none"),
        }
    return records, sources


def load_candidate_records(inputs):
    """The 769 output-only H2.8a candidates joined to the vendored expansions."""
    path = os.path.join(RATCHETS, "h2-8a-candidates.v1.json")
    data = load_json(path)
    inputs.append({"path": rel(path), "sha256": sha256_file(path)})
    settings_by_id = {}
    for exp_name, fixtures_key in (("test-suite-expansion.v1.json", "compiler_fixtures"), ("conformance-suite-expansion.v1.json", "fixtures")):
        p = os.path.join(VENDOR, exp_name)
        exp = load_json(p)
        inputs.append({"path": rel(p), "sha256": sha256_file(p)})
        fixtures = exp.get(fixtures_key) or []
        for c in exp.get("cases") or []:
            src = c.get("source")
            cfg = c.get("configuration")
            if isinstance(cfg, dict):
                cfg = cfg.get("configuration")
            if not isinstance(src, int) or src >= len(fixtures):
                continue
            fx = fixtures[src]
            merged = list(fx.get("settings") or [])
            cfgs = fx.get("configurations") or []
            if isinstance(cfg, int) and cfg < len(cfgs):
                merged.extend(cfgs[cfg].get("settings") or [])
            paths = [u.get("name") for u in fx.get("normal_units") or [] if isinstance(u, dict)]
            settings_by_id[c.get("id")] = (merged, paths, fx.get("virtual_config") is not None)
        del exp
    p = os.path.join(VENDOR, "project-profile-classification.v1.json")
    proj = load_json(p)
    inputs.append({"path": rel(p), "sha256": sha256_file(p)})
    for c in proj.get("cases") or []:
        mv = c.get("module_variant") or {}
        settings_by_id[c.get("id")] = ([{"name": "module", "value": mv.get("name")}], [], "project-descriptor")
    del proj
    records = []
    joined = collections.Counter()
    for c in data.get("cases") or []:
        rs = c.get("required_slices") or []
        if rs != ["H2.8a"]:
            continue  # only the 769 output-only candidates
        cid = c.get("case_id")
        st = settings_by_id.get(cid)
        opts = {}
        files = []
        if st is not None:
            opts = normalize_options(st[0])
            files = [{"path": pth} for pth in st[1] if isinstance(pth, str)]
            joined["project-descriptor" if st[2] == "project-descriptor" else ("virtual-config" if st[2] else "settings")] += 1
        else:
            joined["unjoined"] += 1
        rec = make_record("h2-8a-candidates.v1.json", "candidate-settings-only", str(cid), opts, files, None, None, c)
        records.append(rec)
    sources = {
        "h2-8a-candidates.v1.json": {
            "kind": "candidate-matrix",
            "cases": len(records),
            "join": dict(sorted(joined.items())),
            "hosted": "none (integrator-hosted full replay planned; h2_8a_original_corpus has no direct target command)",
            "note": "settings joined from vendor suite expansions by case id; project rows carry only their module variant; no per-case typescript_observation in this artifact",
        }
    }
    return records, sources


def load_checkpoints(inputs):
    out = {}
    p = os.path.join(RATCHETS, "h2-8a-global-after-a6-37.v1.json")
    d = load_json(p)
    inputs.append({"path": rel(p), "sha256": sha256_file(p)})
    failures = sorted(set(f.get("case_id") for f in d.get("failures") or [] if isinstance(f, dict)))
    out["global"] = {
        "artifact": rel(p), "revision": d.get("revision"), "eligible": d.get("eligible"),
        "exact": d.get("exact"), "failed": d.get("failed"), "failure_case_ids": failures,
        "remaining_failures_by_suite": d.get("remaining_failures_by_suite"),
        "project_retained_exact": d.get("project_retained_exact"),
        "typed_or_runtime_boundary_case_ids": d.get("typed_or_runtime_boundary_case_ids"),
    }
    p = os.path.join(RATCHETS, "h2-8a-class-convergence-after-a6-34.v1.json")
    d = load_json(p)
    inputs.append({"path": rel(p), "sha256": sha256_file(p)})
    bands = []
    for b in d.get("bands") or []:
        bands.append({"fixture": b.get("fixture"), "eligible": b.get("eligible"), "exact_twice": len(b.get("exact_twice") or []), "failed_once": len(b.get("failed_once") or [])})
    out["class"] = {"artifact": rel(p), "revision": d.get("revision"), "eligible": d.get("eligible"), "exact_twice": d.get("exact_twice"), "failed_once": len(d.get("failed_once") or []), "bands": sorted(bands, key=lambda b: b["fixture"])}
    return out


# --------------------------------------------------------------------------
# Axis extraction
# --------------------------------------------------------------------------

OPTION_AXES = (
    "module", "target", "newLine", "emitBOM", "removeComments", "declaration",
    "declarationMap", "sourceMap", "inlineSourceMap", "inlineSources",
    "mapRoot", "sourceRoot", "outFile", "outDir", "rootDir", "declarationDir",
    "noEmitOnError", "listEmittedFiles", "emitDeclarationOnly", "importHelpers",
    "jsx", "esModuleInterop", "useDefineForClassFields", "moduleDetection",
)


def layout_of(opts):
    of = opts.get("outFile") == "set" or opts.get("out") == "set"
    od = opts.get("outDir") == "set"
    if of and od:
        return "outFile+outDir"
    if of:
        return "outFile"
    if od:
        return "outDir"
    return "neither"


def map_of(opts):
    flags = []
    if opts.get("sourceMap") == "true":
        flags.append("sourceMap")
    if opts.get("declarationMap") == "true":
        flags.append("declarationMap")
    if opts.get("inlineSourceMap") == "true":
        flags.append("inlineSourceMap")
    if opts.get("inlineSources") == "true":
        flags.append("inlineSources")
    return "+".join(flags) if flags else "none"


def axis_values(r):
    vals = {}
    for name in OPTION_AXES:
        vals[name] = [r.opts.get(name, "absent")]
    vals["product"] = sorted(r.products) if r.products else ["no-observed-writes"]
    vals["layout"] = [layout_of(r.opts)]
    vals["maps"] = [map_of(r.opts)]
    roots = []
    if r.roots_n >= 2:
        roots.append("multiple-roots")
    elif r.roots_n == 1:
        roots.append("single-root")
    else:
        roots.append("implicit-roots")
    if r.reversed_roots:
        roots.append("reversed-root-order")
    vals["roots"] = roots
    vals["inputs"] = ["multiple-files" if r.files_n >= 2 else ("single-file" if r.files_n == 1 else "no-inline-files")]
    vals["unicode"] = sorted(r.unicode)
    vals["host"] = [r.host]
    vals["outcome"] = sorted(r.outcome) if r.outcome else ["no-observation"]
    vals["write_metadata"] = sorted(r.callback_meta) if r.callback_meta else ["none"]
    gen = []
    if r.source in GENERATED_NAME_SOURCES:
        gen.append("curated-suite")
    vals["generated_names"] = gen or ["not-curated"]
    return vals


PAIRS = (
    ("module_x_target", "module", "target"),
    ("layout_x_maps", "layout", "maps"),
    ("emitBOM_x_newLine", "emitBOM", "newLine"),
    ("removeComments_x_declaration", "removeComments", "declaration"),
    ("layout_x_product", "layout", "product"),
    ("host_x_layout", "host", "layout"),
)

# (axis, value, required product or None, rationale) — the audit's expected set
REQUIREMENTS = [
    ("module", m, "js", "ordinary module kinds at 6.0.3") for m in
    ("none", "commonjs", "amd", "umd", "system", "es2015", "es2020", "es2022", "esnext", "node16", "node18", "nodenext", "preserve")
] + [
    ("target", t, "js", "ordinary script targets at 6.0.3") for t in
    ("es5", "es2015", "es2016", "es2017", "es2018", "es2019", "es2020", "es2021", "es2022", "es2023", "es2024", "esnext")
] + [
    ("newLine", "crlf", p, "newLine=CRLF per product") for p in ("js", "d.ts", "js.map", "d.ts.map")
] + [
    ("newLine", "lf", p, "newLine=LF per product") for p in ("js", "d.ts", "js.map", "d.ts.map")
] + [
    ("emitBOM", "true", p, "emitBOM per product") for p in ("js", "d.ts", "js.map", "d.ts.map")
] + [
    ("removeComments", "true", p, "removeComments per text product") for p in ("js", "d.ts")
] + [
    ("layout", "outFile", p, "outFile per product") for p in ("js", "d.ts", "js.map", "d.ts.map")
] + [
    ("layout", "outDir", p, "outDir per product") for p in ("js", "d.ts", "js.map", "d.ts.map")
] + [
    ("maps", "inlineSourceMap", "js", "inline maps"),
    ("maps", "inlineSourceMap+inlineSources", "js", "inline maps with sources"),
    ("mapRoot", "set", "js.map", "mapRoot"),
    ("sourceRoot", "set", "js.map", "sourceRoot"),
    ("declarationDir", "set", "d.ts", "declarationDir"),
    ("rootDir", "set", "js", "rootDir"),
    ("roots", "multiple-roots", "js", "multiple roots"),
    ("roots", "reversed-root-order", "js", "reversed root order"),
    ("unicode", "non-ascii-path", "js", "non-ASCII output path"),
    ("unicode", "non-bmp-text", "js", "non-BMP source text"),
    ("unicode", "unicode-escape-text", "js", "unicode escapes in source"),
    ("host", "case-insensitive", "js", "case-insensitive host"),
    ("host", "case-insensitive", "d.ts", "case-insensitive host, declarations"),
    ("outcome", "emit-refused", None, "typed refusal"),
    ("outcome", "emit-skipped", None, "noEmitOnError skip"),
    ("outcome", "fs-fault:first-write", None, "fs fault first write"),
    ("outcome", "fs-fault:create-directory", None, "fs fault create directory"),
    ("outcome", "fs-fault:all-writes", None, "fs fault all writes"),
    ("outcome", "sink:on-error", None, "sink onError"),
    ("outcome", "sink:throw", None, "sink throw"),
    ("outcome", "sink:skip-unchanged", None, "sink skip unchanged"),
    ("outcome", "status-writes", None, "listEmittedFiles status lines"),
    ("write_metadata", "callback-source-files", None, "callback sourceFiles metadata"),
    ("write_metadata", "bom-written", None, "BOM materialized"),
    ("noEmitOnError", "true", "js", "noEmitOnError with emit"),
    ("emitDeclarationOnly", "true", "d.ts", "emitDeclarationOnly"),
    ("importHelpers", "true", "js", "importHelpers"),
]


def build(records, sources_hosted):
    axes = {}
    pairs = {}
    for r in records:
        vals = axis_values(r)
        weight_route = r.route
        for axis, values in vals.items():
            bucket = axes.setdefault(axis, {})
            for v in values:
                cell = bucket.setdefault(v, {"cases": 0, "complete_command_cases": 0, "hosted_cases": 0, "sources": {}})
                cell["cases"] += 1
                if weight_route == "complete-command":
                    cell["complete_command_cases"] += 1
                    if sources_hosted.get(r.source):
                        cell["hosted_cases"] += 1
                cell["sources"][r.source] = cell["sources"].get(r.source, 0) + 1
        for pname, a, b in PAIRS:
            bucket = pairs.setdefault(pname, {})
            for va in vals[a]:
                for vb in vals[b]:
                    key = "%s|%s" % (va, vb)
                    cell = bucket.setdefault(key, {"cases": 0, "complete_command_cases": 0, "hosted_cases": 0, "sources": {}})
                    cell["cases"] += 1
                    if weight_route == "complete-command":
                        cell["complete_command_cases"] += 1
                        if sources_hosted.get(r.source):
                            cell["hosted_cases"] += 1
                    cell["sources"][r.source] = cell["sources"].get(r.source, 0) + 1
    return axes, pairs


def requirements(records, sources_hosted):
    rows = []
    for axis, value, product, why in REQUIREMENTS:
        total = 0
        complete = 0
        hosted = 0
        srcs = collections.Counter()
        for r in records:
            vals = axis_values(r)
            if value not in vals.get(axis, []):
                continue
            if product is not None and product not in r.products:
                continue
            total += 1
            srcs[r.source] += 1
            if r.route == "complete-command":
                complete += 1
                if sources_hosted.get(r.source):
                    hosted += 1
        status = "uncovered" if complete == 0 else ("covered-unhosted" if hosted == 0 else "covered-hosted")
        rows.append({"axis": axis, "value": value, "product": product, "why": why, "cases": total, "complete_command_cases": complete, "hosted_cases": hosted, "status": status, "sources": dict(sorted(srcs.items()))})
    return rows


def generate():
    inputs = []
    module_fixtures, module_target = scan_test_sources()
    inv = latest_inventory()
    inventory = load_json(inv[1])
    inputs.append({"path": rel(inv[1]), "sha256": sha256_file(inv[1])})
    idx, acceptance_modules = hosted_index(inventory)

    fixture_records, fixture_sources = load_fixture_records(inputs)
    referenced = set()
    for fxs in module_fixtures.values():
        referenced.update(fxs)
    emitter_names = {n for n in referenced if not os.path.exists(os.path.join(COMPILER_FIXTURES, n)) and os.path.exists(os.path.join(EMITTER_FIXTURES, n))}
    emitter_records, emitter_sources = load_emitter_fixture_records(emitter_names, inputs)
    qual_records, qual_sources = load_qualification_records(inputs)
    cand_records, cand_sources = load_candidate_records(inputs)
    checkpoints = load_checkpoints(inputs)

    sources = {}
    sources_hosted = {}
    for name, meta in list(fixture_sources.items()) + list(emitter_sources.items()):
        hosted, entries = fixture_hosting(name, module_fixtures, module_target, idx, acceptance_modules)
        meta = dict(meta)
        meta["loaded_by"] = sorted(set(m for m, fxs in module_fixtures.items() if name in fxs))
        meta["hosted"] = hosted
        meta["hosting"] = sorted(entries, key=lambda e: json.dumps(e, sort_keys=True))
        if name in GENERATED_NAME_SOURCES:
            meta["generated_names"] = GENERATED_NAME_SOURCES[name]
        sources[name] = meta
        sources_hosted[name] = hosted
    for name, meta in qual_sources.items():
        sources[name] = meta
        sources_hosted[name] = meta["hosted"] != "acceptance:none"
    for name, meta in cand_sources.items():
        sources[name] = meta
        sources_hosted[name] = False

    records = fixture_records + emitter_records + qual_records + cand_records
    axes, pairs = build(records, sources_hosted)
    reqs = requirements(records, sources_hosted)
    route_counts = collections.Counter(r.route for r in records)
    unreferenced = sorted(n for n in fixture_sources if not any(n in fxs for fxs in module_fixtures.values()))

    return {
        "schema": "ef8-axis-coverage",
        "version": 1,
        "generator": "docs/design/greenfield/slices/emitter-final-batch/ef8/axis-matrix.py",
        "scope": "source/evidence audit of recorded inputs and TypeScript observations; no Rust, Node or TypeScript executed; counts are recorded cases, not Rust results",
        "inventory": rel(inv[1]),
        "inputs": sorted(inputs, key=lambda e: e["path"]),
        "summary": {
            "records": len(records),
            "by_route": dict(sorted(route_counts.items())),
            "sources": len(sources),
            "compiler_fixtures_unreferenced_by_any_test": unreferenced,
            "requirements": collections.Counter(r["status"] for r in reqs),
        },
        "sources": dict(sorted(sources.items())),
        "axes": {a: dict(sorted(v.items())) for a, v in sorted(axes.items())},
        "pairs": {p: dict(sorted(v.items())) for p, v in sorted(pairs.items())},
        "requirements": reqs,
        "checkpoints": checkpoints,
        "api_boundary_known_rows": {
            "disposed_print_typed_known_2": [
                "decorator-binding-direct/lifecycle/dispose/numbered#direct#after_dispose",
                "decorator-binding-direct/lifecycle/dispose/file-level#direct#after_dispose",
            ],
            "disposed_print_source": "crates/emitter/tests/decorator_binding_contract.rs KNOWN_DIVERGENCES (fixture crates/emitter/tests/fixtures/decorator-binding-direct.json, route direct-generated-name-controls)",
            "shared_node_super_known_4": [
                "decorator-super-direct/es2015/set/shared-node-used-twice",
                "decorator-super-direct/es2015/define/shared-node-used-twice",
                "decorator-super-direct/es2022/set/shared-node-used-twice",
                "decorator-super-direct/es2022/define/shared-node-used-twice",
            ],
            "shared_node_source": "crates/emitter/tests/decorator_super_direct_contract.rs (fixture crates/emitter/tests/fixtures/decorator-super-direct.json, route direct-transform-custom-before)",
        },
    }


def dump(doc):
    return json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"


def summary(doc):
    print("records:", doc["summary"]["records"], "routes:", doc["summary"]["by_route"])
    print("requirements:", dict(doc["summary"]["requirements"]))
    for row in doc["requirements"]:
        if row["status"] != "covered-hosted":
            print("  %-16s %-28s %-9s %-16s cases=%d complete=%d hosted=%d %s" % (row["status"], row["axis"] + "=" + row["value"], str(row["product"]), "", row["cases"], row["complete_command_cases"], row["hosted_cases"], ",".join(sorted(row["sources"]))[:80]))
    print("sources not hosted (complete-command fixtures):")
    for name, meta in doc["sources"].items():
        if meta.get("kind", "").endswith("fixture") or "fixture" in meta.get("kind", ""):
            if not meta.get("hosted") and "complete-command" in str(meta.get("routes", "")):
                print("  ", name, meta.get("routes"), "loaded_by=", meta.get("loaded_by"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--summary", action="store_true")
    args = ap.parse_args()
    if not (args.write or args.check or args.summary):
        ap.error("pass --write, --check or --summary")
    doc = generate()
    text = dump(doc)
    if args.write:
        with open(OUT, "w", encoding="utf-8") as f:
            f.write(text)
        print("wrote", rel(OUT), len(text), "bytes")
    if args.check:
        if not os.path.exists(OUT):
            sys.exit("axis-matrix: %s missing; run --write" % rel(OUT))
        with open(OUT, "r", encoding="utf-8") as f:
            current = f.read()
        if current != text:
            sys.exit("axis-matrix: %s is stale; run --write" % rel(OUT))
        print("axis-matrix: up to date")
    if args.summary:
        summary(doc)


if __name__ == "__main__":
    main()
