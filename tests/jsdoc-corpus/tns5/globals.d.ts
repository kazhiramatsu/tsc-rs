interface Object {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface Array<T> { length: number; [n: number]: T; }
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
interface IArguments {}
type Record<K extends keyof any, T> = { [P in K]: T };
declare module "types" {
	export namespace Linter {
		interface Config<Rules = RulesRecord> { rules?: Rules; name?: string; }
		type RulesRecord = Record<string, number>;
	}
	export namespace Rule {
		interface CodePathSegment { id: string; }
	}
}
