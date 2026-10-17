const Dep = require("./Dep");
/** @typedef {{ id: number }} Meta */
/** @typedef {(err: Error | null) => void} ErrorCallback */
/**
 * @callback DoneCallback
 * @param {Error | null} err error
 * @returns {void}
 */
/**
 * @typedef {object} ApplyOptions
 * @property {Iterable<string>=} chunks chunks
 * @property {Meta=} meta the meta
 */
class Store {}
Store.List = class List {};
module.exports = { Store, List: Store.List };
