#!/usr/bin/env python3
"""gate-tax 8 S5: the report-only restamp shadow evaluator.

Runs at walk end from the run directory's events.jsonl + pre/post
captures + the selector manifest, and evaluates the packet §7 truth
table per MINT EVENT (one rung write invocation, producer x round):

  predict SKIP iff (1) the generator+contract diffs are pin-literal-
  only, (2) every changed input leaf is `replaceable-hash` with its
  new value equal to the recipe-derived value, (3) every touched edge
  is declared, (4) no open undeclared-read flag, (5) 1-4 hold for
  every declared output. Anything else -> EXECUTE; any refused
  construction -> UNKNOWN. Construction (value-span replacement on
  the pre-capture bytes; recipes identity-copy / sha256-of-file /
  sha256-of-canonical-json / self-sha256-after) is attempted
  REGARDLESS of prediction; classification against the post-capture:
    SKIP+match      true-skip        SKIP+mismatch    precision-miss
    EXECUTE+match   false-negative   EXECUTE+mismatch true-execute
    refusal         UNKNOWN (excluded from both metrics)
  An unmodeled producer's event abstains `unmodeled` (UNKNOWN).

The report NEVER affects the walk (report-only; G3): any internal
failure is a summary warning, not a red. Wall-clock event costs carry
advisory weight only; the promotion-grade tick/window protocol is
spec-frozen for the Stage-2 window opening (packet §7/§9).

Usage: shadow-report.py <run_dir> [--manifest <path>]
Writes <run_dir>/shadow-report.v1.json and prints one summary line.
"""
import hashlib
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import canonical_json  # noqa: E402

MANIFEST = "scripts/shadow/selector-manifest.v1.json"
MODEL_FILES = [
    "scripts/shadow/selector-manifest.v1.json",
    "scripts/shadow/shadow-report.py",
    "scripts/shadow/capture.py",
    "scripts/shadow/canonical_json.py",
]


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def model_digest(manifest_path):
    framed = []
    for path in sorted(set(MODEL_FILES + [manifest_path])):
        try:
            body = open(path, "rb").read()
        except OSError:
            body = b"<missing>"
        framed.append(f"{len(path)}\n{path}\n{len(body)}\n".encode() + body)
    return sha256_bytes(b"".join(framed))


def capture_path(run_directory, stage, round_number, output):
    return os.path.join(
        run_directory, stage, str(round_number), output.replace("/", "__")
    )


def derive(recipe, pre_bytes, run_directory, round_number):
    """The closed v1 recipe vocabulary; returns (value, None) or
    (None, abstain_reason)."""
    kind = recipe.get("recipe")
    if kind == "sha256-of-file":
        source = capture_path(
            run_directory, "post", round_number, recipe.get("upstream_output", "")
        )
        try:
            return sha256_bytes(open(source, "rb").read()), None
        except OSError:
            return None, "recipe-source-missing"
    if kind == "identity-copy":
        source = capture_path(
            run_directory, "post", round_number, recipe.get("upstream_output", "")
        )
        try:
            upstream = json.load(open(source, encoding="utf-8"))
        except (OSError, ValueError):
            return None, "recipe-source-missing"
        node = upstream
        for part in recipe.get("source_pointer", "").lstrip("/").split("/"):
            if not part:
                return None, "recipe-vocabulary"
            node = node[int(part)] if isinstance(node, list) else node.get(part)
            if node is None:
                return None, "recipe-source-missing"
        return node if isinstance(node, str) else None, (
            None if isinstance(node, str) else "recipe-vocabulary"
        )
    if kind == "sha256-of-canonical-json":
        source = capture_path(
            run_directory, "post", round_number, recipe.get("upstream_output", "")
        )
        try:
            body = open(source, encoding="utf-8").read()
        except OSError:
            return None, "recipe-source-missing"
        encoded, reason = canonical_json.encode_source(body)
        if reason:
            return None, f"encoder-scope:{reason}"
        return sha256_bytes(encoded), None
    if kind == "self-sha256-after":
        encoded, reason = canonical_json.exclude_members(
            pre_bytes.decode("utf-8", errors="replace"),
            recipe.get("excluded_members", []),
        )
        if reason:
            return None, f"self-recipe-scope:{reason}"
        return sha256_bytes(encoded), None
    return None, "recipe-vocabulary"


def construct(producer_row, event, run_directory):
    """Predicted restamped bytes per output via value-span replacement
    on the pre-capture; (outputs->bytes, None) or (None, reason)."""
    round_number = event["round"]
    constructed = {}
    ordered = sorted(
        producer_row.get("outputs", []),
        key=lambda output: output.get("output_path", ""),
    )
    for output in ordered:
        path = output["output_path"]
        pre_file = capture_path(run_directory, "pre", round_number, path)
        try:
            body = open(pre_file, "rb").read()
        except OSError:
            return None, "capture-missing"
        text = body.decode("utf-8", errors="replace")
        leaves = sorted(
            (leaf for leaf in output.get("leaves", [])
             if leaf.get("class") == "replaceable-hash"),
            key=lambda leaf: leaf.get("order", 0),
        )
        for leaf in leaves:
            recipe = leaf.get("recipe")
            if not recipe:
                return None, "recipe-vocabulary"
            value, reason = derive(recipe, body, run_directory, round_number)
            if reason:
                return None, reason
            old = leaf.get("span_current")
            if not old or text.count(old) != leaf.get("span_count", 1):
                return None, "span-inexpressible"
            text = text.replace(old, value)
        constructed[path] = text.encode("utf-8")
    return constructed, None


def main():
    run_directory = sys.argv[1]
    manifest_path = MANIFEST
    if "--manifest" in sys.argv:
        manifest_path = sys.argv[sys.argv.index("--manifest") + 1]
    manifest = json.load(open(manifest_path, encoding="utf-8"))
    producers = manifest.get("producers", {})
    flags = manifest.get("flags", {})

    events = []
    try:
        for line in open(os.path.join(run_directory, "events.jsonl"),
                         encoding="utf-8"):
            row = json.loads(line)
            if row.get("phase") == "write":
                events.append({"round": row["round"], "rung": row["rung"]})
    except OSError:
        pass

    outcomes = []
    for event in events:
        rung = event["rung"]
        row = producers.get(rung)
        if row is None:
            outcomes.append({**event, "outcome": "UNKNOWN",
                             "reason": "unmodeled"})
            continue
        if rung in flags:
            prediction = "EXECUTE"
            prediction_reason = "undeclared-read-flag"
        else:
            prediction = row.get("prediction", "EXECUTE")
            prediction_reason = row.get("prediction_reason", "declared")
        constructed, reason = construct(row, event, run_directory)
        if reason:
            outcomes.append({**event, "outcome": "UNKNOWN", "reason": reason,
                             "prediction": prediction})
            continue
        matches = True
        for path, predicted in constructed.items():
            post_file = capture_path(run_directory, "post", event["round"], path)
            try:
                minted = open(post_file, "rb").read()
            except OSError:
                matches = False
                break
            if minted != predicted:
                matches = False
                break
        if prediction == "SKIP":
            outcome = "true-skip" if matches else "precision-miss"
        else:
            outcome = "false-negative" if matches else "true-execute"
        outcomes.append({**event, "outcome": outcome,
                         "prediction": prediction,
                         "prediction_reason": prediction_reason})

    counted = [o for o in outcomes if o["outcome"] != "UNKNOWN"]
    skips = [o for o in counted if o["prediction"] == "SKIP"]
    true_skip = sum(1 for o in counted if o["outcome"] == "true-skip")
    false_negative = sum(1 for o in counted if o["outcome"] == "false-negative")
    report = {
        "schema": 1,
        "model_digest": model_digest(manifest_path),
        "mint_events": len(events),
        "modeled_events": len(counted),
        "unknown_events": len(events) - len(counted),
        "outcomes": outcomes,
        "precision": (
            true_skip / len(skips) if skips else None
        ),
        "recall": (
            true_skip / (true_skip + false_negative)
            if (true_skip + false_negative) else None
        ),
        "note": "report-only; pre-hermetic model-maturation evidence "
                "(packet §7/§9) — never promotion-countable",
    }
    out = os.path.join(run_directory, "shadow-report.v1.json")
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=1)
        handle.write("\n")
    print(
        f"shadow: events={report['mint_events']} "
        f"modeled={report['modeled_events']} "
        f"unknown={report['unknown_events']} "
        f"precision={report['precision']} recall={report['recall']} "
        f"model={report['model_digest'][:12]}.."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
