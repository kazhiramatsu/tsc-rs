// @noLib: true

// relpin p408: assignable source="V59<{ x: number }>" target="V59<{ x: number | string }>"
interface V59<T extends { x: unknown }> { v: T["x"] }
declare var s: V59<{ x: number }>;
var t: V59<{ x: number | string }> = s;
