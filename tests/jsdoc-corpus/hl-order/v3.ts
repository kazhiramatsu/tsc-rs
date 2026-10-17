export function c(actual: number) {
	if (actual) throw new Error("x");
	return actual;
}
