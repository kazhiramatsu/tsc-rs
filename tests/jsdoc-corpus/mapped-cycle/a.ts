type P = { extra?: { raw?: string } };
interface A { readonly type: "A"; readonly parent: _Node; readonly value: B; }
interface B { readonly type: "B"; readonly parent: _Node; }
interface C { readonly type: "C"; readonly items: _Node[]; }
type _Node = A | B | C;
type ExtendNode<Input> = Input extends _Node
  ? { [Key in keyof Input]: ExtendNode<Input[Key]> } & P
  : Input extends readonly any[] ? ExtendNode<Input[number]>[] : Input;
type Node = ExtendNode<_Node>;
declare const n: Node;
n.id;
declare const m: Node;
if (m.type === "A") { m.value.id; m.parent.id; }
export {};
