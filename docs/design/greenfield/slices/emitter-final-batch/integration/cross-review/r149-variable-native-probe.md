# Native delimiter and comment phase probe

At cfb0137c2, all203 syntax library tests pass. The new delimiter positives (including using/await using), negative neighbours and tampered event guards pass without changing any prior predicate. The first native printer run has145 exact controls twice and2 failures, both newly added synthetic CloneName cases. All original48 flag controls and all typed/empty-type/no-nested-comment controls pass.

The two clone failures retain newline comments after a cloned name whose raw range is synthetic. TypeScript prints no such comments because emitInitializer reads the selected name/type's raw end, without following its original link. Actual Opus134 reviewed the concrete transforms: ES5 renames, destructuring and using names that need ranges explicitly retain them; generated declarations retain the token context shape gate.

The repair changes only this VariableDeclaration initializer cursor to the existing node_end_cursor. Original-chain cursor helpers, shared collectors and source-map code are unchanged. The captured native failures are retained. Next: rerun all147 printer controls before broader compiler/emitter tests, then fresh successor proof and selected complete commands. All live KNOWN guards remain unchanged.
