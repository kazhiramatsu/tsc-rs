# Follow-on investigation, not an applied change

Opus99 notes that `visit_for_await_statement` is dispatched before the object-rest branch in `es2018.rs`, whereas upstream first executes transformForOfStatementWithObjectRest and only then transformForAwaitOfStatement. The native ordinary-for-of object-rest function does exist, but it eagerly visits its expression, binding and body, so calling it as a prepass would visit/allocate twice and cannot be the direct repair.

The new 24 object-rest for-await whole commands will establish actual differences. If red, a narrow shared raw createForOfBindingStatement helper could create (without visiting) the original binding statement with a fresh temporary. The for-await-only preparation would create a ranged let-temp head and a ranged block/statement-array from the original body, exactly as `_tsc.js:102196-102240`, before planning/visiting the await loop. The existing await path then visits the prepared body once. Keep the ordinary-for-of implementation unchanged initially unless the observed cause requires a shared update; any broader refactor needs separate controls.

Upstream's helper clears type and exclamation fields, ranges the statement to initializer, and takes only the first declaration. It does not visit. The preparation's new loop declaration also receives the initializer range. Block bodies use their statement-array range; single bodies use the statement range for both new block and array. For-await's later conversion must retain these visited ranges. Binding identity is created before the rest of the await plan; final generated-name numbering is determined by the existing binding finalizer, not copied strings.

This is source analysis only; no implementation or qualification claimed.
