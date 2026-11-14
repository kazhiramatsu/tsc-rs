export namespace N {
    export const a = 1;
    export function f(x: number | undefined) {
        return x === undefined
            ? a
            : x + 1;
    }
}
