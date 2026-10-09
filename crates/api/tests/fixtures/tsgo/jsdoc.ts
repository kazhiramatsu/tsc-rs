/**
 * Text with a {@link Foo} link, {@linkcode Bar.baz} and {@link A#b}.
 * @param x the x
 * @param {string} y - the y
 * @returns nothing
 * @deprecated use g
 * @template T, U
 */
async function f<T, U>(x: number, y: string): Promise<void> {}

/** plain comment */
const a = 1;

/** @see {@link f} */
function g() { return import("./x"); }

/** @type {import("./y").T} */
let c;

class K {
    /** member */
    m(): void {}
    /**
     * @param p first
     */
    constructor(p: number) {}
}

/** Only a link: {@link K} */
interface L {}

/** */
type M = 1;
