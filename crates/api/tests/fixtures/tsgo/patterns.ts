const [, a, , b = 1, ...c] = [1, 2, 3, 4, 5];
let { x, y: [z, , w] } = { x: 1, y: [1, 2, 3] };
for (const [k, , v] of [[1, 2, 3]]) {}
