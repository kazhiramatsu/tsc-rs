"use strict";
try {
    var Foo_1 = /** @class */ (function () {
        function Foo() {
        }
        Foo.func = function () { return new Foo(); };
        return Foo;
    }());
    Foo_1.func();
}
catch (e) { }
