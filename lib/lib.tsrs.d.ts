/// <reference no-default-lib="true"/>
// Curated standard library for tsrs. Declarations are global (this file is a
// non-module .d.ts). Kept deliberately small but self-consistent so that both
// tsrs and a reference tsc run (`--noLib` over this same file) agree.

type PropertyKey = string | number | symbol;

interface Object {
    constructor: Function;
    toString(): string;
    toLocaleString(): string;
    valueOf(): Object;
    hasOwnProperty(v: PropertyKey): boolean;
    isPrototypeOf(v: Object): boolean;
    propertyIsEnumerable(v: PropertyKey): boolean;
}

interface ObjectConstructor {
    new (value?: any): Object;
    (value?: any): any;
    readonly prototype: Object;
    keys(o: object): string[];
    values<T>(o: { [s: string]: T } | ArrayLike<T>): T[];
    values(o: object): any[];
    entries<T>(o: { [s: string]: T } | ArrayLike<T>): [string, T][];
    entries(o: object): [string, any][];
    assign<T, U>(target: T, source: U): T & U;
    assign(target: object, ...sources: any[]): any;
    freeze<T>(o: T): Readonly<T>;
    create(o: object | null): any;
    getPrototypeOf(o: any): any;
    defineProperty<T>(o: T, p: PropertyKey, attributes: PropertyDescriptor): T;
    getOwnPropertyNames(o: any): string[];
    fromEntries<T>(entries: Iterable<readonly [PropertyKey, T]>): { [k: string]: T };
}
declare var Object: ObjectConstructor;

interface PropertyDescriptor {
    configurable?: boolean;
    enumerable?: boolean;
    value?: any;
    writable?: boolean;
    get?(): any;
    set?(v: any): void;
}

interface Function {
    apply(this: Function, thisArg: any, argArray?: any): any;
    call(this: Function, thisArg: any, ...argArray: any[]): any;
    bind(this: Function, thisArg: any, ...argArray: any[]): any;
    toString(): string;
    prototype: any;
    readonly length: number;
    name: string;
}
interface FunctionConstructor {
    new (...args: string[]): Function;
    (...args: string[]): Function;
    readonly prototype: Function;
}
declare var Function: FunctionConstructor;

interface CallableFunction extends Function {}
interface NewableFunction extends Function {}

interface IArguments {
    [index: number]: any;
    length: number;
    callee: Function;
}

interface Boolean {
    valueOf(): boolean;
}
interface BooleanConstructor {
    new (value?: any): Boolean;
    <T>(value?: T): boolean;
    readonly prototype: Boolean;
}
declare var Boolean: BooleanConstructor;

interface Number {
    toString(radix?: number): string;
    toFixed(fractionDigits?: number): string;
    toPrecision(precision?: number): string;
    valueOf(): number;
}
interface NumberConstructor {
    new (value?: any): Number;
    (value?: any): number;
    readonly prototype: Number;
    readonly MAX_VALUE: number;
    readonly MIN_VALUE: number;
    readonly NaN: number;
    readonly POSITIVE_INFINITY: number;
    readonly NEGATIVE_INFINITY: number;
    isInteger(number: unknown): boolean;
    isNaN(number: unknown): boolean;
    isFinite(number: unknown): boolean;
    parseFloat(string: string): number;
    parseInt(string: string, radix?: number): number;
}
declare var Number: NumberConstructor;

interface String {
    toString(): string;
    charAt(pos: number): string;
    charCodeAt(index: number): number;
    concat(...strings: string[]): string;
    indexOf(searchString: string, position?: number): number;
    lastIndexOf(searchString: string, position?: number): number;
    slice(start?: number, end?: number): string;
    substring(start: number, end?: number): string;
    toLowerCase(): string;
    toUpperCase(): string;
    trim(): string;
    split(separator: string | RegExp, limit?: number): string[];
    replace(searchValue: string | RegExp, replaceValue: string): string;
    includes(searchString: string, position?: number): boolean;
    startsWith(searchString: string, position?: number): boolean;
    endsWith(searchString: string, endPosition?: number): boolean;
    repeat(count: number): string;
    padStart(maxLength: number, fillString?: string): string;
    padEnd(maxLength: number, fillString?: string): string;
    readonly length: number;
    [index: number]: string;
}
interface StringConstructor {
    new (value?: any): String;
    (value?: any): string;
    readonly prototype: String;
    fromCharCode(...codes: number[]): string;
}
declare var String: StringConstructor;

interface RegExp {
    test(string: string): boolean;
    exec(string: string): RegExpExecArray | null;
    readonly source: string;
    readonly flags: string;
    readonly global: boolean;
    lastIndex: number;
}
interface RegExpExecArray extends Array<string> {
    index: number;
    input: string;
}
interface RegExpConstructor {
    new (pattern: string | RegExp, flags?: string): RegExp;
    (pattern: string | RegExp, flags?: string): RegExp;
    readonly prototype: RegExp;
}
declare var RegExp: RegExpConstructor;

interface Symbol {
    toString(): string;
    valueOf(): symbol;
    readonly description: string | undefined;
}
interface SymbolConstructor {
    readonly iterator: unique symbol;
    readonly asyncIterator: unique symbol;
    (description?: string | number): symbol;
    for(key: string): symbol;
}
declare var Symbol: SymbolConstructor;

interface IteratorYieldResult<TYield> {
    done?: false;
    value: TYield;
}
interface IteratorReturnResult<TReturn> {
    done: true;
    value: TReturn;
}
type IteratorResult<T, TReturn = any> = IteratorYieldResult<T> | IteratorReturnResult<TReturn>;

interface Iterator<T, TReturn = any, TNext = undefined> {
    next(...args: [] | [TNext]): IteratorResult<T, TReturn>;
    return?(value?: TReturn): IteratorResult<T, TReturn>;
    throw?(e?: any): IteratorResult<T, TReturn>;
}
interface Iterable<T> {
    [Symbol.iterator](): Iterator<T>;
}
interface IterableIterator<T> extends Iterator<T> {
    [Symbol.iterator](): IterableIterator<T>;
}
interface Generator<T = unknown, TReturn = any, TNext = unknown> extends Iterator<T, TReturn, TNext> {
    next(...args: [] | [TNext]): IteratorResult<T, TReturn>;
    return(value: TReturn): IteratorResult<T, TReturn>;
    throw(e: any): IteratorResult<T, TReturn>;
    [Symbol.iterator](): Generator<T, TReturn, TNext>;
}
interface AsyncIterator<T, TReturn = any, TNext = undefined> {
    next(...args: [] | [TNext]): Promise<IteratorResult<T, TReturn>>;
}
interface AsyncIterable<T> {
    [Symbol.asyncIterator](): AsyncIterator<T>;
}
interface AsyncIterableIterator<T> extends AsyncIterator<T> {
    [Symbol.asyncIterator](): AsyncIterableIterator<T>;
}
interface AsyncGenerator<T = unknown, TReturn = any, TNext = unknown> extends AsyncIterator<T, TReturn, TNext> {
    [Symbol.asyncIterator](): AsyncGenerator<T, TReturn, TNext>;
}

interface ArrayLike<T> {
    readonly length: number;
    readonly [n: number]: T;
}
interface ConcatArray<T> {
    readonly length: number;
    readonly [n: number]: T;
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
}

interface Array<T> {
    length: number;
    toString(): string;
    concat(...items: ConcatArray<T>[]): T[];
    concat(...items: (T | ConcatArray<T>)[]): T[];
    join(separator?: string): string;
    pop(): T | undefined;
    push(...items: T[]): number;
    shift(): T | undefined;
    unshift(...items: T[]): number;
    reverse(): T[];
    slice(start?: number, end?: number): T[];
    splice(start: number, deleteCount?: number): T[];
    sort(compareFn?: (a: T, b: T) => number): this;
    indexOf(searchElement: T, fromIndex?: number): number;
    lastIndexOf(searchElement: T, fromIndex?: number): number;
    includes(searchElement: T, fromIndex?: number): boolean;
    find(predicate: (value: T, index: number, obj: T[]) => boolean): T | undefined;
    findIndex(predicate: (value: T, index: number, obj: T[]) => boolean): number;
    forEach(callbackfn: (value: T, index: number, array: T[]) => void): void;
    map<U>(callbackfn: (value: T, index: number, array: T[]) => U): U[];
    filter(predicate: (value: T, index: number, array: T[]) => boolean): T[];
    reduce(callbackfn: (acc: T, value: T, index: number, array: T[]) => T): T;
    reduce<U>(callbackfn: (acc: U, value: T, index: number, array: T[]) => U, initialValue: U): U;
    some(predicate: (value: T, index: number, array: T[]) => boolean): boolean;
    every(predicate: (value: T, index: number, array: T[]) => boolean): boolean;
    fill(value: T, start?: number, end?: number): this;
    [Symbol.iterator](): IterableIterator<T>;
    [n: number]: T;
}
interface ReadonlyArray<T> {
    readonly length: number;
    toString(): string;
    concat(...items: ConcatArray<T>[]): T[];
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
    indexOf(searchElement: T, fromIndex?: number): number;
    includes(searchElement: T, fromIndex?: number): boolean;
    find(predicate: (value: T, index: number, obj: readonly T[]) => boolean): T | undefined;
    forEach(callbackfn: (value: T, index: number, array: readonly T[]) => void): void;
    map<U>(callbackfn: (value: T, index: number, array: readonly T[]) => U): U[];
    filter(predicate: (value: T, index: number, array: readonly T[]) => boolean): T[];
    every(predicate: (value: T, index: number, array: readonly T[]) => boolean): boolean;
    some(predicate: (value: T, index: number, array: readonly T[]) => boolean): boolean;
    [Symbol.iterator](): IterableIterator<T>;
    readonly [n: number]: T;
}
interface ArrayConstructor {
    new (arrayLength?: number): any[];
    new <T>(arrayLength: number): T[];
    new <T>(...items: T[]): T[];
    (arrayLength?: number): any[];
    <T>(...items: T[]): T[];
    isArray(arg: any): arg is any[];
    from<T>(iterable: Iterable<T> | ArrayLike<T>): T[];
    of<T>(...items: T[]): T[];
    readonly prototype: any[];
}
declare var Array: ArrayConstructor;

interface TemplateStringsArray extends ReadonlyArray<string> {
    readonly raw: readonly string[];
}

interface Promise<T> {
    then<R1 = T, R2 = never>(
        onfulfilled?: ((value: T) => R1 | PromiseLike<R1>) | undefined | null,
        onrejected?: ((reason: any) => R2 | PromiseLike<R2>) | undefined | null
    ): Promise<R1 | R2>;
    catch<R = never>(onrejected?: ((reason: any) => R | PromiseLike<R>) | undefined | null): Promise<T | R>;
    finally(onfinally?: (() => void) | undefined | null): Promise<T>;
}
interface PromiseLike<T> {
    then<R1 = T, R2 = never>(
        onfulfilled?: ((value: T) => R1 | PromiseLike<R1>) | undefined | null,
        onrejected?: ((reason: any) => R2 | PromiseLike<R2>) | undefined | null
    ): PromiseLike<R1 | R2>;
}
interface PromiseConstructor {
    readonly prototype: Promise<any>;
    new <T>(executor: (resolve: (value: T | PromiseLike<T>) => void, reject: (reason?: any) => void) => void): Promise<T>;
    resolve(): Promise<void>;
    resolve<T>(value: T | PromiseLike<T>): Promise<T>;
    reject<T = never>(reason?: any): Promise<T>;
    all<T>(values: Iterable<T | PromiseLike<T>>): Promise<T[]>;
    race<T>(values: Iterable<T | PromiseLike<T>>): Promise<T>;
}
declare var Promise: PromiseConstructor;

interface Error {
    name: string;
    message: string;
    stack?: string;
}
interface ErrorConstructor {
    new (message?: string): Error;
    (message?: string): Error;
    readonly prototype: Error;
}
declare var Error: ErrorConstructor;
interface TypeError extends Error {}
declare var TypeError: ErrorConstructor;
interface RangeError extends Error {}
declare var RangeError: ErrorConstructor;
interface SyntaxError extends Error {}
declare var SyntaxError: ErrorConstructor;

interface Map<K, V> {
    readonly size: number;
    get(key: K): V | undefined;
    set(key: K, value: V): this;
    has(key: K): boolean;
    delete(key: K): boolean;
    clear(): void;
    forEach(callbackfn: (value: V, key: K, map: Map<K, V>) => void): void;
    keys(): IterableIterator<K>;
    values(): IterableIterator<V>;
    entries(): IterableIterator<[K, V]>;
    [Symbol.iterator](): IterableIterator<[K, V]>;
}
interface MapConstructor {
    new <K, V>(entries?: readonly (readonly [K, V])[] | null): Map<K, V>;
    new (): Map<any, any>;
    readonly prototype: Map<any, any>;
}
declare var Map: MapConstructor;
interface ReadonlyMap<K, V> {
    readonly size: number;
    get(key: K): V | undefined;
    has(key: K): boolean;
    forEach(callbackfn: (value: V, key: K, map: ReadonlyMap<K, V>) => void): void;
}

interface Set<T> {
    readonly size: number;
    add(value: T): this;
    has(value: T): boolean;
    delete(value: T): boolean;
    clear(): void;
    forEach(callbackfn: (value: T, value2: T, set: Set<T>) => void): void;
    keys(): IterableIterator<T>;
    values(): IterableIterator<T>;
    entries(): IterableIterator<[T, T]>;
    [Symbol.iterator](): IterableIterator<T>;
}
interface SetConstructor {
    new <T>(values?: readonly T[] | null): Set<T>;
    new (): Set<any>;
    readonly prototype: Set<any>;
}
declare var Set: SetConstructor;
interface ReadonlySet<T> {
    readonly size: number;
    has(value: T): boolean;
    forEach(callbackfn: (value: T, value2: T, set: ReadonlySet<T>) => void): void;
}

interface WeakMap<K extends object, V> {
    get(key: K): V | undefined;
    set(key: K, value: V): this;
    has(key: K): boolean;
    delete(key: K): boolean;
}
interface WeakMapConstructor {
    new <K extends object, V>(entries?: readonly [K, V][] | null): WeakMap<K, V>;
    readonly prototype: WeakMap<object, any>;
}
declare var WeakMap: WeakMapConstructor;

interface WeakSet<T extends object> {
    add(value: T): this;
    has(value: T): boolean;
    delete(value: T): boolean;
}
interface WeakSetConstructor {
    new <T extends object>(values?: readonly T[] | null): WeakSet<T>;
    readonly prototype: WeakSet<object>;
}
declare var WeakSet: WeakSetConstructor;

interface Math {
    readonly PI: number;
    readonly E: number;
    abs(x: number): number;
    ceil(x: number): number;
    floor(x: number): number;
    round(x: number): number;
    trunc(x: number): number;
    sign(x: number): number;
    sqrt(x: number): number;
    cbrt(x: number): number;
    pow(x: number, y: number): number;
    exp(x: number): number;
    log(x: number): number;
    min(...values: number[]): number;
    max(...values: number[]): number;
    random(): number;
    sin(x: number): number;
    cos(x: number): number;
    tan(x: number): number;
    atan2(y: number, x: number): number;
    hypot(...values: number[]): number;
}
declare var Math: Math;

interface JSON {
    parse(text: string, reviver?: (this: any, key: string, value: any) => any): any;
    stringify(value: any, replacer?: (this: any, key: string, value: any) => any, space?: string | number): string;
}
declare var JSON: JSON;

interface Date {
    getTime(): number;
    getFullYear(): number;
    getMonth(): number;
    getDate(): number;
    getHours(): number;
    getMinutes(): number;
    getSeconds(): number;
    toISOString(): string;
    toString(): string;
    valueOf(): number;
}
interface DateConstructor {
    new (): Date;
    new (value: number | string): Date;
    new (year: number, monthIndex: number, date?: number): Date;
    (): string;
    readonly prototype: Date;
    now(): number;
    parse(s: string): number;
}
declare var Date: DateConstructor;

interface Console {
    log(...data: any[]): void;
    info(...data: any[]): void;
    warn(...data: any[]): void;
    error(...data: any[]): void;
    debug(...data: any[]): void;
    trace(...data: any[]): void;
    assert(condition?: boolean, ...data: any[]): void;
}
declare var console: Console;

interface BigInt {
    toString(radix?: number): string;
    valueOf(): bigint;
}
interface BigIntConstructor {
    (value: bigint | boolean | number | string): bigint;
    readonly prototype: BigInt;
}
declare var BigInt: BigIntConstructor;

// Global functions
declare function parseInt(string: string, radix?: number): number;
declare function parseFloat(string: string): number;
declare function isNaN(number: number): boolean;
declare function isFinite(number: number): boolean;
declare function encodeURIComponent(uriComponent: string | number | boolean): string;
declare function decodeURIComponent(encodedURIComponent: string): string;
declare function structuredClone<T>(value: T): T;
declare function setTimeout(handler: () => void, timeout?: number): number;
declare function clearTimeout(id: number): void;

// ── Utility types ───────────────────────────────────────────────────────────
type Partial<T> = { [P in keyof T]?: T[P] };
type Required<T> = { [P in keyof T]-?: T[P] };
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Record<K extends keyof any, T> = { [P in K]: T };
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type NonNullable<T> = T & {};
type Parameters<T extends (...args: any) => any> = T extends (...args: infer P) => any ? P : never;
type ConstructorParameters<T extends abstract new (...args: any) => any> = T extends abstract new (...args: infer P) => any ? P : never;
type ReturnType<T extends (...args: any) => any> = T extends (...args: any) => infer R ? R : any;
type InstanceType<T extends abstract new (...args: any) => any> = T extends abstract new (...args: any) => infer R ? R : any;
type Awaited<T> = T extends PromiseLike<infer U> ? Awaited<U> : T;
type Uppercase<S extends string> = intrinsic;
type Lowercase<S extends string> = intrinsic;
type Capitalize<S extends string> = intrinsic;
type Uncapitalize<S extends string> = intrinsic;
interface ThisType<T> {}
