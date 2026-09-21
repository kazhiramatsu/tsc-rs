module.exports = ({ ts, program }) => {
  const checker = program.getTypeChecker();
  const lazy = program.getSourceFile("/.src/LazySet.js");
  const idx = program.getSourceFile("/.src/index.js");
  function find(sf, pred) { let r; (function walk(n){ if (!r && pred(n)) r = n; ts.forEachChild(n, walk);} )(sf); return r; }
  const order = process.env.ORDER || "clone-first";
  const getClone = () => checker.getTypeAtLocation(find(idx, n => ts.isVariableDeclaration(n) && n.name.getText()==="stringSet").name);
  const getOrig = () => checker.getTypeAtLocation(find(lazy, n => ts.isParameter(n)).name);
  const props = t => checker.getPropertiesOfType(t).map(p=>p.escapedName).join(",");
  if (order === "clone-first") { const a = getClone(); console.log("clone props:", props(a)); const b = getOrig(); console.log("orig props:", props(b)); }
  else { const b = getOrig(); console.log("orig props:", props(b)); const a = getClone(); console.log("clone props:", props(a)); }
};
