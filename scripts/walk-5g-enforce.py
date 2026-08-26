#!/usr/bin/env python3
"""gate-tax 5-C: zero-observation enforcement for the 5g rung.

Reads the machine-readable outcome record the 5g --check writes
(target/h2-5g/check-outcome.v1.json) — never prose lines: a receipt HIT
is a guarded adoption pass that REUSES the observation progress printer,
so progress-line presence can never distinguish adoption from
re-observation. The verdict is MISS TERM × MINTED-THIS-WALK:

  cold terms   (absent/invalid/workspace/node/platform/arch/
                stored-artifact)                → allowed, notice
  anchor terms (normalized-generator/normalizer/pin-index/
                normalizer-refusal/global-records/observation-content/
                case *)  + 5g re-minted this walk → allowed ONCE, logged
  anchor terms + NO 5g re-mint this walk          → HARD RED (pin-tax
                                                   regression, named)
  any second full observation inside one walk     → HARD RED (broken
                                                   receipt producer)

Overrides (both RECORDED by the driver, never silent):
  WALK_EXPECT_OBS=1  disables enforcement (deliberate re-anchor)
  WALK_EXPECT_OBS=0  forces the strict row (any observation is red)
  TSRS_H2_5G_FRESH=1 runs record `bypassed-fresh` and are exempt

Prints one machine line: verdict=<ok|notice|red> observed=<0|1> ...
Exit: 0 ok/notice, 1 red, 2 usage/self-test failure.
"""
import json
import os
import pathlib
import sys

COLD_TERMS = {
    "absent",
    "invalid",
    "workspace",
    "node",
    "platform",
    "arch",
    "stored-artifact",
}
ANCHOR_TERMS = {
    "normalized-generator",
    "normalizer",
    "pin-index",
    "normalizer-refusal",
    "global-records",
    "observation-content",
}


def classify_term(term):
    if term in COLD_TERMS:
        return "cold"
    if term in ANCHOR_TERMS or (isinstance(term, str) and term.startswith("case ")):
        return "anchor"
    return "unknown"


def decide(outcome, minted_this_walk, observations_so_far, expect_obs):
    """Returns (verdict, observed, message)."""
    if outcome is None:
        return (
            "notice",
            0,
            "no outcome record — the 5g check did not run or predates gate-tax 5",
        )
    attempt = outcome.get("receipt_attempt")
    observed = 1 if outcome.get("full_observation_started") else 0
    if (
        observed
        and outcome.get("completed")
        and (outcome.get("observed_cases") or 0) == 0
    ):
        # a completed run that observed ZERO fresh cases is a pure
        # journal-adoption pass: under the amended keystone the union of
        # partial runs under one key is ONE observation, so this round
        # does not count against the per-walk observation budget
        observed = 0
    if expect_obs == "1":
        return (
            "notice",
            observed,
            "enforcement DISABLED by WALK_EXPECT_OBS=1 (recorded deliberate re-anchor)",
        )
    if attempt == "bypassed-fresh":
        return (
            "notice",
            observed,
            "TSRS_H2_5G_FRESH=1 full observation (recorded approval; exempt)",
        )
    if observed and observations_so_far >= 1:
        return (
            "red",
            observed,
            f"SECOND full observation in one walk (prior={observations_so_far}) — "
            "the receipt producer is broken; every round is re-observing",
        )
    if attempt == "hit":
        return ("ok", observed, "receipt hit — adoption pass, zero observations")
    if attempt != "miss":
        return ("notice", observed, f"unrecognized receipt_attempt {attempt!r}")
    term = outcome.get("miss_term")
    if expect_obs == "0" and observed:
        return (
            "red",
            observed,
            f"WALK_EXPECT_OBS=0 strict mode: observation on miss ({term}) is red",
        )
    kind = classify_term(term)
    if kind == "cold":
        return (
            "notice",
            observed,
            f"cold cache / environment move (miss {term}) — "
            + (
                "journal-adoption pass, 0 fresh observations"
                if observed == 0
                else "observation allowed"
            ),
        )
    if minted_this_walk:
        return (
            "notice",
            observed,
            f"expected trust anchor after this walk's own 5g re-mint (miss {term}) — allowed once",
        )
    return (
        "red",
        observed,
        f"PIN-TAX REGRESSION: 5g receipt key broke (miss {term}) with NO 5g "
        "re-mint this walk — an anchor term diverged without an evidence "
        "change (gate-tax 5-A violation); inspect the receipt terms",
    )


def self_test():
    cases = [
        # (outcome, minted, so_far, expect, want_verdict, want_observed)
        (None, 0, 0, None, "notice", 0),
        ({"receipt_attempt": "hit", "full_observation_started": False}, 0, 0, None, "ok", 0),
        ({"receipt_attempt": "miss", "miss_term": "absent", "full_observation_started": True}, 0, 0, None, "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "invalid", "full_observation_started": True}, 0, 0, None, "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "normalized-generator", "full_observation_started": True}, 1, 0, None, "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "normalized-generator", "full_observation_started": True}, 0, 0, None, "red", 1),
        ({"receipt_attempt": "miss", "miss_term": "global-records", "full_observation_started": True}, 0, 0, None, "red", 1),
        ({"receipt_attempt": "miss", "miss_term": "case x#es5", "full_observation_started": True}, 0, 0, None, "red", 1),
        ({"receipt_attempt": "miss", "miss_term": "case x#es5", "full_observation_started": True}, 1, 0, None, "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "absent", "full_observation_started": True}, 1, 1, None, "red", 1),
        ({"receipt_attempt": "hit", "full_observation_started": False}, 0, 1, None, "ok", 0),
        ({"receipt_attempt": "bypassed-fresh", "full_observation_started": True}, 0, 0, None, "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "normalized-generator", "full_observation_started": True}, 0, 0, "1", "notice", 1),
        ({"receipt_attempt": "miss", "miss_term": "absent", "full_observation_started": True}, 1, 0, "0", "red", 1),
        # pure journal-adoption pass (completed, 0 fresh observations):
        # one logical observation under the amended keystone — never a
        # "second observation" red, and exempt from strict mode
        ({"receipt_attempt": "miss", "miss_term": "invalid", "full_observation_started": True, "completed": True, "observed_cases": 0}, 1, 1, None, "notice", 0),
        ({"receipt_attempt": "miss", "miss_term": "invalid", "full_observation_started": True, "completed": True, "observed_cases": 0}, 1, 0, "0", "notice", 0),
        # …but an ANCHOR-term key break stays red even via adoption
        ({"receipt_attempt": "miss", "miss_term": "normalized-generator", "full_observation_started": True, "completed": True, "observed_cases": 0}, 0, 0, None, "red", 0),
        # a real second observation (fresh cases) still reds
        ({"receipt_attempt": "miss", "miss_term": "absent", "full_observation_started": True, "completed": True, "observed_cases": 5}, 1, 1, None, "red", 1),
    ]
    for index, (outcome, minted, so_far, expect, want_verdict, want_observed) in enumerate(cases):
        verdict, observed, message = decide(outcome, minted, so_far, expect)
        if verdict != want_verdict or observed != want_observed:
            print(
                f"self-test case {index} FAILED: got ({verdict}, {observed}) "
                f"want ({want_verdict}, {want_observed}) — {message}"
            )
            return 2
    print(f"enforce self-test: ok ({len(cases)} table rows)")
    return 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    outcome_path = None
    minted = 0
    so_far = 0
    rest = list(argv)
    while rest:
        argument = rest.pop(0)
        if argument == "--outcome":
            outcome_path = rest.pop(0)
        elif argument == "--minted-this-walk":
            minted = int(rest.pop(0))
        elif argument == "--observations-so-far":
            so_far = int(rest.pop(0))
        else:
            print(f"unknown argument {argument}", file=sys.stderr)
            return 2
    if outcome_path is None:
        print(
            "usage: walk-5g-enforce.py --outcome <path> --minted-this-walk 0|1 "
            "[--observations-so-far N] | --self-test",
            file=sys.stderr,
        )
        return 2
    outcome = None
    try:
        outcome = json.loads(pathlib.Path(outcome_path).read_text())
    except (OSError, ValueError):
        outcome = None
    verdict, observed, message = decide(
        outcome, minted, so_far, os.environ.get("WALK_EXPECT_OBS")
    )
    term = (outcome or {}).get("miss_term")
    attempt = (outcome or {}).get("receipt_attempt")
    print(
        f"verdict={verdict} observed={observed} attempt={attempt} term={term} :: {message}"
    )
    return 1 if verdict == "red" else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
