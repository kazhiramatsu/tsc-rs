/** @import { A, B } from "./types" */
import { g } from "./b.js";

/** @typedef {{ start?: number, end?: number }} NodeLike */
export class Parser {
	generate() {
		return g(1);
	}
}
