export function e(first, actual) {
	const x = first;
	if (actual) throw new Error("x");
	return actual + x;
}
