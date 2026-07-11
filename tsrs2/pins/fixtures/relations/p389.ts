// @noLib: true

// relpin p389: assignable source="string" target="{ length: number }"
interface String { length: number }
declare var s: string;
var t: { length: number } = s;
