"use strict";
const ContextDependencyHelpers = require("./helpers");
class Dep { /** @param {import("./options").ContextOptions} o @param {number} r */ constructor(o, r) { this.o = o; this.r = r; } }
const dep = ContextDependencyHelpers.create(Dep, "req", { recursive: true });
const parsed = ContextDependencyHelpers.parseObjectExpression("x");
module.exports = { dep, parsed };
