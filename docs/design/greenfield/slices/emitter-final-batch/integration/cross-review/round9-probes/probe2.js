module.exports = ({ ts, program }) => {
  const checker = program.getTypeChecker();
  const idx = program.getSourceFile("/.src/index.js");
  const lazy = program.getSourceFile("/.src/LazySet.js");
  function find(sf, pred) { let r; (function walk(n){ if (!r && pred(n)) r = n; ts.forEachChild(n, walk);} )(sf); return r; }
  const stringSetDecl = find(idx, n => ts.isVariableDeclaration(n) && n.name.getText() === "stringSet");
  const t1 = checker.getTypeAtLocation(stringSetDecl.name);
  const param = find(lazy, n => ts.isParameter(n) && n.name.getText() === "iterable");
  const t2 = checker.getTypeAtLocation(param.name);
  const show = (label, t) => {
    const s = t.getSymbol();
    console.log(label, checker.typeToString(t), "| typeId", t.id, "| symbol", s && s.escapedName, "flags", s && s.flags,
      "decls", s && s.declarations && s.declarations.map(d => ts.SyntaxKind[d.kind] + "@" + d.getSourceFile().fileName + ":" + d.pos),
      "| props", checker.getPropertiesOfType(t).map(p => p.escapedName));
  };
  show("stringSet:", t1); show("param:", t2);
  console.log("same type object:", t1 === t2);
  // how does the name LazySet in index.js resolve?
  const typeRef = ts.getJSDocType(stringSetDecl);
  const nameSym = checker.getSymbolAtLocation(typeRef.typeName);
  console.log("name symbol:", nameSym && nameSym.escapedName, nameSym && nameSym.flags, nameSym && nameSym.declarations.map(d=>ts.SyntaxKind[d.kind]));
  const aliased = nameSym && (nameSym.flags & ts.SymbolFlags.Alias) ? checker.getAliasedSymbol(nameSym) : null;
  console.log("aliased:", aliased && aliased.escapedName, aliased && aliased.flags, aliased && aliased.declarations.map(d=>ts.SyntaxKind[d.kind]+":"+d.getSourceFile().fileName));
  const cls = find(lazy, n => ts.isClassDeclaration(n));
  const clsSym = checker.getSymbolAtLocation(cls.name);
  console.log("class symbol flags", clsSym.flags, "decls", clsSym.declarations.map(d=>ts.SyntaxKind[d.kind]), "same as aliased:", clsSym === aliased, "merged:", checker.getMergedSymbol(clsSym) === checker.getMergedSymbol(aliased));
  console.log("class decl jsDoc tags:", ts.getJSDocTags(cls).map(t=>ts.SyntaxKind[t.kind]));
};
