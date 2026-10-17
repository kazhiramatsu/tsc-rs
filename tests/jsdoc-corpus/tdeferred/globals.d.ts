interface Object {}
interface Function {}
interface Array<T> { length: number; [n: number]: T; }
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
interface IArguments {}
type Record<K extends keyof any, T> = { [P in K]: T };
declare class UnsetAdditionalOptions { _UnsetAdditionalOptions: true; }
declare class SyncBailHook<T, R, AdditionalOptions = UnsetAdditionalOptions> { call(...args: T extends unknown[] ? T : never): R; options: AdditionalOptions; }
declare class TypedHookMap<M extends Record<any, SyncBailHook<any, any>>> { for<K extends keyof M>(key: K): M[K]; }
interface CallableFunction {}
interface NewableFunction {}
