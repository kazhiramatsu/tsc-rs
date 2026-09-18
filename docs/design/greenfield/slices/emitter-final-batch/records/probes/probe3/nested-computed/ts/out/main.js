"use strict";
var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {
    if (typeof name === "symbol") name = name.description ? "[".concat(name.description, "]") : "";
    return Object.defineProperty(f, "name", { configurable: true, value: prefix ? "".concat(prefix, " ", name) : name });
};
var _a;
Object.defineProperty(exports, "__esModule", { value: true });
exports.tail = exports.Foo = void 0;
var Foo = (_a = /** @class */ (function () {
        function class_1() {
        }
        return class_1;
    }()),
    __setFunctionName(_a, "Foo"),
    _a.key = 'member',
    _a.nested = /** @class */ (function () {
        function class_2() {
        }
        class_2.prototype[this.key] = function () { };
        return class_2;
    }()),
    _a);
exports.Foo = Foo;
exports.tail = 1;
//# sourceMappingURL=main.js.map