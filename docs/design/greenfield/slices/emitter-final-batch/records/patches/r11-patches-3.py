#!/usr/bin/env python3
"""r11 patch script, part 3 (harness side): project `noEmitOnError` / `preserveConstEnums` in the
universe replay (`isolatedModulesNoEmitOnError`, `isolatedModulesRequiresPreserveConstEnum`).
usage: python3 r11-patches-3.py <tree>"""
import sys, os
ROOT = sys.argv[1]
def patch(rel, old, new, count=1):
    path = os.path.join(ROOT, rel)
    s = open(path).read()
    if new in s:
        print(f"  already applied: {rel}"); return
    assert s.count(old) == count, f"{rel}: anchor count {s.count(old)} != {count}"
    open(path, "w").write(s.replace(old, new)); print(f"  patched {rel}")
patch("crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs",
"""            "noEmit" => options.no_emit = Some(value.as_bool().unwrap()),
""",
"""            "noEmit" => options.no_emit = Some(value.as_bool().unwrap()),
            "noEmitOnError" => options.no_emit_on_error = Some(value.as_bool().unwrap()),
            "preserveConstEnums" => {
                options.preserve_const_enums = Some(value.as_bool().unwrap())
            }
""")
print("r11 part 3 applied")
