export var N;
(function (N) {
    N.a = 1;
    function f(x) {
        return x === undefined
            ? N.a : x + 1;
    }
    N.f = f;
})(N || (N = {}));
