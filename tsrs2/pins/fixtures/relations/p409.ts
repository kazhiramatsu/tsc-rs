// @noLib: true

// relpin p409: assignable source="V59<{ x: number | string }>" target="V59<{ x: number }>"
interface V59<T extends { x: unknown }> { v: T["x"] }
declare var s: V59<{ x: number | string }>;
var t: V59<{ x: number }> = s;
