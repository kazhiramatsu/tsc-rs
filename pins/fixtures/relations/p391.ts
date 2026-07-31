// @noLib: true

// relpin p391: assignable source="number" target="{ toFixed(digits: number): string }"
interface Number { toFixed(digits: number): string }
declare var s: number;
var t: { toFixed(digits: number): string } = s;
