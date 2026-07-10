// @noLib: true

// relpin p175: assignable source="string" target="`a${string}`"
declare var s: string;
var t: `a${string}` = s;
