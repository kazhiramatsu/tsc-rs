#!/usr/bin/env python3
"""Parallel golden classify.

Changed fixtures are expanded with the same harness directives that
`tsrs --check-batch` understands, then classified against the tsc oracle.
The comparison scope intentionally stays at the historical `main.ts` /
`main.tsx` diagnostics gate.
"""
import atexit, json, os, select, sys, subprocess, time
from collections import Counter
from concurrent.futures import ProcessPoolExecutor, as_completed

def default_root():
    if "TSRS_ROOT" in os.environ:
        return os.environ["TSRS_ROOT"]
    cwd = os.getcwd()
    if os.path.exists(os.path.join(cwd, "difftest", "diag_oracle.js")):
        return cwd
    script_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    if os.path.exists(os.path.join(script_root, "difftest", "diag_oracle.js")):
        return script_root
    return cwd

ROOT = default_root()
ORACLE = os.environ.get("TSRS_DIAG_ORACLE", os.path.join(ROOT, "difftest", "diag_oracle.js"))
LIBCODES = {2318, 2304, 2583, 2584, 2792}
PRIMARY_FILES = {"main.ts", "main.tsx"}

def env_positive_int(name, default):
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    try:
        value = int(raw)
    except ValueError:
        print(f"{name}: expected positive integer, got {raw!r}; using {default}", file=sys.stderr)
        return default
    if value < 1:
        print(f"{name}: expected positive integer, got {raw!r}; using {default}", file=sys.stderr)
        return default
    return value

CLASSIFY_JOBS = env_positive_int("TSRS_CLASSIFY_JOBS", 4)
TSC_TIMEOUT = env_positive_int("TSRS_TSC_TIMEOUT", 45)
EXTENSIONLESS_TRIES = (".ts", ".tsx", ".d.ts", ".mts", ".cts", ".json")
_LIB_TEXT_CACHE = {}
_ORACLE_WORKER = None

BOOL_OPTIONS = {
    "strict": "strict",
    "strictnullchecks": "strictNullChecks",
    "strictfunctiontypes": "strictFunctionTypes",
    "strictpropertyinitialization": "strictPropertyInitialization",
    "strictbindcallapply": "strictBindCallApply",
    "noimplicitany": "noImplicitAny",
    "noimplicitthis": "noImplicitThis",
    "useunknownincatchvariables": "useUnknownInCatchVariables",
    "nounusedlocals": "noUnusedLocals",
    "nounusedparameters": "noUnusedParameters",
    "noimplicitreturns": "noImplicitReturns",
    "nofallthroughcasesinswitch": "noFallthroughCasesInSwitch",
    "exactoptionalpropertytypes": "exactOptionalPropertyTypes",
    "nouncheckedindexedaccess": "noUncheckedIndexedAccess",
    "noimplicitoverride": "noImplicitOverride",
    "erasablesyntaxonly": "erasableSyntaxOnly",
    "usedefineforclassfields": "useDefineForClassFields",
    "allowunreachablecode": "allowUnreachableCode",
    "allowunusedlabels": "allowUnusedLabels",
    "experimentaldecorators": "experimentalDecorators",
    "emitdecoratormetadata": "emitDecoratorMetadata",
    "sourcemap": "sourceMap",
    "inlinesourcemap": "inlineSourceMap",
    "inlinesources": "inlineSources",
    "declaration": "declaration",
    "declarationmap": "declarationMap",
    "composite": "composite",
    "isolateddeclarations": "isolatedDeclarations",
    "allowimportingtsextensions": "allowImportingTsExtensions",
    "rewriterelativeimportextensions": "rewriteRelativeImportExtensions",
    "resolvepackagejsonexports": "resolvePackageJsonExports",
    "resolvepackagejsonimports": "resolvePackageJsonImports",
    "noemit": "noEmit",
    "emitdeclarationonly": "emitDeclarationOnly",
    "resolvejsonmodule": "resolveJsonModule",
    "incremental": "incremental",
    "isolatedmodules": "isolatedModules",
    "verbatimmodulesyntax": "verbatimModuleSyntax",
    "preserveconstenums": "preserveConstEnums",
    "alwaysstrict": "alwaysStrict",
    "esmoduleinterop": "esModuleInterop",
    "allowsyntheticdefaultimports": "allowSyntheticDefaultImports",
    "downleveliteration": "downlevelIteration",
    "importhelpers": "importHelpers",
    "noimplicitusestrict": "noImplicitUseStrict",
    "keyofstringsonly": "keyofStringsOnly",
    "suppressexcesspropertyerrors": "suppressExcessPropertyErrors",
    "suppressimplicitanyindexerrors": "suppressImplicitAnyIndexErrors",
    "nostrictgenericchecks": "noStrictGenericChecks",
    "preservevalueimports": "preserveValueImports",
}

VALUE_OPTIONS = {
    "declarationdir": "declarationDir",
    "outfile": "outFile",
    "moduleresolution": "moduleResolution",
    "tsbuildinfofile": "tsBuildInfoFile",
    "maproot": "mapRoot",
    "jsxfactory": "jsxFactory",
    "jsxfragmentfactory": "jsxFragmentFactory",
    "reactnamespace": "reactNamespace",
    "jsximportsource": "jsxImportSource",
    "baseurl": "baseUrl",
    "charset": "charset",
    "out": "out",
    "importsnotusedasvalues": "importsNotUsedAsValues",
    "jsx": "jsx",
    "ignoredeprecations": "ignoreDeprecations",
    "rootdir": "rootDir",
}

NOOP_OPTIONS = {
    "allowjs",
    "checkjs",
    "notypesandsymbols",
    "noemithelpers",
    "lib",
    "outdir",
    "noimplicitreferences",
    "traceresolution",
    "suppressoutputpathcheck",
    "currentdirectory",
    "typeroots",
    "allowarbitraryextensions",
    "nolib",
    "noemitonerror",
    "nopropertyaccessfromindexsignature",
    "customconditions",
    "maxnodemodulejsdepth",
    "stripinternal",
    "pretty",
    "allowumdglobalaccess",
    "nouncheckedsideeffectimports",
    "moduledetection",
    "removecomments",
    "strictbuiltiniteratorreturn",
    "libreplacement",
}

IGNORED_BOOL_OPTIONS = {"skipdefaultlibcheck", "skiplibcheck"}

def parse_directive_line(line):
    if not line.startswith("//"):
        return None
    rest = line[2:].lstrip(" \t")
    if not rest.startswith("@"):
        return None
    rest = rest[1:]
    name_end = 0
    while name_end < len(rest) and rest[name_end].isascii() and rest[name_end].isalnum():
        name_end += 1
    if name_end == 0 or not rest[0].isalpha():
        return None
    name = rest[:name_end].lower()
    after = rest[name_end:].lstrip(" \t")
    if not after.startswith(":"):
        return None
    return name, after[1:].strip()

def parse_fixture(source):
    # A leading BOM otherwise hides the first directive line and flips the
    # header state, dropping every option. Mirrors src/harness/mod.rs — both
    # parsers must agree or the corpus comparison stops being apples-to-apples.
    source = source.removeprefix("\ufeff")
    options = []
    extra_root_files = []
    cli_args = None
    files = []
    current = None
    in_header = True
    default_lines = []

    for line in source.split("\n"):
        directive = parse_directive_line(line)
        if directive is not None:
            name, value = directive
            if name == "filename":
                if current is not None:
                    files.append(current)
                current = [value, []]
                continue
            if in_header and current is None and name == "extrarootfiles":
                extra_root_files.extend(s.strip() for s in value.split(",") if s.strip())
                continue
            if in_header and current is None and name == "cliargs":
                cli_args = value.split()
                continue
            if in_header and current is None:
                options.append((name, value))
                continue

        if current is not None:
            current[1].append(line)
        else:
            if line.strip():
                in_header = False
            if not in_header:
                default_lines.append(line)

    if current is not None:
        files.append(current)
    if not files:
        files.append(["main.ts", default_lines])
    return {
        "files": [(name, "\n".join(lines)) for name, lines in files],
        "options": options,
        "extra_root_files": extra_root_files,
        "cli_args": cli_args,
    }

def first_variant(value):
    return value.split(",", 1)[0].strip()

def bool_value(name, value):
    first = first_variant(value).lower()
    if first in ("true", "*"):
        return True
    if first == "false":
        return False
    raise ValueError(f"bad bool for @{name}: {value}")

def compiler_options_from_directives(dirs):
    opts = {
        "noEmit": False,
        "noLib": True,
        "strict": True,
        "declaration": False,
        "target": "es5",
        "moduleResolution": "bundler",
        "ignoreDeprecations": "6.0",
        "alwaysStrict": True,
        "esModuleInterop": True,
        "allowSyntheticDefaultImports": True,
    }
    try:
        for name, value in dirs:
            if name in BOOL_OPTIONS:
                opts[BOOL_OPTIONS[name]] = bool_value(name, value)
            elif name in IGNORED_BOOL_OPTIONS:
                bool_value(name, value)
            elif name in VALUE_OPTIONS:
                opts[VALUE_OPTIONS[name]] = value
            elif name == "target":
                opts["target"] = first_variant(value).lower()
            elif name == "module":
                first = first_variant(value).lower()
                if first == "undefined":
                    opts.pop("module", None)
                else:
                    opts["module"] = first
            elif name == "types":
                opts["types"] = [s.strip() for s in value.split(",") if s.strip()]
            elif name == "paths":
                opts["paths"] = json.loads(value)
            elif name in NOOP_OPTIONS:
                pass
            else:
                raise ValueError(f"unknown directive @{name}")
    except Exception as exc:
        sys.stderr.write(f"harness options unavailable: {exc}\n")
        return None
    return opts

def snapshot_diag_set(diags):
    out = set()
    for x in diags:
        file = x.get("file")
        if not file:
            continue
        base = os.path.basename(file)
        if base not in PRIMARY_FILES:
            continue
        out.add((base, x["code"], (x.get("startLine"), x.get("startCol"))))
    return out

def load(path):
    out = {}
    with open(path, encoding="utf8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if "\x01" not in line: continue
            p, js = line.split("\x01", 1)
            try:
                d = json.loads(js)
            except json.JSONDecodeError:
                out[p] = out.get(p, set())
                continue
            out[p] = snapshot_diag_set(d.get("diagnostics", []))
    return out

def fixture_disk_name(name):
    name = name.replace("\\", "/").strip()
    while name.startswith("/"):
        name = name[1:]
    if len(name) >= 2 and name[1] == ":":
        name = name[0] + "_" + name[2:]
    parts = []
    for part in name.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            continue
        parts.append(part)
    return "/".join(parts) or "main.ts"

def lib_text(lib):
    text = _LIB_TEXT_CACHE.get(lib)
    if text is None:
        with open(lib, encoding="utf8", errors="replace") as fh:
            text = fh.read()
        _LIB_TEXT_CACHE[lib] = text
    return text

def fixture_payload(path, lib):
    with open(path, encoding="utf8", errors="replace") as fh:
        fixture = parse_fixture(fh.read())
    if path.endswith(".tsx") and len(fixture["files"]) == 1 and fixture["files"][0][0] == "main.ts":
        fixture["files"][0] = ("main.tsx", fixture["files"][0][1])

    opts = compiler_options_from_directives(fixture["options"])
    if opts is None:
        return None

    return {
        "files": fixture["files"],
        "extraRootFiles": fixture["extra_root_files"],
        "options": opts,
        "libName": "lib.tsrs.d.ts",
        "libText": lib_text(lib),
        "extensionlessTries": list(EXTENSIONLESS_TRIES),
    }

class OracleWorker:
    def __init__(self):
        self.proc = subprocess.Popen(
            ["node", ORACLE, "--server-jsonl", "--all-files"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.next_id = 0

    def close(self):
        if self.proc.poll() is None:
            try:
                self.proc.stdin.close()
            except Exception:
                pass
            try:
                self.proc.terminate()
            except Exception:
                pass

    def restart(self):
        self.close()
        self.__init__()

    def run(self, payload):
        self.next_id += 1
        req_id = self.next_id
        try:
            self.proc.stdin.write(json.dumps({"id": req_id, "payload": payload}, separators=(",", ":")) + "\n")
            self.proc.stdin.flush()
        except Exception:
            self.restart()
            return None

        deadline = time.monotonic() + TSC_TIMEOUT
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.restart()
                return None
            ready, _, _ = select.select([self.proc.stdout], [], [], remaining)
            if not ready:
                self.restart()
                return None
            line = self.proc.stdout.readline()
            if not line:
                self.restart()
                return None
            try:
                response = json.loads(line)
            except json.JSONDecodeError:
                continue
            if response.get("id") != req_id:
                continue
            if not response.get("ok"):
                return None
            return response.get("result")

def oracle_worker():
    global _ORACLE_WORKER
    if _ORACLE_WORKER is None or _ORACLE_WORKER.proc.poll() is not None:
        _ORACLE_WORKER = OracleWorker()
        atexit.register(_ORACLE_WORKER.close)
    return _ORACLE_WORKER

def tsc_diags_one(path, lib):
    payload = fixture_payload(path, lib)
    if payload is None:
        return None
    d = oracle_worker().run(payload)
    if d is None:
        return None
    return snapshot_diag_set(d.get("diagnostics", []))

def worker(args):
    fn, lib = args
    return fn, tsc_diags_one(fn, lib)

def main():
    if len(sys.argv) != 4:
        print("usage: parallel_classify.py OLD_SNAPSHOT NEW_SNAPSHOT LIB_DTS", file=sys.stderr)
        return 2
    old_p, new_p, lib = sys.argv[1], sys.argv[2], sys.argv[3]
    old = load(old_p)
    new = load(new_p)
    changed = sorted(fn for fn in (set(old) | set(new)) if old.get(fn, set()) != new.get(fn, set()))
    if not changed:
        print("IDENTICAL"); return 0
    print(f"changed: {len(changed)} files")
    print(f"classify_workers: {CLASSIFY_JOBS}, tsc_timeout: {TSC_TIMEOUT}s")
    
    tasks = [(fn, lib) for fn in changed if os.path.exists(fn)]
    tsc_by_fn = {}
    tsc_fail = []
    with ProcessPoolExecutor(max_workers=CLASSIFY_JOBS) as ex:
        futures = {ex.submit(worker, t): t[0] for t in tasks}
        done = 0
        for fut in as_completed(futures):
            fn, tsc = fut.result()
            done += 1
            if done % 50 == 0:
                sys.stderr.write(f"  {done}/{len(tasks)}\n"); sys.stderr.flush()
            if tsc is None:
                tsc_fail.append(fn)
            else:
                tsc_by_fn[fn] = tsc
    
    print(f"tsc_fail: {len(tsc_fail)}")
    new_fp, new_fn = [], []
    ok_add = ok_rm = 0
    for fn, _lib in tasks:
        tsc = tsc_by_fn.get(fn)
        if tsc is None: continue
        o, n = old.get(fn, set()), new.get(fn, set())
        for d in sorted(n - o):
            if d[1] in LIBCODES: continue
            if d in tsc:
                ok_add += 1
            else:
                new_fp.append((fn, d[0], d[1], d[2]))
        for d in sorted(o - n):
            if d[1] in LIBCODES: continue
            if d in tsc:
                new_fn.append((fn, d[0], d[1], d[2]))
            else:
                ok_rm += 1

    print(f"\nimprovements: +{ok_add} correct additions, -{ok_rm} correct FP removals")
    print(f"NEW FALSE NEGATIVES: {len(new_fn)}")
    print(f"NEW FALSE POSITIVES: {len(new_fp)}")

    def by_file(rows):
        files = {}
        for path, file, code, _ in rows:
            base = os.path.basename(path)
            label = base if file in ("main.ts", "main.tsx") else f"{base}:{file}"
            files.setdefault(label, Counter())[code] += 1
        for base in sorted(files):
            codes = files[base]
            total = sum(codes.values())
            detail = " ".join(f"{c}x{codes[c]}" for c in sorted(codes))
            print(f"    {base}: {total}  [{detail}]")

    print("\nNEW FN detail:")
    by_file(new_fn)
    print("\nNEW FP detail:")
    by_file(new_fp)
    return 1 if new_fp else 0

if __name__ == "__main__":
    sys.exit(main())
