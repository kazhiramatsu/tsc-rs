namespace Local { export interface Seg { id: string; } }
import { Rule } from "types";
import * as T from "types";
export function h(a: Local.Seg, b: Rule.CodePathSegment, c: T.Linter.Config<number>) {
	const v1: number = { a };
	const v2: number = { b };
	const v3: number = { c };
	return v1 + v2 + v3;
}
