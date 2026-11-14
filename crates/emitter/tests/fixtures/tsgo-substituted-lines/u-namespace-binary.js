export var N;
(function (N) {
    N.a = 1;
    function f(x) {
        return N.a + x;
    }
    N.f = f;
})(N || (N = {}));
