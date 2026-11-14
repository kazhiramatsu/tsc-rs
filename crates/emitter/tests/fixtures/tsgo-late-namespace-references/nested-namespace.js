"use strict";
var Outer;
(function (Outer) {
    let Inner;
    (function (Inner) {
        Inner.x = Inner.React.createElement("div", null);
    })(Inner = Outer.Inner || (Outer.Inner = {}));
})(Outer || (Outer = {}));
