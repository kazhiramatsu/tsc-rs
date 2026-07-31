// @noLib: true

// relpin p168: assignable source="TA" target="TB"
interface TA { self(): TB }
interface TB { self(): TA }
declare var s: TA;
var t: TB = s;
