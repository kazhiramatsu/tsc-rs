"use strict";
let multiRobotA = ["mower", ["mowing", ""]];
for (let [, nameA] of [multiRobotA]) {
    console.log(nameA);
}
for (let [...multiRobotAInfo] = multiRobotA, i = 0; i < 1; i++) {
    console.log(multiRobotAInfo);
}
for (let [...multiRobotBInfo] = ["trimmer", ["trimming", "edging"]], j = 0; j < 1; j++) {
    console.log(multiRobotBInfo);
}
//# sourceMappingURL=main.js.map