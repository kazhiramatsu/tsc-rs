const a = 1;
export function f(x) {
    return x === undefined
        ? a
        : x + 1;
}
