/** @import { Meta, ErrorCallback, DoneCallback, ApplyOptions } from "./Store" */
/** @import { List } from "./Store" */
/** @import Dep from "./Dep" */

/**
 * @typedef {object} Options
 * @property {Meta=} meta the meta
 * @property {InstanceType<List>=} list the list
 * @property {Dep[]=} deps deps
 * @property {Iterable<string>=} chunks chunks
 */

class Compiler {
	constructor() {
		/**
		 * Embedded sources.
		 * @type {Map<string, { source: string, info: Meta, jobModule: number }>}
		 */
		this._embedded = new Map();
		/**
		 * @deprecated
		 * @type {{ add: (item: string) => void }}
		 */
		this.compilationDependencies = {
			add: (item) => {}
		};
	}

	/** @typedef {{ range?: [number, number], value: string, code: boolean }} Calculated */

	/**
	 * @param {string | { name: string }} options hook options
	 * @returns {{ name: string, stage: number }} normalized
	 */
	normalize(options) {
		return { name: typeof options === "string" ? options : options.name, stage: 0 };
	}

	/**
	 * @param {Options} options options
	 * @param {ErrorCallback} callback callback
	 */
	run(options, callback) {
		const chunks = ["a"];
		this.apply({ chunks, meta: /** @type {Meta} */ ({ id: 1 }) });
		return callback(null);
	}

	/** @param {Options} options options */
	apply(options) {
		this.compilationDependencies.add("x");
		return options.meta;
	}

	/**
	 * @param {ApplyOptions} options options
	 * @param {DoneCallback} done done
	 */
	finish(options, done) {
		const chunks = ["b"];
		this.applyImported({ chunks, meta: { id: 2 } });
		return done(null);
	}

	/** @param {ApplyOptions} options options */
	applyImported(options) {
		return options.chunks;
	}

	/**
	 * @param {object} options options
	 * @param {Iterable<string>=} options.chunks chunks
	 * @param {string} options.fs file system
	 * @returns {void}
	 */
	processRequirements(options) {}

	report() {
		const chunks = ["c"];
		this.processRequirements({ chunks, fs: "memory" });
	}

	/**
	 * @param {function(): void} cb callback
	 * @param {ErrorCallback} ecb error callback
	 */
	later(cb, ecb) {
		cb();
		ecb(null);
	}
}
module.exports = Compiler;
