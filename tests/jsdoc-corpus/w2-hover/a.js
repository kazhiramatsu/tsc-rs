/** @typedef {{ id: number }} Module */
/**
 * @param {Module[]} modules modules
 * @param {Set<Module>} set set
 * @param {Map<string, number>} map map
 * @returns {number} n
 */
function count(modules, set, map) {
	return modules.length + set.size + map.size;
}
module.exports = { count };
