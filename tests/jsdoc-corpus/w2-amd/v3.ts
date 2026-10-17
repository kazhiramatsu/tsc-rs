import type { Options } from "./types";
export function f3(options: Options) {
	const n: number = { ...options };
	return n;
}
