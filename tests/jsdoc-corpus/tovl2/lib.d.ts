/**
 * @overload
 *
 * Reads ignore files and returns objects with the ignore patterns.
 *
 * @param {string[]} paths The paths of ignore files to include.
 * @returns {string[]}
 */
export function includeIgnoreFile(paths: string[]): string[];
/**
 * @overload
 *
 * Reads an ignore file and returns an object with the ignore patterns.
 *
 * @param {string} paths The path of the ignore file to include.
 * @returns {string}
 */
export function includeIgnoreFile(paths: string): string;
