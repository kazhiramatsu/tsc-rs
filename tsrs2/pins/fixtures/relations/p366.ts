// @noLib: true

// relpin p366: assignable source="CoVoid<{ a: string }>" target="CoVoid<void>"
interface CoVoid<T> { x: T }
declare var s: CoVoid<{ a: string }>;
var t: CoVoid<void> = s;
