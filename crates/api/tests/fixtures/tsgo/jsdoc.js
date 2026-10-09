const fs = require("fs");
const { join } = require("path");
/**
 * Reads a file. See {@link fs.readFileSync}.
 * @see https://nodejs.org
 */
function read(name) { return fs.readFileSync(join(".", name)); }
/** @deprecated use read */
function old() { return require("./other"); }
async function later() { return import("./later"); }
exports.read = read;
