// @noLib: true

// relpin p390: assignable source="string" target="{ length: number, missing: boolean }"
interface String { length: number }
declare var s: string;
var t: { length: number, missing: boolean } = s;
