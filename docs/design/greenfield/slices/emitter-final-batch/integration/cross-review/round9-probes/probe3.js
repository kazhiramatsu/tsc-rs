module.exports = ({ ts, program, files }) => {
  const checker = program.getTypeChecker();
  const lazy = program.getSourceFile("/.src/LazySet.js");
  const idx = program.getSourceFile("/.src/index.js");
  function find(sf, pred) { let r; (function walk(n){ if (!r && pred(n)) r = n; ts.forEachChild(n, walk);} )(sf); return r; }
  const cls = find(lazy, n => ts.isClassDeclaration(n));
  const orig = cls.symbol; // binder symbol
  const merged = checker.getMergedSymbol(orig);
  const dump = (label, s) => console.log(label, "flags", s.flags, "transient", !!(s.flags & ts.SymbolFlags.Transient), "members", s.members && [...s.members.keys()], "exports", s.exports && [...s.exports.keys()], "decls", s.declarations && s.declarations.map(d=>ts.SyntaxKind[d.kind]));
  dump("orig:", orig); dump("merged:", merged);
  dump("file:", lazy.symbol);
  const t1 = checker.getTypeAtLocation(find(idx, n => ts.isVariableDeclaration(n) && n.name.getText()==="stringSet").name);
  const t2 = checker.getTypeAtLocation(find(lazy, n => ts.isParameter(n)).name);
  console.log("declaredType(orig) is t2:", checker.getDeclaredTypeOfSymbol(orig) === t2, "is t1:", checker.getDeclaredTypeOfSymbol(orig) === t1);
  console.log("declaredType(merged) is t1:", checker.getDeclaredTypeOfSymbol(merged) === t1, "is t2:", checker.getDeclaredTypeOfSymbol(merged) === t2);
  console.log("t1.symbol===merged", t1.symbol===merged, "t2.symbol===orig", t2.symbol===orig);
  // module.exports = LazySet resolution
  const exportEq = lazy.symbol.exports.get("export=");
  dump("export=:", exportEq);
  console.log("aliased(export=) === merged:", checker.getAliasedSymbol(exportEq) === merged);
};
