declare var console: { log(msg: any): void; };
let multiRobotA: [string, [string, string]] = ["mower", ["mowing", ""]];
for (let [, nameA] of [multiRobotA]) { console.log(nameA); }
for (let [...multiRobotAInfo] = multiRobotA, i = 0; i < 1; i++) { console.log(multiRobotAInfo); }
for (let [...multiRobotBInfo] = ["trimmer", ["trimming", "edging"]], j = 0; j < 1; j++) { console.log(multiRobotBInfo); }
