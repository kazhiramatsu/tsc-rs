#!/usr/bin/env python3
"""r8 part 6 (EF2-LOOP-VARIABLE-POLICY): an identifier re-created for an existing
`createLoopVariable` binding (TargetBinding::from_existing at 8 visitors; EmitMetadata::merge_from
for clones) kept the binding id but dropped the `_i`-family policy, so the print-time finalize walk
named the hoisted `var _i` of a lowered generator/async loop as an ordinary temp (`_a`)."""
import re, glob
W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:90])
        s = s.replace(old, new)
    open(path, "w").write(s)
patch(W + "crates/emitter/src/metadata.rs", [
("        self.generated_binding_print_order |= source.generated_binding_print_order;\n",
 "        self.generated_binding_print_order |= source.generated_binding_print_order;\n        self.generated_binding_loop_variable |= source.generated_binding_loop_variable;\n"),
])
patch(W + "crates/emitter/src/builtins/target_bindings.rs", [
("""        file_level_optimistic: bool,
        planned_name_authoritative: bool,
        reserve_in_nested_scopes: bool,
        private_temp: bool,
    ) -> Self {
        debug_assert!(!file_level_optimistic || preferred_base.is_some());""",
"""        file_level_optimistic: bool,
        planned_name_authoritative: bool,
        loop_variable: bool,
        reserve_in_nested_scopes: bool,
        private_temp: bool,
    ) -> Self {
        debug_assert!(!file_level_optimistic || preferred_base.is_some());"""),
("""            ordinary_temp_name_policy: if planned_name_authoritative {
                OrdinaryTempNamePolicy::PlannedSpellingAuthoritative
            } else {
                OrdinaryTempNamePolicy::FinalizerTraversal
            },
            reserve_in_nested_scopes,
            derived_from: None,
        }
    }
""",
"""            // tsc `cloneNode` keeps `autoGenerate` whole: a re-created
            // identifier of a `createLoopVariable` binding still names as
            // the `_i` family at its first print-order event.
            ordinary_temp_name_policy: if loop_variable {
                OrdinaryTempNamePolicy::LoopVariable
            } else if planned_name_authoritative {
                OrdinaryTempNamePolicy::PlannedSpellingAuthoritative
            } else {
                OrdinaryTempNamePolicy::FinalizerTraversal
            },
            reserve_in_nested_scopes,
            derived_from: None,
        }
    }
"""),
])
pat = re.compile(r"(\n(\s+)metadata\.generated_binding_planned_name_is_authoritative\(\),\n)(\s+metadata\.generated_binding_reserved_in_nested_scopes\(\),)")
total = 0
for p in glob.glob(W + "crates/emitter/src/**/*.rs", recursive=True):
    t = open(p).read()
    if "from_existing(" not in t:
        continue
    t2, n = pat.subn(lambda m: m.group(1) + m.group(2) + "metadata.generated_binding_is_loop_variable(),\n" + m.group(3), t)
    if n:
        open(p, "w").write(t2); total += n
assert total == 8, total
print("r8 part 6 applied")
