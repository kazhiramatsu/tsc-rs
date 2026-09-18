function f() { first(); /* F_BETWEEN */ second(); }
class B { m() { first(); /* M_BETWEEN */ second(); } }
class A {
    #method() { first(); /* PRIVATE_BODY_BETWEEN */ second(); } // PRIVATE_MEMBER_OUTER_TRAILING
}
const o = { m() { first(); /* O_BETWEEN */ second(); } };
declare function first(): void; declare function second(): void;
