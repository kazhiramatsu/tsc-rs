"use strict";
var M;
(function (M) {
})(M || (M = {}));
(function (M) {
    // Should emit M.React.createElement
    //  and M.React.__spread
    var foo;
    var spread1 = M.React.createElement("div", Object.assign({ x: '' }, foo, { y: '' }));
    // Quotes
    var x = M.React.createElement("div", null, "This \"quote\" thing");
})(M || (M = {}));
