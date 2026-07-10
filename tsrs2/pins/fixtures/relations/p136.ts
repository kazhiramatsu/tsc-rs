// @noLib: true

// relpin p136: assignable source="() => void" target="{ a: number }"
declare var s: () => void;
var t: { a: number } = s;
