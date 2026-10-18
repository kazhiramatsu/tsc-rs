const o = {
	get first() { return "first value here"; },
	set first(value: string) {},
	second(count: number): string { return String(count); },
	third: <T extends string>(value: T, ...rest: number[]) => value,
	fourth: [1, "two", true] as const,
	fifth: null as string | null,
	sixth: undefined,
	seventh: 10n,
	eighth: 0x10,
	ninth: "nine",
	tenth: { a: 1, b: "two", c: { d: true } },
	eleventh: true,
	twelfth: [1, 2, 3],
	thirteenth: () => 1,
	fourteenth: function (x: string) { return x; },
};
const n: number = o;
