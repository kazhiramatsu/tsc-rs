"use strict";
module.exports = /** @class */ (function () {
    /**
     * @param {number} p
     */
    function exports(p) {
        this.t = 12 + p;
    }
    return exports;
}());
module.exports.Sub = /** @class */ (function () {
    function Sub() {
        this.instance = new module.exports(10);
    }
    return Sub;
}());
