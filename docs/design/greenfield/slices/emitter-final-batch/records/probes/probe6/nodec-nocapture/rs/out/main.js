"use strict";
try {
    var Foo = /** @class */ (function () {
        function Foo() {
        }
        Foo.func = function () { return 1; };
        return Foo;
    }());
    Foo.func();
}
catch (e) { }
