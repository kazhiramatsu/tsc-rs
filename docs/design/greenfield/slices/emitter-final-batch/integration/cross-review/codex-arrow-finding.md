# Codex independent finding for comparison

Complete command capture: `arrow-complete-capture.json.gz` (decompressed SHA-256 a8f413bea2d254a5d15fdb4ecd5dc35277af515fae78f14c601aa84c9e4a63a5). Baseline arrow map failures also differ in JavaScript:

TypeScript: `() => { var _a, _b; return Reflect.set(...), _a; }`
Rust: `() => { var _a, _b; return (Reflect.set(...), _a); }`

Both factory/printer partial-wrapper changes left this unchanged. The likely owner is `standard_decorators.rs::visit_function_like_body`: it calls `update_generic` (therefore `factory.update_node` and its new arrow-concise parenthesizer rule) before obtaining hoisted temporaries. Only afterward does it replace the concise body with a block/return. The intermediate arrow factory has already wrapped the comma body, so the return retains unnecessary parentheses. Upstream `visitFunctionBody` builds the block before `factory.updateArrowFunction` applies concise-body rules.

Proposed minimal correction: visit the parameter list and raw child NodeData under the lexical environment, then finish lexical environment; if no temporaries/initializations, update normally. If a block conversion is needed, build it from the raw visited expression before updating the enclosing function. Preserve typed original/range/flags through the same final update. Do not suppress maps or delete all parentheses. Validate selected 18 primary/followup commands, full decorator-super/controls/pipeline, and factory/arrow/printer units. Claude should independently confirm or reject the cause and recommend whether the two partial-wrapper changes are actually needed in this train.
