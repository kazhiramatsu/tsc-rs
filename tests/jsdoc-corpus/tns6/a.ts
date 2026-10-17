import type { SourceLocation } from "estree";
import type { AST, Scope } from "types";
class Reference { resolved = true; }
declare const loc: SourceLocation | AST.SourceLocation | null;
declare const ref: Scope.Reference | Reference;
const s: string = loc;
const t: string = ref;
export { s, t };
