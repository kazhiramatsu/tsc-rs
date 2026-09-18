"use strict";
var _A_instances, _A_method;
function f() { first(); /* F_BETWEEN */ /* F_BETWEEN */ second(); }
class B {
    m() { first(); /* M_BETWEEN */ /* M_BETWEEN */ second(); }
}
class A {
    constructor() {
        _A_instances.add(this);
    }
}
_A_instances = new WeakSet(), _A_method = function _A_method() { first(); /* PRIVATE_BODY_BETWEEN */ /* PRIVATE_BODY_BETWEEN */ second(); };
const o = { m() { first(); /* O_BETWEEN */ /* O_BETWEEN */ second(); } };
