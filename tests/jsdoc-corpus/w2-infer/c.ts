type Toplevel = { body: string[] };
const readSource = <T,>(text: string, convert: (program: { kind: string }) => T): { result: T; table: number } => ({ result: convert({ kind: text }), table: 1 });
const adopt = (program: { kind: string }): Toplevel => ({ body: [program.kind] });
const { result: toplevel, table } = readSource("x", (program) => adopt(program));
const n: number = toplevel.body;
export { n, table };
