/**
 * @param {string[]} names names
 * @param {Map<string, number>} map map
 * @param {Set<number>} set set
 * @returns {number} n
 */
function count(names, map, set) {
	return names.length + map.size + set.size;
}
module.exports = count;
