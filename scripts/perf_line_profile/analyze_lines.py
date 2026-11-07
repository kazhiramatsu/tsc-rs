#!/usr/bin/env python3
"""Attribute samply thread CPU deltas to inline-aware functions and source lines.

Resolve each address with atos: the samply sidecar merges addresses by symbol.
Inclusive entries overlap; owner entries attribute each sample to exactly one
nearest project frame. No call counts are inferred from samples. Zero-CPU waits
are excluded, even when their weight is big. macOS arm64 only.
"""
import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess


def analyze(path, binary, demangler=None):
    with gzip.open(path, "rt") as f:
        profile = json.load(f)
    sidecar = json.loads(Path(str(path).replace(".json.gz", ".json.syms.json")).read_text())
    strings = sidecar["string_table"]
    assert profile["meta"]["sampleUnits"]["threadCPUDelta"] == "µs"
    symbols = {}
    for library in sidecar["data"]:
        addresses = {}
        for address, index in library["known_addresses"]:
            if index is None:
                continue
            symbol = library["symbol_table"][index]
            # This samply sidecar deduplicates by symbol, so its line/inline
            # information describes one address, not every address in a symbol.
            addresses[address] = [(strings[symbol["symbol"]], None, None)]
        # macOS sidecars use a UUID; the profile uses uppercase Breakpad UUID + age.
        symbols[(library["debug_name"], library["debug_id"].replace("-", "").lower()[:32])] = addresses

    primary = next(i for i, x in enumerate(profile["libs"]) if x["name"] == binary.name)
    library = profile["libs"][primary]
    if library.get("arch") != "arm64":
        raise ValueError("This analyzer currently supports macOS arm64 binaries only")
    uuid_output = subprocess.check_output(["dwarfdump", "--uuid", str(binary)], text=True)
    expected_uuid = library["breakpadId"][:32].lower()
    uuid_matches = re.findall(r"UUID: ([0-9A-Fa-f-]+) \(arm64\)", uuid_output)
    if not any(x.replace("-", "").lower() == expected_uuid for x in uuid_matches):
        raise ValueError("The symbolication binary UUID does not match the recorded profile")
    library_key = (library["debugName"], library["breakpadId"].lower()[:32])
    all_addresses = set()
    for thread in profile["threads"]:
        ft, fn, rt = (thread[k] for k in ("frameTable", "funcTable", "resourceTable"))
        for i in range(ft["length"]):
            resource = fn["resource"][ft["func"][i]]
            if resource is not None and resource >= 0 and rt["lib"][resource] == primary:
                all_addresses.add(ft["address"][i])
    all_addresses = sorted(all_addresses)
    address_file = Path(str(path) + ".addresses.txt")
    address_file.write_text("".join(hex(0x100000000 + address) + "\n" for address in all_addresses))
    raw_file = Path(str(path) + ".atos.txt")
    command = ["atos", "-o", str(binary), "-arch", "arm64", "-l", "0x100000000", "-i", "-fullPath", "-f", str(address_file)]
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    raw_file.write_text(result.stdout)
    groups = re.split(r"\n\s*\n", result.stdout.strip())
    assert len(groups) == len(all_addresses), (len(groups), len(all_addresses), result.stderr)
    line_pattern = re.compile(r"^(.*?) \(in [^)]*\)(?: \((.*):(\d+)\))?(?: \+ \d+)?$")
    mangled_names = sorted({match.group(1) for group in groups for raw in group.splitlines()
                            if (match := line_pattern.match(raw)) and match.group(1).startswith("_R")})
    demangled = {}
    demangler = str(demangler) if demangler else shutil.which("rustfilt")
    if demangler and mangled_names:
        decoded = subprocess.run([demangler], input="\n".join(mangled_names) + "\n", capture_output=True, text=True, check=True).stdout.splitlines()
        if len(decoded) != len(mangled_names):
            raise ValueError("Demangler did not return one function name per input")
        demangled = dict(zip(mangled_names, decoded))
    address_frames = {}
    for address, group in zip(all_addresses, groups):
        frames = []
        raw_lines = group.splitlines()
        for frame_index, raw in enumerate(raw_lines):
            match = line_pattern.match(raw)
            if match:
                name, source, line = match.groups()
                name = demangled.get(name, name)
                if name.startswith("_R") and frame_index == len(raw_lines) - 1:
                    name = symbols.get(library_key, {}).get(address, [(name, None, None)])[0][0]
                name = re.sub(r"::h[0-9a-f]{16}$", "", name)
                frames.append((name, source, int(line) if line else None))
            else:
                frames.append((raw, None, None))
        address_frames[address] = list(reversed(frames))
    symbols[library_key] = address_frames
    Path(str(path) + ".address-frames.json").write_text(json.dumps(address_frames, ensure_ascii=False))

    def project(frame):
        file = frame[1] or ""
        allocator_shim = any(name in frame[0] for name in ("__rust_alloc", "__rust_dealloc", "__rust_realloc", "_rust_alloc", "_rust_dealloc", "_rust_realloc"))
        return "/crates/" in file and ("/tsc-rs" in file or "/tsrs" in file) and not allocator_shim

    def location(frame, line=True):
        name, file, number = frame
        if file and "/crates/" in file:
            file = "crates/" + file.split("/crates/", 1)[1]
        return f"{file or '?'}{':' + str(number) if line and number else ''} | {name}"

    counters = {name: Counter() for name in ("self_lines", "self_functions", "owner_lines", "owner_functions", "inclusive_functions", "inclusive_lines", "inclusive_source_lines", "self_source_lines", "caller_edges", "owner_callers", "self_samples", "owner_samples", "inclusive_samples")}
    total = 0
    line_cpu = 0
    project_cpu = 0
    threads = []
    for thread in profile["threads"]:
        ft, fn, rt, stacks = (thread[k] for k in ("frameTable", "funcTable", "resourceTable", "stackTable"))
        logical_frames = []
        for i in range(ft["length"]):
            resource = fn["resource"][ft["func"][i]]
            lib_index = rt["lib"][resource] if resource is not None and resource >= 0 else None
            library = profile["libs"][lib_index] if lib_index is not None else {}
            key = (library.get("debugName"), library.get("breakpadId", "").lower()[:32])
            frames = symbols.get(key, {}).get(ft["address"][i])
            logical_frames.append(frames or [(library.get("name", "?") + ":" + hex(ft["address"][i]), None, None)])
        cache = {None: ()}

        def stack(index):
            pending = []
            while index not in cache:
                pending.append(index)
                index = stacks["prefix"][index]
            result = cache[index]
            for index in reversed(pending):
                result = result + tuple(logical_frames[stacks["frame"][index]])
                cache[index] = result
            return result

        cpu = 0
        active = 0
        missing = 0
        zero = 0
        samples = thread["samples"]
        first = last = None
        for index, delta, timestamp in zip(samples["stack"], samples["threadCPUDelta"], samples["time"]):
            if delta is None or index is None:
                missing += 1
                continue
            if delta <= 0:
                zero += 1
                continue
            frames = stack(index)
            if not frames:
                missing += 1
                continue
            cpu += delta
            active += 1
            first = timestamp if first is None else first
            last = timestamp
            total += delta
            if frames[-1][1] and frames[-1][2]:
                line_cpu += delta
            counters["self_lines"][location(frames[-1])] += delta
            counters["self_source_lines"][location(frames[-1]).split(" | ", 1)[0]] += delta
            counters["self_functions"][location(frames[-1], False)] += delta
            counters["self_samples"][location(frames[-1])] += 1
            project_frames = [frame for frame in frames if project(frame)]
            owner = project_frames[-1] if project_frames else frames[-1]
            if project_frames:
                project_cpu += delta
            counters["owner_lines"][location(owner)] += delta
            counters["owner_samples"][location(owner)] += 1
            counters["owner_functions"][location(owner, False)] += delta
            for function in {location(frame, False) for frame in frames}:
                counters["inclusive_functions"][function] += delta
            for line in {location(frame) for frame in frames}:
                counters["inclusive_lines"][line] += delta
                counters["inclusive_samples"][line] += 1
            for line in {location(frame).split(" | ", 1)[0] for frame in frames}:
                counters["inclusive_source_lines"][line] += delta
            edges = set()
            for caller, callee in zip(project_frames, project_frames[1:]):
                if location(caller, False) != location(callee, False):
                    edges.add(location(callee, False) + " <- " + location(caller))
            for edge in edges:
                counters["caller_edges"][edge] += delta
            caller = next((frame for frame in reversed(project_frames[:-1]) if location(frame, False) != location(owner, False)), None)
            if caller:
                counters["owner_callers"][location(owner, False) + " <- " + location(caller)] += delta
        threads.append({"pid":thread["pid"], "tid":thread["tid"], "name":thread["name"], "cpu_ms":cpu / 1000,
                        "active_samples":active, "zero_cpu_samples":zero, "missing_samples":missing,
                        "first_active_ms":first, "last_active_ms":last})
    assert sum(counters["self_lines"].values()) == total
    assert sum(counters["owner_lines"].values()) == total
    return {"profile":str(path), "cpu_ms":total / 1000,
            "symbolication":{"binary":str(binary), "uuid":expected_uuid,
                             "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
                             "profile_sha256":hashlib.sha256(path.read_bytes()).hexdigest(),
                             "demangler":demangler, "mangled_names":len(mangled_names),
                             "decoded_names":sum(name != value for name,value in demangled.items()),
                             "command":command, "raw_address_frames":str(path) + ".address-frames.json"},
            "self_line_cpu_percent":100 * line_cpu / total if total else 0,
            "project_owner_cpu_percent":100 * project_cpu / total if total else 0,
            "threads":threads, "counters_us":{k:dict(v.most_common()) for k,v in counters.items()},
            "limitations":["sampling estimates CPU attribution, not exact per-line execution time or invocation count",
                           "optimized source lines and inline frames can span/reorder work",
                           "inclusive function costs overlap; owner cost assigns descendants to nearest project frame",
                           "CPU delta is attributed to the stack at the end of the sample interval",
                           "profiling runtime is not used to claim a wall-time speedup"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("profile", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--demangler", type=Path, help="Rust symbol filter (otherwise rustfilt from PATH if available)")
    args = parser.parse_args()
    result = analyze(args.profile, args.binary, args.demangler)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({key:result[key] for key in ("cpu_ms", "self_line_cpu_percent", "project_owner_cpu_percent")}, ensure_ascii=False))
    for category in ("owner_functions", "owner_lines"):
        print(category)
        for key, value in list(result["counters_us"][category].items())[:20]:
            print(f"{value / 1000:9.2f} ms {value / (result['cpu_ms'] * 10):6.2f}% {key}")
