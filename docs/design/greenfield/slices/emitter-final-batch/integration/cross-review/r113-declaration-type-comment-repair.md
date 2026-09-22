# r113 declaration type-comment repair candidate

The r109 complete-command audit found five differences in 61 cases (56 exact): three declaration-comment commands and two non-block do/while layouts. The latter are addressed by r112's completed child phase; the three declaration commands require two additional local phases.

The VariableDeclaration declaration-syntax arm completes an explicit type annotation against the declaration's own comment container. Initialized declarations print the type comment there; uninitialized ones retain it for their enclosing terminator. The shared type-annotation writer is unchanged.

A PropertyAccess name emits its erased-type trailing boundary only when that name itself is the original VariableDeclaration identifier. It uses the name's flags and the type's raw range. The metadata query runs before parent inspection. Parameter type metadata and inherited clone metadata are excluded. No metadata-merging policy changes.

Opus rounds113–115 reviewed the source and phases. The root rejected an unverified supplementary Unicode suggestion: at ES5 `\u{1d400}` is syntax-invalid. Valid BMP `\u{61}` correctly loses the erased-type metadata during ES2015 normalization, while ES2015 target retains it. The complete controls cover both, plus System, multiple declarations, a literal annotation, parameter and class properties, a hoisted generator and a namespace export. All nine TS commands were observed twice with no diagnostics. Native qualification remains pending. The earlier five/eight observations and their exact observer bytes are archived; existing case objects are unchanged.

System's bare identifier and namespace ownership are additional controls, not assumed covered by the new PropertyAccess phase. Any failures must be diagnosed before qualification.
