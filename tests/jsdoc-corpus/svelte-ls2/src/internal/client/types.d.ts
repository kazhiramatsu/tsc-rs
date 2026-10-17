export interface Signal {
	f: number;
}
export type Equals = (this: Value, value: unknown) => boolean;
export interface Value<V = unknown> extends Signal {
	equals: Equals;
	v: V;
}
