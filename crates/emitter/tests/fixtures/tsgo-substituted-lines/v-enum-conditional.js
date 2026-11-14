export var E;
(function (E) {
    E[E["A"] = x] = "A";
    E[E["B"] = x === 0
        ? E.A : x + 1] = "B";
})(E || (E = {}));
