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
declare module "estree" {
	export interface SourceLocation { source?: string; }
}
declare module "types" {
	export namespace AST {
		interface SourceLocation { start: number; }
	}
	export namespace Scope {
		interface Reference { identifier: string; }
	}
}
