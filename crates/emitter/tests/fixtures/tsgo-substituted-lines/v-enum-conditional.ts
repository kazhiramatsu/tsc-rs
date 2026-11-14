declare const x: number;
export enum E {
    A = x,
    B = x === 0
        ? A
        : x + 1,
}
