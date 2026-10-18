"use strict";
const ContextDependencyHelpers = require("./helpers");
const dep = ContextDependencyHelpers.create("req", { recursive: true });
const parsed = ContextDependencyHelpers.parseObjectExpression("x");
module.exports = { dep, parsed };
