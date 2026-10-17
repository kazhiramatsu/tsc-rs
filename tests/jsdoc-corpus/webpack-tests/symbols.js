export class C {
	constructor() {
		/** @type {Map<string, { source: string, info: number }>} */
		this._embedded = new Map();
	}
}
/**
 * @param {string} name
 * @returns {{ name: string, stage: number }}
 */
export const normalize = (name) => ({ name, stage: 0 });
