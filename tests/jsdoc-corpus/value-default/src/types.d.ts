export interface Signal {
	f: number;
}
export interface Value<V = unknown> extends Signal {
	v: V;
	/** Equality function */
	equals: (this: Value, value: unknown) => boolean;
}
export interface Source<V = unknown> extends Value<V> {
	s: V;
}
