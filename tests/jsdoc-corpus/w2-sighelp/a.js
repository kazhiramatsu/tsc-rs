/**
 * @typedef {object} Utils
 * @property {(phase: number | undefined) => boolean} isEvaluation true if evaluation
 * @property {(phase: number) => string} stringify name of phase
 */

/** @type {Utils} */
const Utils = {
	isEvaluation(phase) {
		return phase === 1;
	},
	stringify(phase) {
		return String(phase);
	}
};
const ok = Utils.isEvaluation(1);
const name = Utils.stringify(2);
module.exports = { ok, name };
