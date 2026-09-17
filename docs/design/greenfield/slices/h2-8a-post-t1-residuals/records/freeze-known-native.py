#!/usr/bin/env python3
"""Freeze native projections dumped by the contract test
(`TSC_RS_POST_T1_RESIDUALS_DUMP_DIR`, `known-native-<key>.json`) into
`post-t1-residuals-known-native.json` with an owner classification.

usage: python3 freeze-known-native.py <dump-dir> <fixture-known-native.json> <class> <owner> <case_id>...
"""
import hashlib, json, sys
dump, fixture, klass, owner = sys.argv[1:5]
ids = sys.argv[5:]
data = json.load(open(fixture))
existing = {c["case_id"] for c in data["cases"]}
for cid in ids:
    key = hashlib.sha256(cid.encode()).hexdigest()[:16]
    native = json.load(open(f"{dump}/known-native-{key}.json"))
    assert native["case_id"] == cid
    assert cid not in existing, cid
    data["cases"].append({"case_id": cid, "class": klass, "owner": owner,
                          "disposition": "divergent-writes-no-exact-credit", "native": native["native"]})
json.dump(data, open(fixture, "w"), indent=2)
open(fixture, "a").write("\n")
print(json.dumps({"frozen": ids, "total": len(data["cases"])}))
