# Round69: typed wrapper temporary implementation

Read-only review/no builds. Recovery-next /Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next emitter code now updated per agreed round68. Please inspect current git diff, especially system.rs, factory.rs, printer.rs. Source frozen until your review ends.
- SystemHoistedName Plain(String)/Generated(TargetBinding), typed declaration occurrence order; hoist_name_node no longer inserts generated identity into text map.
- allocate_update_temp returns TargetBinding::allocate for all scopes; wrapper entry vs local context hoist. ensure_system_binding_identifier and FlattenHost delegate; manual temp allocator removed.
- module-name reservation restored all identifier spellings; numbered/default/import/export/context text map retained.
- class/function local names preserve typed metadata by create_local_name_reference; assignment target existing name nodes used.
- wrapper declaration constructor with TransformNode preserves binding identity.
- unary/LHS kind predicates shared factory+printer exact upstream including MissingDeclaration; PartiallyEmitted ancestry unwraps removed.

Audit remaining textual temp recreation paths and binding metadata policies carefully. Does generated_binding_of_identifier/from_existing reproduce FinalizerTraversal/ScopeReserve policies? Required invariants: for-ofexecute _a can shadowwrapper _a; wrapper earlier/later pass temps distinct and named hoist order; private authoritative temps not clobbered.

Fixture expanded further788->842; prior788same inputs; the four ineffective one-element tests retained, added multi-element both orders, 2postfix, nullish before/after includingES2019,privatefield includingES2021. Observer running, two complete TScommands each. Factory parentheses/global regression suite will run after compile; no expectation weakening. Please report blockers promptly and confirm scope of mandatory followups. Current census checker run in separatetree, not scope.
