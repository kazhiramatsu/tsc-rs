export interface Signal {
	f: number;
}
export interface Value<V = unknown> extends Signal {
	equals(value: unknown): boolean;
	v: V;
}
export interface ArrayExpression {
	type: 'ArrayExpression';
	elements: number[];
}
export interface ObjectExpression {
	type: 'ObjectExpression';
	properties: string[];
}
export type Expression = ArrayExpression | ObjectExpression;
