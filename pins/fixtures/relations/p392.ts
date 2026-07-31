// @noLib: true

// relpin p392: assignable source="boolean" target="{ valueOf(): boolean }"
interface Boolean { valueOf(): boolean }
declare var s: boolean;
var t: { valueOf(): boolean } = s;
