export type Equals = (this: Value, value: unknown) => boolean;
export interface Signal {
	f: number;
	wv: number;
}
export interface Value<V = unknown> extends Signal {
	/** Equality function */
	equals: Equals;
	v: V;
}
