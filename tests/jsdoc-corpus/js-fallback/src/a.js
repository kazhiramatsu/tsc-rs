export function f(x) {
	return x.toArray();
}
/** @param {{ store: null; unsubscribe: Function }} entry */
export function g(entry) {
	entry.unsubscribe();
}
