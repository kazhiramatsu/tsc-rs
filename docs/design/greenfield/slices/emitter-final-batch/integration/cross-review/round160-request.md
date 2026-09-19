# Round 160: review concrete four-file follow-up patch
Read-only. Do not edit/build. Frozen Repair d2b29439d remains under r195/r196. Concrete scratch patch /tmp/emitter-r201-source.patch, files /tmp/emitter-r201-source-prep and manifest preimage hashes. Please inspect for blocking defects only, especially source-comment resume/filter semantics and constructor first-present selection. Root will apply after all old frozen runs end.

Root verified generic pipeline source: emit_node_id_with_context -> emit_node_with_hint -> emit_node_with_hint_and_source_comments_worker; plain branch printer.rs15187-15222 unconditionally active_expression_comment_scope unless function-body Block. CaseBlock/CaseClause/DefaultClause therefore already claim own range. No new claim added; patch uses expression_context.comments() directly bothstatementtrailing andrestoredclausetrailing. Existing emit_token_with_comments helper already equals OwnerEnd+BoundaryUnion, so bothcoloncallers simplyuseit. Temporaryescaped-onlyguardremoved.

Scanner fixes together: collector CR/LF onlymainlinebranch and Unicodependingnewline; same-line writer delegates boundedcollector; prefixtrimmer skipswhitespaceexceptCR/LF. ExistingASCII delimiterleadinghelper unchanged asr159 recommends. No parser/checker/coordinate change.

Threeconstructorselectors corrected asr159. Otherdecoratedbodyis_some predicatesleftunchanged.

Planned new controls 16 r168shapes in /tmp/emitter-next-control-shapes-r201.json plus 6 trimmer-neighbor shapes (function/emptyblock/EOF xU2028/U2029). Add ctorpresent, missing+presentparameterproperties, decorators+emitDecoratorMetadata true (classmissingconstructor/presentconstructor/missingparameterdecorator) to detectfoldedclassfacts. CorrectBOMvirtualreadremoveonlyfirstU+FEFF, preservealloldobservations exceptexact4 BOMmapmismatches; addembeddedFEFF/doubleBOMcontrols and standaloneTSsys.readFileproof. Doespatch needanythingelsebeforetheplannedfullcontrolsandregression?
