/** @typedef {{ js: 1 }} JsParser */
/** @typedef {{ jpo: 1 }} JsParserOptions */
/** @typedef {{ p: 1 }} Parser */
/** @typedef {{ po: 1 }} ParserOptions */
/**
 * @typedef {[
 * [any, JsParser, JsParserOptions],
 * [any, JsParser, JsParserOptions],
 * [any, JsParser, JsParserOptions],
 * [string, Parser, ParserOptions],
 * ]} Entries
 */
/**
 * @template {unknown[]} T
 * @template {number[]} I
 * @typedef {{ [K in keyof I]: K extends keyof I ? I[K] extends keyof T ? T[I[K]] : never : never }} ExtractTupleElements
 */
/**
 * @template {unknown[]} T
 * @template {number[]} A
 * @template [R=void]
 * @typedef {T extends [infer Head extends [string, ...unknown[]], ...infer Tail extends [string, ...unknown[]][]] ? Record<Head[0], SyncBailHook<ExtractTupleElements<Head, A>, R extends number ? Head[R] : R>> & RecordFactoryFromTuple<Tail, A, R> : unknown } RecordFactoryFromTuple
 */
/** @type {TypedHookMap<RecordFactoryFromTuple<Entries, [2], 1>>} */
const map = /** @type {any} */ ({});
/** @type {number} */
const n = map.for("c");
module.exports = { n };
