//! A node printed for an insertion (tsgo `PrintAndPositionNode`,
//! `CreateSyntheticSourceFile`) and formatted where it goes
//! (`handleFormatNodeForInsertion`): the positions of the printed nodes and
//! lists, the text without a source file, and the formatted insertions, all
//! as tsgo gives them (taken from tsgo with a Go probe).

use tsc_emitter::{
    print_and_position, print_and_position_node, transform_nodes, NewLineKind, PositionedNode,
    PositionedPrint, TransformArena, TransformNode,
};
use tsc_ls::lsutil::UserPreferences;
use tsc_ls::tree::{kind_name, Tree, View};
use tsc_syntax::{
    for_each_child, for_each_child_array, parse_source_file, skip_trivia,
    ExternalModuleIndicatorOptions, LanguageVariant, NodeArena, NodeData, NodeId, ParseOptions,
    ScriptKind, SourceFile, SyntaxKind,
};

/// The node as the API decodes it: no positions, no parents, no trailing
/// commas.
fn synthesize(arena: &mut NodeArena, node: NodeId) {
    let mut children = Vec::new();
    for_each_child(arena, arena.node(node), |child| {
        children.push(child);
        false
    });
    let mut arrays = Vec::new();
    for_each_child_array(arena.node(node), |array| {
        arrays.push(array);
        false
    });
    for array in arrays {
        let record = arena.node_array_mut(array);
        record.pos = u32::MAX;
        record.end = u32::MAX;
        record.has_trailing_comma = false;
    }
    let record = arena.node_mut(node);
    record.pos = u32::MAX;
    record.end = u32::MAX;
    record.parent = None;
    for child in children {
        synthesize(arena, child);
    }
}

/// The first statement of `text`, decoded, printed and positioned.
fn print_decoded(
    text: &str,
    script_kind: ScriptKind,
    options: (NewLineKind, usize),
    variant: LanguageVariant,
) -> PositionedNode {
    let file_name = if script_kind == ScriptKind::Tsx {
        "/n.tsx"
    } else {
        "/n.ts"
    };
    let source = parse_source_file(
        file_name,
        text.to_owned(),
        ParseOptions {
            script_kind,
            ..ParseOptions::default()
        },
        None,
    );
    let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
        unreachable!()
    };
    let statement = source.arena.node_array(data.statements.unwrap()).nodes[0];
    // The decoder keeps a single-quoted literal's quote as a flag.
    let single_quoted: Vec<NodeId> = source
        .arena
        .node_ids()
        .filter(|&id| {
            let node = source.arena.node(id);
            node.kind == SyntaxKind::StringLiteral
                && source.text()[skip_trivia(source.text(), node.pos as usize)..].starts_with('\'')
        })
        .collect();
    let mut arena = source.arena.detached_clone();
    synthesize(&mut arena, statement);
    let decoded = SourceFile::from_tree(
        "",
        None,
        arena,
        statement,
        LanguageVariant::Standard,
        ScriptKind::Unknown,
        false,
        ExternalModuleIndicatorOptions::default(),
    );
    print_and_position_node(
        &decoded,
        statement,
        &single_quoted,
        options.0,
        options.1,
        variant,
    )
    .unwrap()
}

/// tsgo's tree of a positioned node, each node and list with its range.
fn dump(tree: &Tree<'_>, view: View, depth: usize, out: &mut String) {
    let (pos, end) = tree.range(view);
    let name = match tree.kind(view) {
        Some(_) => kind_name(tree.kind_number(view))
            .trim_start_matches("Kind")
            .to_owned(),
        None => "List".to_owned(),
    };
    out.push_str(&format!(
        "{}{name} {} {}\n",
        " ".repeat(depth),
        pos as i32,
        end as i32
    ));
    let mut children = Vec::new();
    tree.children(view, &mut children);
    for child in children {
        dump(tree, child, depth + 1, out);
    }
}

/// A statement of a TSX file printed with tsgo's defaults (a new line,
/// four spaces).
#[track_caller]
fn assert_positions(text: &str, printed: &str, expected: &str) {
    let positioned = print_decoded(
        text,
        ScriptKind::Tsx,
        (NewLineKind::LineFeed, 4),
        LanguageVariant::Jsx,
    );
    assert_eq!(positioned.text, printed, "{text}");
    let tree = Tree::detached(&positioned.file);
    let mut out = String::new();
    dump(&tree, View::Node(positioned.node), 0, &mut out);
    assert_eq!(out, expected, "{text}");
}

#[track_caller]
fn assert_insertion(target: &str, position: usize, node: &str, settings: &str, expected: &str) {
    let target = parse_source_file(
        "/t.ts",
        target.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    let preferences = UserPreferences::from_json(&serde_json::from_str(settings).unwrap()).unwrap();
    let options = &preferences.format_code_settings;
    let new_line = if options.editor_settings.new_line_character == "\r\n" {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let indent = usize::try_from(options.editor_settings.indent_size).unwrap_or(4);
    let positioned = print_decoded(
        node,
        ScriptKind::Ts,
        (new_line, indent),
        target.language_variant,
    );
    // The position is UTF-16 in the API; the tests' targets are ASCII up to it
    // but for one, converted here.
    let utf16: Vec<u16> = target.text().encode_utf16().collect();
    let position = String::from_utf16(&utf16[..position]).unwrap().len();
    let formatted = tsc_ls::format_node_for_insertion(&target, position, &positioned, options);
    assert_eq!(formatted, expected, "{node}");
}

#[test]
fn positions_of_printed_statements() {
    assert_positions(
        "const x = 1;",
        "const x = 1;",
        "VariableStatement 0 12\n VariableDeclarationList 0 11\n  List 5 11\n   VariableDeclaration 5 11\n    Identifier 5 7\n    NumericLiteral 9 11\n",
    );
    assert_positions(
        "import { a, b as c } from \"m\";",
        "import { a, b as c } from \"m\";",
        "ImportDeclaration 0 30\n ImportClause 6 20\n  NamedImports 6 20\n   List 8 18\n    ImportSpecifier 8 10\n     Identifier 8 10\n    ImportSpecifier 11 18\n     Identifier 11 13\n     Identifier 16 18\n StringLiteral 25 29\n",
    );
    assert_positions(
        "function f<T, U extends T = T>(a: T, ...b: U[]): void { return; }",
        "function f<T, U extends T = T>(a: T, ...b: U[]): void { return; }",
        "FunctionDeclaration 0 65\n Identifier 8 10\n List 11 29\n  TypeParameter 11 12\n   Identifier 11 12\n  TypeParameter 13 29\n   Identifier 13 15\n   TypeReference 23 25\n    Identifier 23 25\n   TypeReference 27 29\n    Identifier 27 29\n List 31 46\n  Parameter 31 35\n   Identifier 31 32\n   TypeReference 33 35\n    Identifier 33 35\n  Parameter 36 46\n   DotDotDotToken 36 40\n   Identifier 40 41\n   ArrayType 42 46\n    TypeReference 42 44\n     Identifier 42 44\n VoidKeyword 48 53\n Block 53 65\n  List 55 63\n   ReturnStatement 55 63\n",
    );
    assert_positions(
        "class C<T> extends B<T> implements I, J { private readonly x: number = 1; static m(a, b?) { } get p() { return 1; } }",
        "class C<T> extends B<T> implements I, J {\n    private readonly x: number = 1;\n    static m(a, b?) {\n    }\n    get p() { return 1; }\n}",
        "ClassDeclaration 0 133\n Identifier 5 7\n List 8 9\n  TypeParameter 8 9\n   Identifier 8 9\n List 10 39\n  HeritageClause 10 23\n   List 18 23\n    ExpressionWithTypeArguments 18 23\n     Identifier 18 20\n     List 21 22\n      TypeReference 21 22\n       Identifier 21 22\n  HeritageClause 23 39\n   List 34 39\n    TypeReference 34 36\n     Identifier 34 36\n    TypeReference 37 39\n     Identifier 37 39\n List 41 131\n  PropertyDeclaration 41 77\n   List -1 -1\n    PrivateKeyword 41 53\n    ReadonlyKeyword 53 62\n   Identifier 62 64\n   NumberKeyword 65 72\n   NumericLiteral 74 76\n  MethodDeclaration 77 105\n   List -1 -1\n    StaticKeyword 77 88\n   Identifier 88 90\n   List 91 96\n    Parameter 91 92\n     Identifier 91 92\n    Parameter 93 96\n     Identifier 93 95\n     QuestionToken 95 96\n   Block 97 105\n    List 99 99\n  GetAccessor 105 131\n   Identifier 113 115\n   List 116 116\n   Block 117 131\n    List 119 129\n     ReturnStatement 119 129\n      NumericLiteral 126 128\n",
    );
    assert_positions(
        "let o = { a: 1, b: [1, 2, , 3], c() {}, ...d, [e]: f };",
        "let o = { a: 1, b: [1, 2, , 3], c() {\n    }, ...d, [e]: f };",
        "VariableStatement 0 60\n VariableDeclarationList 0 59\n  List 3 59\n   VariableDeclaration 3 59\n    Identifier 3 5\n    ObjectLiteralExpression 7 59\n     List 9 57\n      PropertyAssignment 9 14\n       Identifier 9 11\n       NumericLiteral 12 14\n      PropertyAssignment 15 30\n       Identifier 15 17\n       ArrayLiteralExpression 18 30\n        List 20 29\n         NumericLiteral 20 21\n         NumericLiteral 22 24\n         OmittedExpression 25 25\n         NumericLiteral 27 29\n      MethodDeclaration 31 43\n       Identifier 31 33\n       List 34 34\n       Block 35 43\n        List 37 37\n      SpreadAssignment 44 49\n       Identifier 48 49\n      PropertyAssignment 50 57\n       ComputedPropertyName 50 54\n        Identifier 52 53\n       Identifier 55 57\n",
    );
    assert_positions(
        "f(a, (b) => b + 1, async c => { await c; });",
        "f(a, b => b + 1, async (c) => { await c; });",
        "ExpressionStatement 0 44\n CallExpression 0 43\n  Identifier 0 1\n  List 2 42\n   Identifier 2 3\n   ArrowFunction 4 15\n    List 4 6\n     Parameter 4 6\n      Identifier 4 6\n    EqualsGreaterThanToken 6 9\n    BinaryExpression 9 15\n     Identifier 9 11\n     PlusToken 11 13\n     NumericLiteral 13 15\n   ArrowFunction 16 42\n    List -1 -1\n     AsyncKeyword 16 22\n    List 24 25\n     Parameter 24 25\n      Identifier 24 25\n    EqualsGreaterThanToken 26 29\n    Block 29 42\n     List 31 40\n      ExpressionStatement 31 40\n       AwaitExpression 31 39\n        Identifier 37 39\n",
    );
    assert_positions(
        "switch (x) { case 1: f(); break; default: }",
        "switch (x) {\n    case 1:\n        f();\n        break;\n    default:\n}",
        "SwitchStatement 0 67\n Identifier 8 9\n CaseBlock 10 67\n  List 12 65\n   CaseClause 12 52\n    NumericLiteral 21 23\n    List 24 52\n     ExpressionStatement 24 37\n      CallExpression 24 36\n       Identifier 24 34\n       List 35 35\n     BreakStatement 37 52\n   DefaultClause 52 65\n    List 65 65\n",
    );
    assert_positions(
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "try {\n    a();\n} catch (e) {\n    b(e);\n} finally {\n    c();\n}",
        "TryStatement 0 61\n Block 3 16\n  List 5 14\n   ExpressionStatement 5 14\n    CallExpression 5 13\n     Identifier 5 11\n     List 12 12\n CatchClause 16 40\n  VariableDeclaration 24 25\n   Identifier 24 25\n  Block 26 40\n   List 28 38\n    ExpressionStatement 28 38\n     CallExpression 28 37\n      Identifier 28 34\n      List 35 36\n       Identifier 35 36\n Block 48 61\n  List 50 59\n   ExpressionStatement 50 59\n    CallExpression 50 58\n     Identifier 50 56\n     List 57 57\n",
    );
    assert_positions(
        "@dec() class D { @prop() p: string; constructor(@inject private s: S) {} }",
        "@dec()\nclass D {\n    @prop()\n    p: string;\n    constructor(\n    @inject\n    private s: S) {\n    }\n}",
        "ClassDeclaration 0 100\n List -1 -1\n  Decorator 0 6\n   CallExpression 1 6\n    Identifier 1 4\n    List 5 5\n Identifier 12 14\n List 16 98\n  PropertyDeclaration 16 43\n   List -1 -1\n    Decorator 16 28\n     CallExpression 22 28\n      Identifier 22 26\n      List 27 27\n   Identifier 28 34\n   StringKeyword 35 42\n  Constructor 43 98\n   List 60 89\n    Parameter 60 89\n     List -1 -1\n      Decorator 60 72\n       Identifier 66 72\n      PrivateKeyword 72 84\n     Identifier 84 86\n     TypeReference 87 89\n      Identifier 87 89\n   Block 90 98\n    List 92 92\n",
    );
    assert_positions(
        "x = a ? b : c, y = !z, w = (v as T) satisfies U;",
        "x = a ? b : c, y = !z, w = (v as T) satisfies U;",
        "ExpressionStatement 0 48\n BinaryExpression 0 47\n  BinaryExpression 0 21\n   BinaryExpression 0 13\n    Identifier 0 1\n    EqualsToken 1 3\n    ConditionalExpression 3 13\n     Identifier 3 5\n     QuestionToken 5 7\n     Identifier 7 9\n     ColonToken 9 11\n     Identifier 11 13\n   CommaToken 13 14\n   BinaryExpression 14 21\n    Identifier 14 16\n    EqualsToken 16 18\n    PrefixUnaryExpression 18 21\n     Identifier 20 21\n  CommaToken 21 22\n  BinaryExpression 22 47\n   Identifier 22 24\n   EqualsToken 24 26\n   SatisfiesExpression 26 47\n    ParenthesizedExpression 26 35\n     AsExpression 28 34\n      Identifier 28 29\n      TypeReference 32 34\n       Identifier 32 34\n    TypeReference 45 47\n     Identifier 45 47\n",
    );
    assert_positions(
        "label: while (true) break label;",
        "label: while (true) break label;",
        "LabeledStatement 0 32\n Identifier 0 5\n WhileStatement 6 32\n  TrueKeyword 14 18\n  BreakStatement 19 32\n   Identifier 25 31\n",
    );
    assert_positions(
        "type M = { readonly [K in keyof T]?: T[K] };",
        "type M = {\n    readonly [K in keyof T]?: T[K];\n};",
        "TypeAliasDeclaration 0 49\n Identifier 4 6\n MappedType 8 48\n  ReadonlyKeyword 10 23\n  TypeParameter 25 37\n   Identifier 25 26\n   TypeOperator 29 37\n    TypeReference 35 37\n     Identifier 35 37\n  QuestionToken 38 39\n  IndexedAccessType 40 45\n   TypeReference 40 42\n    Identifier 40 42\n   TypeReference 43 44\n    Identifier 43 44\n  List 0 0\n",
    );
    assert_positions(
        "type U<T> = A | B & C<T> | { a: string; b?(): void } | [x: number, ...y: string[]];",
        "type U<T> = A | (B & C<T>) | {\n    a: string;\n    b?(): void;\n} | [\n    x: number,\n    ...y: string[]\n];",
        "TypeAliasDeclaration 0 104\n Identifier 4 6\n List 7 8\n  TypeParameter 7 8\n   Identifier 7 8\n UnionType 11 103\n  List 11 103\n   TypeReference 11 13\n    Identifier 11 13\n   IntersectionType 17 25\n    List 17 25\n     TypeReference 17 18\n      Identifier 17 18\n     TypeReference 20 25\n      Identifier 20 22\n      List 23 24\n       TypeReference 23 24\n        Identifier 23 24\n   TypeLiteral 28 63\n    List 30 61\n     PropertySignature 30 45\n      Identifier 30 36\n      StringKeyword 37 44\n     MethodSignature 45 61\n      Identifier 45 51\n      QuestionToken 51 52\n      List 53 53\n      VoidKeyword 55 60\n   TupleType 65 103\n    List 67 101\n     NamedTupleMember 67 81\n      Identifier 67 73\n      NumberKeyword 74 81\n     NamedTupleMember 82 101\n      DotDotDotToken 82 90\n      Identifier 90 91\n      ArrayType 92 101\n       StringKeyword 92 99\n",
    );
    assert_positions(
        "const j = <div a=\"b\" {...c}>text{d}</div>;",
        "const j = <div a=\"b\" {...c}>text{d}</div>;",
        "VariableStatement 0 42\n VariableDeclarationList 0 41\n  List 5 41\n   VariableDeclaration 5 41\n    Identifier 5 7\n    JsxElement 9 41\n     JsxOpeningElement 9 28\n      Identifier 11 14\n      JsxAttributes 14 27\n       List 14 27\n        JsxAttribute 14 20\n         Identifier 14 16\n         StringLiteral 17 20\n        JsxSpreadAttribute 20 27\n         Identifier 25 26\n     List 28 35\n      JsxText 28 32\n      JsxExpression 32 35\n       Identifier 33 34\n     JsxClosingElement 35 41\n      Identifier 37 40\n",
    );
    assert_positions(
        "let nn = a!.b!;",
        "let nn = (a!).b!;",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 3 16\n   VariableDeclaration 3 16\n    Identifier 3 6\n    NonNullExpression 8 16\n     PropertyAccessExpression 8 15\n      NonNullExpression 10 12\n       Identifier 10 11\n      Identifier 14 15\n",
    );
    assert_positions(
        "const a = [1, 2,];",
        "const a = [1, 2];",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 5 16\n   VariableDeclaration 5 16\n    Identifier 5 7\n    ArrayLiteralExpression 9 16\n     List 11 15\n      NumericLiteral 11 12\n      NumericLiteral 13 15\n",
    );
    assert_positions(
        "const h = <T,>(x: T): T => x;",
        "const h = <T>(x: T): T => x;",
        "VariableStatement 0 28\n VariableDeclarationList 0 27\n  List 5 27\n   VariableDeclaration 5 27\n    Identifier 5 7\n    ArrowFunction 9 27\n     List 11 12\n      TypeParameter 11 12\n       Identifier 11 12\n     List 14 18\n      Parameter 14 18\n       Identifier 14 15\n       TypeReference 16 18\n        Identifier 16 18\n     TypeReference 20 22\n      Identifier 20 22\n     EqualsGreaterThanToken 22 25\n     Identifier 25 27\n",
    );
    assert_positions(
        "let re = /ab+c/gi, big = 10n, s = 'single';",
        "let re = /ab+c/gi, big = 10n, s = 'single';",
        "VariableStatement 0 43\n VariableDeclarationList 0 42\n  List 3 42\n   VariableDeclaration 3 17\n    Identifier 3 6\n    RegularExpressionLiteral 8 17\n   VariableDeclaration 18 28\n    Identifier 18 22\n    BigIntLiteral 24 28\n   VariableDeclaration 29 42\n    Identifier 29 31\n    StringLiteral 33 42\n",
    );
    assert_positions(
        "enum E { A = 1, B }",
        "enum E {\n    A = 1,\n    B\n}",
        "EnumDeclaration 0 27\n Identifier 4 6\n List 8 25\n  EnumMember 8 18\n   Identifier 8 14\n   NumericLiteral 16 18\n  EnumMember 19 25\n   Identifier 19 25\n",
    );
    assert_positions(
        "declare module \"m\" { export const x: number; }",
        "declare module \"m\" {\n    export const x: number;\n}",
        "ModuleDeclaration 0 50\n List -1 -1\n  DeclareKeyword 0 7\n StringLiteral 14 18\n ModuleBlock 18 50\n  List 20 48\n   VariableStatement 20 48\n    List -1 -1\n     ExportKeyword 20 31\n    VariableDeclarationList 31 47\n     List 37 47\n      VariableDeclaration 37 47\n       Identifier 37 39\n       NumberKeyword 40 47\n",
    );
    // tsgo's AssignPositionsToNode lifts the missing `else` into an empty
    // block without a position (liftToBlock of a nil statement), which its
    // formatter then asserts against; tsc-rs keeps the tree as printed.
    assert_positions(
        "function f() { if (a) { b(); } }",
        "function f() { if (a) {\n    b();\n} }",
        "FunctionDeclaration 0 36\n Identifier 8 10\n List 11 11\n Block 12 36\n  List 14 34\n   IfStatement 14 34\n    Identifier 19 20\n    Block 21 34\n     List 23 32\n      ExpressionStatement 23 32\n       CallExpression 23 31\n        Identifier 23 29\n        List 30 30\n",
    );
    assert_positions(
        "const t = `a${b}c${d}`;",
        "const t = `a${b}c${d}`;",
        "VariableStatement 0 23\n VariableDeclarationList 0 22\n  List 5 22\n   VariableDeclaration 5 22\n    Identifier 5 7\n    TemplateExpression 9 22\n     TemplateHead 9 14\n     List 14 22\n      TemplateSpan 14 19\n       Identifier 14 15\n       TemplateMiddle 15 19\n      TemplateSpan 19 22\n       Identifier 19 20\n       TemplateTail 20 22\n",
    );
}

#[test]
fn formatted_for_insertion() {
    // tsgo fails an assertion on the `else` its positioning adds to an `if`
    // without one (see `positions_of_printed_statements`); tsc-rs formats
    // the statement as it formats one with an `else`.
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) { b(); }",
        "{}",
        "    if (a) {\n        b();\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const x = 1;",
        "{}",
        "    const x = 1;",
    );
    assert_insertion(
        "const a = 1;\nconst b = 2;\n",
        0,
        "const y = 42;",
        "{}",
        "const y = 42;",
    );
    assert_insertion(
        "// é\nfunction greet(name: string) {\n    console.log(name);\n}\n",
        36,
        "const x = 1;",
        "{}",
        "    const x = 1;",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f(a, b) { return a + b; }",
        "{}",
        "    function f(a, b) { return a + b; }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "function f(a, b) { return a + b; }",
        "{}",
        "            function f(a, b) { return a + b; }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "class C { m() { return 1; } p = 2; }",
        "{}",
        "    class C {\n        m() { return 1; }\n        p = 2;\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "class C { m() { return 1; } p = 2; }",
        "{}",
        "            class C {\n                m() { return 1; }\n                p = 2;\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) { b(); } else { c(); }",
        "{}",
        "    if (a) {\n        b();\n    } else {\n        c();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "if (a) { b(); } else { c(); }",
        "{}",
        "            if (a) {\n                b();\n            } else {\n                c();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "switch (k) { case 1: a(); break; default: b(); }",
        "{}",
        "    switch (k) {\n        case 1:\n            a();\n            break;\n        default: b();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "switch (k) { case 1: a(); break; default: b(); }",
        "{}",
        "            switch (k) {\n                case 1:\n                    a();\n                    break;\n                default: b();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "{}",
        "    try {\n        a();\n    } catch (e) {\n        b(e);\n    } finally {\n        c();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "{}",
        "            try {\n                a();\n            } catch (e) {\n                b(e);\n            } finally {\n                c();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1, b: { c: [1, 2] }, m() { return this.a; } };",
        "{}",
        "    const o = { a: 1, b: { c: [1, 2] }, m() { return this.a; } };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "interface I { a: string; m(x: number): void; }",
        "{}",
        "    interface I {\n        a: string;\n        m(x: number): void;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "type T = { a: string; b: { c: number } };",
        "{}",
        "    type T = {\n        a: string;\n        b: {\n            c: number;\n        };\n    };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "enum E { A = 1, B, C }",
        "{}",
        "    enum E {\n        A = 1,\n        B,\n        C\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "import { a, b } from \"m\";",
        "{}",
        "    import { a, b } from \"m\";",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "export const z = () => { return 1; };",
        "{}",
        "    export const z = () => { return 1; };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "namespace N { export const x = 1; }",
        "{}",
        "    namespace N {\n        export const x = 1;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "while (a) { do { b(); } while (c); }",
        "{}",
        "    while (a) {\n        do {\n            b();\n        } while (c);\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "f(a, function () { return 1; }, () => { x(); });",
        "{}",
        "    f(a, function() { return 1; }, () => { x(); });",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const j = [1, [2, 3], { a: 4 }];",
        "{}",
        "    const j = [1, [2, 3], { a: 4 }];",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "let s = `a${b}c`;",
        "{}",
        "    let s = `a${b}c`;",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "label: for (;;) { break label; }",
        "{}",
        "    label: for (; ;) {\n        break label;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) b(); else c();",
        "{}",
        "    if (a) b(); else c();",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "    abstract class K<T> extends B<T> implements I {\n        abstract m(): void;\n        constructor(private x: T) { super(); }\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "            abstract class K<T> extends B<T> implements I {\n                abstract m(): void;\n                constructor(private x: T) { super(); }\n            }",
    );
    assert_insertion(
        "",
        0,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "abstract class K<T> extends B<T> implements I {\n    abstract m(): void;\n    constructor(private x: T) { super(); }\n}",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "x = a ? b : c;",
        "{}",
        "    x = a ? b : c;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "x = a ? b : c;",
        "{}",
        "            x = a ? b : c;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        49,
        "const q = 1;",
        "{}",
        "            const q = 1;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        14,
        "n() { return 2; }",
        "{}",
        "    n();",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        72,
        "class D { }",
        "{}",
        "class D {\n}",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        30,
        "const after = { a: 1 };",
        "{}",
        "const after = { a: 1 };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { return { a: 1 }; }",
        "{\"placeOpenBraceOnNewLineForFunctions\": true}",
        "    function f() { return { a: 1 }; }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1 };",
        "{\"insertSpaceAfterOpeningAndBeforeClosingNonemptyBraces\": false}",
        "    const o = {a: 1};",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1 };",
        "{\"newLineCharacter\": \"\\r\\n\"}",
        "    const o = { a: 1 };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"newLineCharacter\": \"\\r\\n\"}",
        "    function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"indentSize\": 0}",
        "function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"baseIndentSize\": 3}",
        "    function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"indentStyle\": \"none\"}",
        "function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"semicolons\": \"remove\"}",
        "    function f() { a() }",
    );
}

/// The indentation of a statement inserted at each position of a file
/// (`GetIndentation` through `formatNodeForInsertion`): comments, string
/// and template literals, list items after a comma or before the first,
/// object literals, the smart indenter's parents, the block and no indent
/// styles. One width per position, from tsgo.
#[test]
fn indentation_of_an_insertion_at_each_position() {
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "/* block\n * comment\n */\nconst o = {\n    a: 1,\n    b: foo(x,\n        y),\n    c: `t ${x}\n more`,\n    \"s\": 'q',\n};\n// line comment\nswitch (k) {\n    case 1:\n        f();\n    default:\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 0, 0, 4, 4, 4, 0, 0, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 8, 0, 0],
    );
    assert_indentation_at_each_position(
        "class C<T> extends B {\n    private x = 1;\n    m(a: number,\n      b: string): void {\n        const f = (v) =>\n            v + 1;\n        call(a, () => {\n            other();\n        });\n    }\n}\ninterface I {\n    a: string;\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 8, 8, 8, 8, 12, 12, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 16, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 12, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "namespace N {\n    export enum E {\n        A,\n        B,\n    }\n}\nlet x = cond\n    ? a\n    : b;\nfor (let i = 0;\n    i < n; i++) {\n}\ntype T = {\n    k: number\n};\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{\"indentStyle\": \"block\"}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0, 0],
    );
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{\"indentStyle\": \"none\"}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
}

#[track_caller]
fn assert_indentation_at_each_position(target: &str, settings: &str, expected: &[usize]) {
    let source = parse_source_file(
        "/t.ts",
        target.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    let preferences = UserPreferences::from_json(&serde_json::from_str(settings).unwrap()).unwrap();
    let options = &preferences.format_code_settings;
    let positioned = print_decoded(
        "x;",
        ScriptKind::Ts,
        (NewLineKind::LineFeed, 4),
        source.language_variant,
    );
    let widths: Vec<usize> = (0..=target.len())
        .map(|position| {
            let formatted =
                tsc_ls::format_node_for_insertion(&source, position, &positioned, options);
            formatted.len() - formatted.trim_start_matches(' ').len()
        })
        .collect();
    assert_eq!(widths, expected, "{target}");
}

/// Statements printed as nodes of their file (tsgo `PrintAndPositionNode`
/// with the source file, as the change tracker prints them): the text with
/// preserveSourceNewlines over the source lines (list line breaks, the
/// closing lines of a list, parenthesized expressions, statements and
/// members of blocks, classes, interfaces, enums and case blocks, comments)
/// and every node's and list's position, from tsgo.
#[test]
fn source_nodes_printed_with_their_file() {
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        0,
        "import { a,\n    b,\n} from \"m\";",
        "ImportDeclaration 0 30\n ImportClause 6 20\n  NamedImports 6 20\n   List 8 18\n    ImportSpecifier 8 10\n     Identifier 8 10\n    ImportSpecifier 11 17\n     Identifier 11 17\n StringLiteral 25 29\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        1,
        "import { c, // cc\n    d } from \"m\";",
        "ImportDeclaration 0 35\n ImportClause 6 25\n  NamedImports 6 25\n   List 8 23\n    ImportSpecifier 8 10\n     Identifier 8 10\n    ImportSpecifier 11 23\n     Identifier 11 23\n StringLiteral 30 34\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        2,
        "import type { T\n} from \"t\";",
        "ImportDeclaration 0 27\n ImportClause 6 17\n  NamedImports 11 17\n   List 13 15\n    ImportSpecifier 13 15\n     Identifier 13 15\n StringLiteral 22 26\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        3,
        "import def, { x as y\n} from \"z\";",
        "ImportDeclaration 0 32\n ImportClause 6 22\n  Identifier 6 10\n  NamedImports 11 22\n   List 13 20\n    ImportSpecifier 13 20\n     Identifier 13 15\n     Identifier 18 20\n StringLiteral 27 31\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        4,
        "import * as ns from \"ns\";",
        "ImportDeclaration 0 25\n ImportClause 6 14\n  NamespaceImport 6 14\n   Identifier 11 14\n StringLiteral 19 24\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        5,
        "import \"side\";",
        "ImportDeclaration 0 14\n StringLiteral 6 13\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        6,
        "import data from \"./d.json\" with { type: \"json\"\n};",
        "ImportDeclaration 0 50\n ImportClause 6 11\n  Identifier 6 11\n StringLiteral 16 27\n ImportAttributes 27 49\n  List 34 47\n   ImportAttribute 34 47\n    Identifier 34 39\n    StringLiteral 40 47\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        7,
        "export * from \"all\";",
        "ExportDeclaration 0 20\n StringLiteral 13 19\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        8,
        "export * as allNs from \"all\";",
        "ExportDeclaration 0 29\n NamespaceExport 6 17\n  Identifier 11 17\n StringLiteral 22 28\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        9,
        "export { a, b as bb,\n    c\n};",
        "ExportDeclaration 0 29\n NamedExports 6 28\n  List 8 26\n   ExportSpecifier 8 10\n    Identifier 8 10\n   ExportSpecifier 11 19\n    Identifier 11 13\n    Identifier 16 19\n   ExportSpecifier 20 26\n    Identifier 20 26\n",
    );
    assert_source_statement(
        "import {\n    a,\n    b,\n} from \"m\";\nimport { c, // cc\n    d } from \"m\";\nimport type {\n    T\n} from \"t\";\nimport def, {\n    x as y\n} from \"z\";\nimport * as ns from \"ns\";\nimport \"side\";\nimport data from \"./d.json\" with {\n    type: \"json\"\n};\nexport * from \"all\";\nexport * as allNs from \"all\";\nexport {\n    a, b as bb,\n    c\n};\nexport default def;\n",
        10,
        "export default def;",
        "ExportAssignment 0 19\n Identifier 14 18\n",
    );
    assert_source_statement(
        "function f1(\n    a: number,\n    b: string,\n): void {}\nfunction f2<\n    T,\n    U extends T\n>(t: T, u: U) {}\nconst f3 = (\n    x,\n    y\n) => x + y;\nconst f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};\nfunction f5(a, /* c1 */ b /* c2 */, c) {}\n",
        0,
        "function f1(a: number,\n    b: string\n): void { }",
        "FunctionDeclaration 0 48\n Identifier 8 11\n List 12 36\n  Parameter 12 21\n   Identifier 12 13\n   NumberKeyword 14 21\n  Parameter 22 36\n   Identifier 22 28\n   StringKeyword 29 36\n VoidKeyword 39 44\n Block 44 48\n  List 46 46\n",
    );
    assert_source_statement(
        "function f1(\n    a: number,\n    b: string,\n): void {}\nfunction f2<\n    T,\n    U extends T\n>(t: T, u: U) {}\nconst f3 = (\n    x,\n    y\n) => x + y;\nconst f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};\nfunction f5(a, /* c1 */ b /* c2 */, c) {}\n",
        1,
        "function f2<T,\n    U extends T\n>(t: T, u: U) { }",
        "FunctionDeclaration 0 48\n Identifier 8 11\n List 12 30\n  TypeParameter 12 13\n   Identifier 12 13\n  TypeParameter 14 30\n   Identifier 14 20\n   TypeReference 28 30\n    Identifier 28 30\n List 33 43\n  Parameter 33 37\n   Identifier 33 34\n   TypeReference 35 37\n    Identifier 35 37\n  Parameter 38 43\n   Identifier 38 40\n   TypeReference 41 43\n    Identifier 41 43\n Block 44 48\n  List 46 46\n",
    );
    assert_source_statement(
        "function f1(\n    a: number,\n    b: string,\n): void {}\nfunction f2<\n    T,\n    U extends T\n>(t: T, u: U) {}\nconst f3 = (\n    x,\n    y\n) => x + y;\nconst f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};\nfunction f5(a, /* c1 */ b /* c2 */, c) {}\n",
        2,
        "const f3 = (x,\n    y\n) => x + y;",
        "VariableStatement 0 32\n VariableDeclarationList 0 31\n  List 5 31\n   VariableDeclaration 5 31\n    Identifier 5 8\n    ArrowFunction 10 31\n     List 12 20\n      Parameter 12 13\n       Identifier 12 13\n      Parameter 14 20\n       Identifier 14 20\n     EqualsGreaterThanToken 22 25\n     BinaryExpression 25 31\n      Identifier 25 27\n      PlusToken 27 29\n      Identifier 29 31\n",
    );
    assert_source_statement(
        "function f1(\n    a: number,\n    b: string,\n): void {}\nfunction f2<\n    T,\n    U extends T\n>(t: T, u: U) {}\nconst f3 = (\n    x,\n    y\n) => x + y;\nconst f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};\nfunction f5(a, /* c1 */ b /* c2 */, c) {}\n",
        3,
        "const f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};",
        "VariableStatement 0 79\n VariableDeclarationList 0 78\n  List 5 78\n   VariableDeclaration 5 78\n    Identifier 5 8\n    ArrowFunction 10 78\n     List -1 -1\n      AsyncKeyword 10 16\n     List 18 57\n      Parameter 18 23\n       Identifier 18 19\n       NumericLiteral 21 23\n      Parameter 24 41\n       ObjectBindingPattern 24 33\n        List 26 31\n         BindingElement 26 28\n          Identifier 26 28\n         BindingElement 29 31\n          Identifier 29 31\n       TypeReference 34 36\n        Identifier 34 36\n       ObjectLiteralExpression 38 41\n        List 40 40\n      Parameter 42 57\n       ArrayBindingPattern 42 49\n        List 44 48\n         BindingElement 44 45\n          Identifier 44 45\n         BindingElement 46 48\n          Identifier 46 48\n       TypeReference 50 52\n        Identifier 50 52\n       ArrayLiteralExpression 54 57\n        List 56 56\n     EqualsGreaterThanToken 58 61\n     Block 61 78\n      List 63 76\n       ExpressionStatement 63 76\n        AwaitExpression 63 75\n         Identifier 73 75\n",
    );
    assert_source_statement(
        "function f1(\n    a: number,\n    b: string,\n): void {}\nfunction f2<\n    T,\n    U extends T\n>(t: T, u: U) {}\nconst f3 = (\n    x,\n    y\n) => x + y;\nconst f4 = async (a = 1, { b, c }: O = {}, [d, e]: A = []) => {\n    await a;\n};\nfunction f5(a, /* c1 */ b /* c2 */, c) {}\n",
        4,
        "function f5(a, /* c1 */ b /* c2 */, c) { }",
        "FunctionDeclaration 0 42\n Identifier 8 11\n List 12 37\n  Parameter 12 13\n   Identifier 12 13\n  Parameter 14 25\n   Identifier 14 25\n  Parameter 35 37\n   Identifier 35 37\n Block 38 42\n  List 40 40\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        0,
        "foo(1,\n    2\n);",
        "ExpressionStatement 0 15\n CallExpression 0 14\n  Identifier 0 3\n  List 4 12\n   NumericLiteral 4 5\n   NumericLiteral 6 12\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        1,
        "foo(1,\n    2, 3);",
        "ExpressionStatement 0 17\n CallExpression 0 16\n  Identifier 0 3\n  List 4 15\n   NumericLiteral 4 5\n   NumericLiteral 6 12\n   NumericLiteral 13 15\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        2,
        "obj.a()\n    .b()\n    .c(x => {\n    return x;\n});",
        "ExpressionStatement 0 48\n CallExpression 0 47\n  PropertyAccessExpression 0 23\n   CallExpression 0 16\n    PropertyAccessExpression 0 14\n     CallExpression 0 7\n      PropertyAccessExpression 0 5\n       Identifier 0 3\n       Identifier 4 5\n      List 6 6\n     Identifier 13 14\n    List 15 15\n   Identifier 22 23\n  List 24 46\n   ArrowFunction 24 46\n    List 24 25\n     Parameter 24 25\n      Identifier 24 25\n    EqualsGreaterThanToken 25 28\n    Block 28 46\n     List 30 44\n      ReturnStatement 30 44\n       Identifier 41 43\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        3,
        "new Klass(arg\n);",
        "ExpressionStatement 0 16\n NewExpression 0 15\n  Identifier 3 9\n  List 10 13\n   Identifier 10 13\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        4,
        "const t = tag `a${b}c`;",
        "VariableStatement 0 23\n VariableDeclarationList 0 22\n  List 5 22\n   VariableDeclaration 5 22\n    Identifier 5 7\n    TaggedTemplateExpression 9 22\n     Identifier 9 13\n     TemplateExpression 13 22\n      TemplateHead 13 18\n      List 18 22\n       TemplateSpan 18 22\n        Identifier 18 19\n        TemplateTail 19 22\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        5,
        "const s = `x${y}z`;",
        "VariableStatement 0 19\n VariableDeclarationList 0 18\n  List 5 18\n   VariableDeclaration 5 18\n    Identifier 5 7\n    TemplateExpression 9 18\n     TemplateHead 9 14\n     List 14 18\n      TemplateSpan 14 18\n       Identifier 14 15\n       TemplateTail 15 18\n",
    );
    assert_source_statement(
        "foo(\n    1,\n    2\n);\nfoo(1,\n    2, 3);\nobj.a()\n    .b()\n    .c(x => {\n        return x;\n    });\nnew Klass(\n    arg\n);\nconst t = tag`a${\n    b\n}c`;\nconst s = `x${y}z`;\nfoo(function () {\n    bar();\n}, 2);\n",
        6,
        "foo(function () {\n    bar();\n}, 2);",
        "ExpressionStatement 0 35\n CallExpression 0 34\n  Identifier 0 3\n  List 4 33\n   FunctionExpression 4 30\n    List 14 14\n    Block 15 30\n     List 17 28\n      ExpressionStatement 17 28\n       CallExpression 17 27\n        Identifier 17 25\n        List 26 26\n   NumericLiteral 31 33\n",
    );
    assert_source_statement(
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};\nconst arr = [1, 2,\n    3];\nconst nested = { a: { b: { c: 1 } } };\nconst empty = {\n};\nconst emptyArr = [\n];\n",
        0,
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};",
        "VariableStatement 0 161\n VariableDeclarationList 0 160\n  List 5 160\n   VariableDeclaration 5 160\n    Identifier 5 7\n    ObjectLiteralExpression 9 160\n     List 11 158\n      PropertyAssignment 11 20\n       Identifier 11 17\n       NumericLiteral 18 20\n      PropertyAssignment 21 73\n       Identifier 21 42\n       ArrayLiteralExpression 43 73\n        List 45 67\n         NumericLiteral 45 55\n         NumericLiteral 56 66\n      MethodDeclaration 74 108\n       Identifier 74 80\n       List 81 81\n       Block 82 108\n        List 84 102\n         ReturnStatement 84 102\n          NumericLiteral 99 101\n      GetAccessor 109 135\n       Identifier 117 119\n       List 120 120\n       Block 121 135\n        List 123 133\n         ReturnStatement 123 133\n          NumericLiteral 130 132\n      PropertyAssignment 136 147\n       ComputedPropertyName 136 144\n        Identifier 142 143\n       NumericLiteral 145 147\n      SpreadAssignment 148 157\n       Identifier 156 157\n",
    );
    assert_source_statement(
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};\nconst arr = [1, 2,\n    3];\nconst nested = { a: { b: { c: 1 } } };\nconst empty = {\n};\nconst emptyArr = [\n];\n",
        1,
        "const arr = [1, 2,\n    3];",
        "VariableStatement 0 26\n VariableDeclarationList 0 25\n  List 5 25\n   VariableDeclaration 5 25\n    Identifier 5 9\n    ArrayLiteralExpression 11 25\n     List 13 24\n      NumericLiteral 13 14\n      NumericLiteral 15 17\n      NumericLiteral 18 24\n",
    );
    assert_source_statement(
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};\nconst arr = [1, 2,\n    3];\nconst nested = { a: { b: { c: 1 } } };\nconst empty = {\n};\nconst emptyArr = [\n];\n",
        2,
        "const nested = { a: { b: { c: 1 } } };",
        "VariableStatement 0 38\n VariableDeclarationList 0 37\n  List 5 37\n   VariableDeclaration 5 37\n    Identifier 5 12\n    ObjectLiteralExpression 14 37\n     List 16 35\n      PropertyAssignment 16 35\n       Identifier 16 18\n       ObjectLiteralExpression 19 35\n        List 21 33\n         PropertyAssignment 21 33\n          Identifier 21 23\n          ObjectLiteralExpression 24 33\n           List 26 31\n            PropertyAssignment 26 31\n             Identifier 26 28\n             NumericLiteral 29 31\n",
    );
    assert_source_statement(
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};\nconst arr = [1, 2,\n    3];\nconst nested = { a: { b: { c: 1 } } };\nconst empty = {\n};\nconst emptyArr = [\n];\n",
        3,
        "const empty = {};",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 5 16\n   VariableDeclaration 5 16\n    Identifier 5 11\n    ObjectLiteralExpression 13 16\n     List 15 15\n",
    );
    assert_source_statement(
        "const o = {\n    a: 1,\n    // comment\n    b: [\n        1,\n        2,\n    ],\n    c() {\n        return 1;\n    },\n    get d() { return 2; },\n    [e]: 3,\n    ...f,\n};\nconst arr = [1, 2,\n    3];\nconst nested = { a: { b: { c: 1 } } };\nconst empty = {\n};\nconst emptyArr = [\n];\n",
        4,
        "const emptyArr = [];",
        "VariableStatement 0 20\n VariableDeclarationList 0 19\n  List 5 19\n   VariableDeclaration 5 19\n    Identifier 5 14\n    ArrayLiteralExpression 16 19\n     List 18 18\n",
    );
    assert_source_statement(
        "class C extends B\n    implements I, J {\n    // first\n    private x = 1;\n\n    @dec()\n    method(\n        a: number\n    ) { }\n    constructor(\n        private readonly a: string,\n        public b?: number\n    ) {\n        super();\n    }\n    [key: string]: any;\n    static {\n        init();\n    }\n}\n",
        0,
        "class C extends B\n     implements I, J {\n    // first\n    private x = 1;\n    @dec()\n    method(a: number\n    ) { }\n    constructor(private readonly a: string,\n        public b?: number\n    ) {\n        super();\n    }\n    [key: string]: any;\n    static {\n        init();\n    }\n}",
        "ClassDeclaration 0 276\n Identifier 5 7\n List 7 38\n  HeritageClause 7 17\n   List 15 17\n    ExpressionWithTypeArguments 15 17\n     Identifier 15 17\n  HeritageClause 17 38\n   List 33 38\n    TypeReference 33 35\n     Identifier 33 35\n    TypeReference 36 38\n     Identifier 36 38\n List 40 274\n  PropertyDeclaration 40 72\n   List -1 -1\n    PrivateKeyword 40 65\n   Identifier 65 67\n   NumericLiteral 69 71\n  MethodDeclaration 72 114\n   List -1 -1\n    Decorator 72 83\n     CallExpression 78 83\n      Identifier 78 81\n      List 82 82\n   Identifier 83 94\n   List 95 104\n    Parameter 95 104\n     Identifier 95 96\n     NumberKeyword 97 104\n   Block 110 114\n    List 112 112\n  Constructor 114 215\n   List 131 184\n    Parameter 131 157\n     List -1 -1\n      PrivateKeyword 131 138\n      ReadonlyKeyword 138 147\n     Identifier 147 149\n     StringKeyword 150 157\n    Parameter 158 184\n     List -1 -1\n      PublicKeyword 158 173\n     Identifier 173 175\n     QuestionToken 175 176\n     NumberKeyword 177 184\n   Block 190 215\n    List 192 209\n     ExpressionStatement 192 209\n      CallExpression 192 208\n       SuperKeyword 192 206\n       List 207 207\n  IndexSignature 215 239\n   List 221 232\n    Parameter 221 232\n     Identifier 221 224\n     StringKeyword 225 232\n   AnyKeyword 234 238\n  ClassStaticBlockDeclaration 239 274\n   Block 250 274\n    List 252 268\n     ExpressionStatement 252 268\n      CallExpression 252 267\n       Identifier 252 265\n       List 266 266\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        0,
        "interface I {\n    a: string;\n    b(x: number\n    ): void;\n}",
        "InterfaceDeclaration 0 59\n Identifier 9 11\n List 13 57\n  PropertySignature 13 28\n   Identifier 13 19\n   StringKeyword 20 27\n  MethodSignature 28 57\n   Identifier 28 34\n   List 35 44\n    Parameter 35 44\n     Identifier 35 36\n     NumberKeyword 37 44\n   VoidKeyword 51 56\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        1,
        "type U = A |\n    B |\n    C;",
        "TypeAliasDeclaration 0 27\n Identifier 4 6\n UnionType 8 26\n  List 8 26\n   TypeReference 8 10\n    Identifier 8 10\n   TypeReference 12 18\n    Identifier 12 18\n   TypeReference 20 26\n    Identifier 20 26\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        2,
        "type X = A &\n    B;",
        "TypeAliasDeclaration 0 19\n Identifier 4 6\n IntersectionType 8 18\n  List 8 18\n   TypeReference 8 10\n    Identifier 8 10\n   TypeReference 12 18\n    Identifier 12 18\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        3,
        "type Tup = [\n    a: string,\n    b?: number\n];",
        "TypeAliasDeclaration 0 45\n Identifier 4 8\n TupleType 10 44\n  List 12 42\n   NamedTupleMember 12 26\n    Identifier 12 18\n    StringKeyword 19 26\n   NamedTupleMember 27 42\n    Identifier 27 33\n    QuestionToken 33 34\n    NumberKeyword 35 42\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        4,
        "type M = {\n    [K in keyof T]: T[K];\n};",
        "TypeAliasDeclaration 0 39\n Identifier 4 6\n MappedType 8 38\n  TypeParameter 16 28\n   Identifier 16 17\n   TypeOperator 20 28\n    TypeReference 26 28\n     Identifier 26 28\n  IndexedAccessType 30 35\n   TypeReference 30 32\n    Identifier 30 32\n   TypeReference 33 34\n    Identifier 33 34\n  List 0 0\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        5,
        "type Cond<T> = T extends string ? 1 : 2;",
        "TypeAliasDeclaration 0 40\n Identifier 4 9\n List 10 11\n  TypeParameter 10 11\n   Identifier 10 11\n ConditionalType 14 39\n  TypeReference 14 16\n   Identifier 14 16\n  StringKeyword 24 31\n  LiteralType 33 35\n   NumericLiteral 33 35\n  LiteralType 37 39\n   NumericLiteral 37 39\n",
    );
    assert_source_statement(
        "interface I {\n    a: string;\n    b(\n        x: number\n    ): void;\n}\ntype U =\n    | A\n    | B\n    | C;\ntype X = A &\n    B;\ntype Tup = [\n    a: string,\n    b?: number\n];\ntype M = {\n    [K in keyof T]: T[K]\n};\ntype Cond<T> = T extends string\n    ? 1\n    : 2;\ntype Fn = (\n    a: number\n) => void;\n",
        6,
        "type Fn = (a: number\n) => void;",
        "TypeAliasDeclaration 0 31\n Identifier 4 7\n FunctionType 9 30\n  List 11 20\n   Parameter 11 20\n    Identifier 11 12\n    NumberKeyword 13 20\n  VoidKeyword 25 30\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        0,
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}",
        "IfStatement 0 62\n Identifier 4 5\n Block 6 19\n  List 8 17\n   ExpressionStatement 8 17\n    CallExpression 8 16\n     Identifier 8 14\n     List 15 15\n IfStatement 24 62\n  Identifier 29 30\n  Block 31 44\n   List 33 42\n    ExpressionStatement 33 42\n     CallExpression 33 41\n      Identifier 33 39\n      List 40 40\n  Block 49 62\n   List 51 60\n    ExpressionStatement 51 60\n     CallExpression 51 59\n      Identifier 51 57\n      List 58 58\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        1,
        "try {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}",
        "TryStatement 0 60\n Block 3 16\n  List 5 14\n   ExpressionStatement 5 14\n    CallExpression 5 13\n     Identifier 5 11\n     List 12 12\n CatchClause 16 39\n  VariableDeclaration 24 25\n   Identifier 24 25\n  Block 26 39\n   List 28 37\n    ExpressionStatement 28 37\n     CallExpression 28 36\n      Identifier 28 34\n      List 35 35\n Block 47 60\n  List 49 58\n   ExpressionStatement 49 58\n    CallExpression 49 57\n     Identifier 49 55\n     List 56 56\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        2,
        "switch (x) {\n    case 1:\n        a();\n        break;\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}",
        "SwitchStatement 0 113\n Identifier 8 9\n CaseBlock 10 113\n  List 12 111\n   CaseClause 12 52\n    NumericLiteral 21 23\n    List 24 52\n     ExpressionStatement 24 37\n      CallExpression 24 36\n       Identifier 24 34\n       List 35 35\n     BreakStatement 37 52\n   CaseClause 52 85\n    NumericLiteral 61 63\n    List 64 85\n     Block 64 85\n      List 66 79\n       ExpressionStatement 66 79\n        CallExpression 66 78\n         Identifier 66 76\n         List 77 77\n   DefaultClause 85 111\n    List 98 111\n     ExpressionStatement 98 111\n      CallExpression 98 110\n       Identifier 98 108\n       List 109 109\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        3,
        "for (const x of xs) f(x);",
        "ForOfStatement 0 25\n VariableDeclarationList 5 12\n  List 10 12\n   VariableDeclaration 10 12\n    Identifier 10 12\n Identifier 15 18\n ExpressionStatement 19 25\n  CallExpression 19 24\n   Identifier 19 21\n   List 22 23\n    Identifier 22 23\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        4,
        "while (a) b();",
        "WhileStatement 0 14\n Identifier 7 8\n ExpressionStatement 9 14\n  CallExpression 9 13\n   Identifier 9 11\n   List 12 12\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        5,
        "do a();\nwhile (b);",
        "DoStatement 0 18\n ExpressionStatement 2 7\n  CallExpression 2 6\n   Identifier 2 4\n   List 5 5\n Identifier 15 16\n",
    );
    assert_source_statement(
        "if (a) {\n    b();\n}\nelse if (c) {\n    d();\n}\nelse {\n    e();\n}\ntry {\n    a();\n}\ncatch (e) {\n    b();\n}\nfinally {\n    c();\n}\nswitch (x) {\n    case 1:\n        a();\n        break;\n\n    case 2: {\n        b();\n    }\n    default:\n        c();\n}\nfor (const x of xs)\n    f(x);\nwhile (a)\n    b();\ndo\n    a();\nwhile (b);\nlabel:\n    for (;;) break label;\n",
        6,
        "label: for (;;) break label;",
        "LabeledStatement 0 28\n Identifier 0 5\n ForStatement 6 28\n  BreakStatement 15 28\n   Identifier 21 27\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        0,
        "let a = 1, b = 2;",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 3 16\n   VariableDeclaration 3 9\n    Identifier 3 5\n    NumericLiteral 7 9\n   VariableDeclaration 10 16\n    Identifier 10 12\n    NumericLiteral 14 16\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        1,
        "const c = x\n    ? y\n    : z;",
        "VariableStatement 0 28\n VariableDeclarationList 0 27\n  List 5 27\n   VariableDeclaration 5 27\n    Identifier 5 7\n    ConditionalExpression 9 27\n     Identifier 9 11\n     QuestionToken 11 17\n     Identifier 17 19\n     ColonToken 19 25\n     Identifier 25 27\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        2,
        "const d = a +\n    b -\n    c;",
        "VariableStatement 0 28\n VariableDeclarationList 0 27\n  List 5 27\n   VariableDeclaration 5 27\n    Identifier 5 7\n    BinaryExpression 9 27\n     BinaryExpression 9 19\n      Identifier 9 11\n      PlusToken 11 13\n      Identifier 13 19\n     MinusToken 19 21\n     Identifier 21 27\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        3,
        "returnValue(a && b ||\n    c\n);",
        "ExpressionStatement 0 30\n CallExpression 0 29\n  Identifier 0 11\n  List 12 27\n   BinaryExpression 12 27\n    BinaryExpression 12 18\n     Identifier 12 13\n     AmpersandAmpersandToken 13 16\n     Identifier 16 18\n    BarBarToken 18 21\n    Identifier 21 27\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        4,
        "enum E {\n    A = 1,\n    B\n}",
        "EnumDeclaration 0 27\n Identifier 4 6\n List 8 25\n  EnumMember 8 18\n   Identifier 8 14\n   NumericLiteral 16 18\n  EnumMember 19 25\n   Identifier 19 25\n",
    );
    assert_source_statement(
        "let a = 1,\n    b = 2;\nconst c = x\n    ? y\n    : z;\nconst d = a +\n    b -\n    c;\nreturnValue(\n    a && b ||\n        c\n);\nenum E {\n    A = 1,\n    B,\n}\nnamespace N.M {\n    export const x = 1;\n}\ndeclare global {\n    interface Window {\n        a: string;\n    }\n}\n",
        6,
        "declare global {\n    interface Window {\n        a: string;\n    }\n}",
        "ModuleDeclaration 0 66\n List -1 -1\n  DeclareKeyword 0 7\n Identifier 7 14\n ModuleBlock 14 66\n  List 16 64\n   InterfaceDeclaration 16 64\n    Identifier 30 37\n    List 39 58\n     PropertySignature 39 58\n      Identifier 39 49\n      StringKeyword 50 57\n",
    );
    assert_source_statement(
        "/** doc */\nexport function documented(\n    /** param doc */ a: number\n): void {\n    // inside\n    a;\n    /* block */\n}\n// trailing comment\n",
        0,
        "/** doc */\nexport function documented(\n/** param doc */ a: number\n): void {\n    // inside\n    a;\n    /* block */\n}",
        "FunctionDeclaration 0 114\n List -1 -1\n  ExportKeyword 0 17\n Identifier 26 37\n List 38 65\n  Parameter 38 65\n   Identifier 38 57\n   NumberKeyword 58 65\n VoidKeyword 68 73\n Block 73 114\n  List 75 96\n   ExpressionStatement 75 96\n    Identifier 75 95\n",
    );
    assert_source_statement(
        "const x = {\n    a: 1, b: 2,\n    c: 3\n}, y = [\n    1, 2\n];\nfunction g() {\n    return {\n        a,\n        b\n    };\n}\n",
        0,
        "const x = {\n    a: 1, b: 2,\n    c: 3\n}, y = [\n    1, 2\n];",
        "VariableStatement 0 57\n VariableDeclarationList 0 56\n  List 5 56\n   VariableDeclaration 5 38\n    Identifier 5 7\n    ObjectLiteralExpression 9 38\n     List 11 36\n      PropertyAssignment 11 20\n       Identifier 11 17\n       NumericLiteral 18 20\n      PropertyAssignment 21 26\n       Identifier 21 23\n       NumericLiteral 24 26\n      PropertyAssignment 27 36\n       Identifier 27 33\n       NumericLiteral 34 36\n   VariableDeclaration 39 56\n    Identifier 39 41\n    ArrayLiteralExpression 43 56\n     List 45 54\n      NumericLiteral 45 51\n      NumericLiteral 52 54\n",
    );
    assert_source_statement(
        "const x = {\n    a: 1, b: 2,\n    c: 3\n}, y = [\n    1, 2\n];\nfunction g() {\n    return {\n        a,\n        b\n    };\n}\n",
        1,
        "function g() {\n    return {\n        a,\n        b\n    };\n}",
        "FunctionDeclaration 0 57\n Identifier 8 10\n List 11 11\n Block 12 57\n  List 14 55\n   ReturnStatement 14 55\n    ObjectLiteralExpression 25 54\n     List 27 48\n      ShorthandPropertyAssignment 27 37\n       Identifier 27 37\n      ShorthandPropertyAssignment 38 48\n       Identifier 38 48\n",
    );
    assert_source_statement(
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];\nconst w = obj\n    .prop\n    .method();\nconst p = (\n    a +\n    b\n);\nconst q = a\n\n    + b;\nlet r = x ||\n\n    y;\n",
        0,
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];",
        "VariableStatement 0 66\n VariableDeclarationList 0 65\n  List 5 65\n   VariableDeclaration 5 65\n    Identifier 5 7\n    ConditionalExpression 9 65\n     Identifier 9 14\n     QuestionToken 14 20\n     ObjectLiteralExpression 20 41\n      List 22 35\n       PropertyAssignment 22 35\n        Identifier 22 32\n        NumericLiteral 33 35\n     ColonToken 41 47\n     ArrayLiteralExpression 47 65\n      List 49 59\n       NumericLiteral 49 59\n",
    );
    assert_source_statement(
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];\nconst w = obj\n    .prop\n    .method();\nconst p = (\n    a +\n    b\n);\nconst q = a\n\n    + b;\nlet r = x ||\n\n    y;\n",
        1,
        "const w = obj\n    .prop\n    .method();",
        "VariableStatement 0 38\n VariableDeclarationList 0 37\n  List 5 37\n   VariableDeclaration 5 37\n    Identifier 5 7\n    CallExpression 9 37\n     PropertyAccessExpression 9 35\n      PropertyAccessExpression 9 23\n       Identifier 9 13\n       Identifier 19 23\n      Identifier 29 35\n     List 36 36\n",
    );
    assert_source_statement(
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];\nconst w = obj\n    .prop\n    .method();\nconst p = (\n    a +\n    b\n);\nconst q = a\n\n    + b;\nlet r = x ||\n\n    y;\n",
        2,
        "const p = (a +\n    b\n);",
        "VariableStatement 0 23\n VariableDeclarationList 0 22\n  List 5 22\n   VariableDeclaration 5 22\n    Identifier 5 7\n    ParenthesizedExpression 9 22\n     BinaryExpression 11 20\n      Identifier 11 12\n      PlusToken 12 14\n      Identifier 14 20\n",
    );
    assert_source_statement(
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];\nconst w = obj\n    .prop\n    .method();\nconst p = (\n    a +\n    b\n);\nconst q = a\n\n    + b;\nlet r = x ||\n\n    y;\n",
        3,
        "const q = a\n    + b;",
        "VariableStatement 0 20\n VariableDeclarationList 0 19\n  List 5 19\n   VariableDeclaration 5 19\n    Identifier 5 7\n    BinaryExpression 9 19\n     Identifier 9 11\n     PlusToken 11 17\n     Identifier 17 19\n",
    );
    assert_source_statement(
        "const v = cond\n    ? {\n        a: 1\n    }\n    : [\n        2\n    ];\nconst w = obj\n    .prop\n    .method();\nconst p = (\n    a +\n    b\n);\nconst q = a\n\n    + b;\nlet r = x ||\n\n    y;\n",
        4,
        "let r = x ||\n    y;",
        "VariableStatement 0 19\n VariableDeclarationList 0 18\n  List 3 18\n   VariableDeclaration 3 18\n    Identifier 3 5\n    BinaryExpression 7 18\n     Identifier 7 9\n     BarBarToken 9 12\n     Identifier 12 18\n",
    );
    assert_source_statement(
        "if (a)\n{\n    b();\n}\n\n\nif (c) d();\n\nelse e();\nfor (let i = 0;\n    i < 10;\n    i++) {\n}\nwhile (\n    cond\n) { }\n",
        1,
        "if (c) d();\nelse e();",
        "IfStatement 0 21\n Identifier 4 5\n ExpressionStatement 6 11\n  CallExpression 6 10\n   Identifier 6 8\n   List 9 9\n ExpressionStatement 16 21\n  CallExpression 16 20\n   Identifier 16 18\n   List 19 19\n",
    );
    assert_source_statement(
        "if (a)\n{\n    b();\n}\n\n\nif (c) d();\n\nelse e();\nfor (let i = 0;\n    i < 10;\n    i++) {\n}\nwhile (\n    cond\n) { }\n",
        2,
        "for (let i = 0; i < 10; i++) {\n}",
        "ForStatement 0 32\n VariableDeclarationList 5 14\n  List 8 14\n   VariableDeclaration 8 14\n    Identifier 8 10\n    NumericLiteral 12 14\n BinaryExpression 15 22\n  Identifier 15 17\n  LessThanToken 17 19\n  NumericLiteral 19 22\n PostfixUnaryExpression 23 27\n  Identifier 23 25\n Block 28 32\n  List 30 30\n",
    );
    assert_source_statement(
        "if (a)\n{\n    b();\n}\n\n\nif (c) d();\n\nelse e();\nfor (let i = 0;\n    i < 10;\n    i++) {\n}\nwhile (\n    cond\n) { }\n",
        3,
        "while (cond) { }",
        "WhileStatement 0 16\n Identifier 7 11\n Block 12 16\n  List 14 14\n",
    );
    assert_source_statement(
        "export default class extends Base<\n    T,\n    U\n> {\n}\nexport class K<T extends {\n    a: string\n}> {}\nfunction g<T,>(x: T) {}\nconst h = <T,>(x: T) => x;\n",
        0,
        "export default class extends Base<T, U\n> {\n}",
        "ClassDeclaration 0 44\n List -1 -1\n  ExportKeyword 0 6\n  DefaultKeyword 6 14\n List 20 40\n  HeritageClause 20 40\n   List 28 40\n    ExpressionWithTypeArguments 28 40\n     Identifier 28 33\n     List 34 38\n      TypeReference 34 35\n       Identifier 34 35\n      TypeReference 36 38\n       Identifier 36 38\n List 42 42\n",
    );
    assert_source_statement(
        "export default class extends Base<\n    T,\n    U\n> {\n}\nexport class K<T extends {\n    a: string\n}> {}\nfunction g<T,>(x: T) {}\nconst h = <T,>(x: T) => x;\n",
        1,
        "export class K<T extends {\n    a: string;\n}> {\n}",
        "ClassDeclaration 0 48\n List -1 -1\n  ExportKeyword 0 6\n Identifier 12 14\n List 15 43\n  TypeParameter 15 43\n   Identifier 15 16\n   TypeLiteral 24 43\n    List 26 41\n     PropertySignature 26 41\n      Identifier 26 32\n      StringKeyword 33 40\n List 46 46\n",
    );
    assert_source_statement(
        "export default class extends Base<\n    T,\n    U\n> {\n}\nexport class K<T extends {\n    a: string\n}> {}\nfunction g<T,>(x: T) {}\nconst h = <T,>(x: T) => x;\n",
        2,
        "function g<T>(x: T) { }",
        "FunctionDeclaration 0 23\n Identifier 8 10\n List 11 12\n  TypeParameter 11 12\n   Identifier 11 12\n List 14 18\n  Parameter 14 18\n   Identifier 14 15\n   TypeReference 16 18\n    Identifier 16 18\n Block 19 23\n  List 21 21\n",
    );
    assert_source_statement(
        "export default class extends Base<\n    T,\n    U\n> {\n}\nexport class K<T extends {\n    a: string\n}> {}\nfunction g<T,>(x: T) {}\nconst h = <T,>(x: T) => x;\n",
        3,
        "const h = <T,>(x: T) => x;",
        "VariableStatement 0 26\n VariableDeclarationList 0 25\n  List 5 25\n   VariableDeclaration 5 25\n    Identifier 5 7\n    ArrowFunction 9 25\n     List 11 13\n      TypeParameter 11 12\n       Identifier 11 12\n     List 15 19\n      Parameter 15 19\n       Identifier 15 16\n       TypeReference 17 19\n        Identifier 17 19\n     EqualsGreaterThanToken 20 23\n     Identifier 23 25\n",
    );
    assert_source_statement(
        "type Obj = {\n\n    a: string;\n\n\n    b: number;\n};\ninterface J extends\n    A,\n    B {\n}\ntype Fn2 = {\n    (a: string): void;\n    new (\n        b: number\n    ): J;\n};\ntype Idx = {\n    [\n        key: string\n    ]: number;\n};\n",
        0,
        "type Obj = {\n    a: string;\n    b: number;\n};",
        "TypeAliasDeclaration 0 45\n Identifier 4 8\n TypeLiteral 10 44\n  List 12 42\n   PropertySignature 12 27\n    Identifier 12 18\n    StringKeyword 19 26\n   PropertySignature 27 42\n    Identifier 27 33\n    NumberKeyword 34 41\n",
    );
    assert_source_statement(
        "type Obj = {\n\n    a: string;\n\n\n    b: number;\n};\ninterface J extends\n    A,\n    B {\n}\ntype Fn2 = {\n    (a: string): void;\n    new (\n        b: number\n    ): J;\n};\ntype Idx = {\n    [\n        key: string\n    ]: number;\n};\n",
        1,
        "interface J extends A, B {\n}",
        "InterfaceDeclaration 0 28\n Identifier 9 11\n List 11 24\n  HeritageClause 11 24\n   List 19 24\n    TypeReference 19 21\n     Identifier 19 21\n    TypeReference 22 24\n     Identifier 22 24\n List 26 26\n",
    );
    assert_source_statement(
        "type Obj = {\n\n    a: string;\n\n\n    b: number;\n};\ninterface J extends\n    A,\n    B {\n}\ntype Fn2 = {\n    (a: string): void;\n    new (\n        b: number\n    ): J;\n};\ntype Idx = {\n    [\n        key: string\n    ]: number;\n};\n",
        2,
        "type Fn2 = {\n    (a: string): void;\n    new (b: number\n    ): J;\n};",
        "TypeAliasDeclaration 0 67\n Identifier 4 8\n TypeLiteral 10 66\n  List 12 64\n   CallSignature 12 35\n    List 18 27\n     Parameter 18 27\n      Identifier 18 19\n      StringKeyword 20 27\n    VoidKeyword 29 34\n   ConstructSignature 35 64\n    List 45 54\n     Parameter 45 54\n      Identifier 45 46\n      NumberKeyword 47 54\n    TypeReference 61 63\n     Identifier 61 63\n",
    );
    assert_source_statement(
        "type Obj = {\n\n    a: string;\n\n\n    b: number;\n};\ninterface J extends\n    A,\n    B {\n}\ntype Fn2 = {\n    (a: string): void;\n    new (\n        b: number\n    ): J;\n};\ntype Idx = {\n    [\n        key: string\n    ]: number;\n};\n",
        3,
        "type Idx = {\n    [key: string\n    ]: number;\n};",
        "TypeAliasDeclaration 0 47\n Identifier 4 8\n TypeLiteral 10 46\n  List 12 44\n   IndexSignature 12 44\n    List 18 29\n     Parameter 18 29\n      Identifier 18 21\n      StringKeyword 22 29\n    NumberKeyword 36 43\n",
    );
    assert_source_statement(
        "switch (\n    x\n) {\n    case 1: case 2:\n        break;\n    default: {\n    }\n}\ntry { a(); }\ncatch { }\n",
        0,
        "switch (x) {\n    case 1:case 2:\n        break;\n    default: {\n    }\n}",
        "SwitchStatement 0 69\n Identifier 8 9\n CaseBlock 10 69\n  List 12 67\n   CaseClause 12 24\n    NumericLiteral 21 23\n    List 24 24\n   CaseClause 24 46\n    NumericLiteral 28 30\n    List 31 46\n     BreakStatement 31 46\n   DefaultClause 46 67\n    List 59 67\n     Block 59 67\n      List 61 61\n",
    );
    assert_source_statement(
        "switch (\n    x\n) {\n    case 1: case 2:\n        break;\n    default: {\n    }\n}\ntry { a(); }\ncatch { }\n",
        1,
        "try {\n    a();}\ncatch { }",
        "TryStatement 0 25\n Block 3 15\n  List 5 14\n   ExpressionStatement 5 14\n    CallExpression 5 13\n     Identifier 5 11\n     List 12 12\n CatchClause 15 25\n  Block 21 25\n   List 23 23\n",
    );
    assert_source_statement(
        "enum Color {\n    Red,\n\n    Green = 2,\n    // blue\n    Blue\n}\nconst enum CE { A, B }\ndeclare enum DE {\n}\n",
        0,
        "enum Color {\n    Red,\n    Green = 2,\n    // blue\n    Blue\n}",
        "EnumDeclaration 0 59\n Identifier 4 10\n List 12 57\n  EnumMember 12 20\n   Identifier 12 20\n  EnumMember 21 35\n   Identifier 21 31\n   NumericLiteral 33 35\n  EnumMember 36 57\n   Identifier 36 57\n",
    );
    assert_source_statement(
        "enum Color {\n    Red,\n\n    Green = 2,\n    // blue\n    Blue\n}\nconst enum CE { A, B }\ndeclare enum DE {\n}\n",
        1,
        "const enum CE {\n    A,B}",
        "EnumDeclaration 0 24\n List -1 -1\n  ConstKeyword 0 5\n Identifier 10 13\n List 15 23\n  EnumMember 15 21\n   Identifier 15 21\n  EnumMember 22 23\n   Identifier 22 23\n",
    );
    assert_source_statement(
        "enum Color {\n    Red,\n\n    Green = 2,\n    // blue\n    Blue\n}\nconst enum CE { A, B }\ndeclare enum DE {\n}\n",
        2,
        "declare enum DE {\n}",
        "EnumDeclaration 0 19\n List -1 -1\n  DeclareKeyword 0 7\n Identifier 12 15\n List 17 17\n",
    );
    assert_source_statement(
        "const tpl = `line1\nline2 ${\n    expr\n} end`;\nconst arr2 = [\n    // first\n    1,\n    /* second */ 2,\n    3, // third\n];\n",
        0,
        "const tpl = `line1\nline2 ${expr} end`;",
        "VariableStatement 0 38\n VariableDeclarationList 0 37\n  List 5 37\n   VariableDeclaration 5 37\n    Identifier 5 9\n    TemplateExpression 11 37\n     TemplateHead 11 27\n     List 27 37\n      TemplateSpan 27 37\n       Identifier 27 31\n       TemplateTail 31 37\n",
    );
    assert_source_statement(
        "const tpl = `line1\nline2 ${\n    expr\n} end`;\nconst arr2 = [\n    // first\n    1,\n    /* second */ 2,\n    3, // third\n];\n",
        1,
        "const arr2 = [\n    // first\n    1,\n    /* second */ 2,\n    3, // third\n];",
        "VariableStatement 0 73\n VariableDeclarationList 0 72\n  List 5 72\n   VariableDeclaration 5 72\n    Identifier 5 10\n    ArrayLiteralExpression 12 72\n     List 14 61\n      NumericLiteral 14 33\n      NumericLiteral 34 53\n      NumericLiteral 54 60\n",
    );
    assert_source_statement(
        "f(a, b,\n    c, d);\nf(\n    a,\n    // c\n    b\n);\nf(a\n    , b);\nf(...args,\n    last);\n",
        0,
        "f(a, b,\n    c, d);",
        "ExpressionStatement 0 18\n CallExpression 0 17\n  Identifier 0 1\n  List 2 16\n   Identifier 2 3\n   Identifier 4 6\n   Identifier 7 13\n   Identifier 14 16\n",
    );
    assert_source_statement(
        "f(a, b,\n    c, d);\nf(\n    a,\n    // c\n    b\n);\nf(a\n    , b);\nf(...args,\n    last);\n",
        1,
        "f(a,\n    // c\n    b\n);",
        "ExpressionStatement 0 22\n CallExpression 0 21\n  Identifier 0 1\n  List 2 19\n   Identifier 2 3\n   Identifier 4 19\n",
    );
    assert_source_statement(
        "f(a, b,\n    c, d);\nf(\n    a,\n    // c\n    b\n);\nf(a\n    , b);\nf(...args,\n    last);\n",
        2,
        "f(a,\n    b);",
        "ExpressionStatement 0 12\n CallExpression 0 11\n  Identifier 0 1\n  List 2 10\n   Identifier 2 3\n   Identifier 4 10\n",
    );
    assert_source_statement(
        "f(a, b,\n    c, d);\nf(\n    a,\n    // c\n    b\n);\nf(a\n    , b);\nf(...args,\n    last);\n",
        3,
        "f(...args,\n    last);",
        "ExpressionStatement 0 21\n CallExpression 0 20\n  Identifier 0 1\n  List 2 19\n   SpreadElement 2 9\n    Identifier 5 9\n   Identifier 10 19\n",
    );
    assert_source_statement(
        "const { a,\n    b: { c },\n    ...rest } = obj;\nconst [x,\n    , y] = arr;\nfunction h({\n    a,\n    b\n}) {}\n",
        0,
        "const { a, b: { c }, ...rest } = obj;",
        "VariableStatement 0 37\n VariableDeclarationList 0 36\n  List 5 36\n   VariableDeclaration 5 36\n    ObjectBindingPattern 5 30\n     List 7 28\n      BindingElement 7 9\n       Identifier 7 9\n      BindingElement 10 19\n       Identifier 10 12\n       ObjectBindingPattern 13 19\n        List 15 17\n         BindingElement 15 17\n          Identifier 15 17\n      BindingElement 20 28\n       DotDotDotToken 20 24\n       Identifier 24 28\n    Identifier 32 36\n",
    );
    assert_source_statement(
        "const { a,\n    b: { c },\n    ...rest } = obj;\nconst [x,\n    , y] = arr;\nfunction h({\n    a,\n    b\n}) {}\n",
        1,
        "const [x, , y] = arr;",
        "VariableStatement 0 21\n VariableDeclarationList 0 20\n  List 5 20\n   VariableDeclaration 5 20\n    ArrayBindingPattern 5 14\n     List 7 13\n      BindingElement 7 8\n       Identifier 7 8\n      BindingElement 9 9\n      BindingElement 11 13\n       Identifier 11 13\n    Identifier 16 20\n",
    );
    assert_source_statement(
        "const { a,\n    b: { c },\n    ...rest } = obj;\nconst [x,\n    , y] = arr;\nfunction h({\n    a,\n    b\n}) {}\n",
        2,
        "function h({ a, b\n}) { }",
        "FunctionDeclaration 0 24\n Identifier 8 10\n List 11 19\n  Parameter 11 19\n   ObjectBindingPattern 11 19\n    List 12 17\n     BindingElement 12 14\n      Identifier 12 14\n     BindingElement 15 17\n      Identifier 15 17\n Block 20 24\n  List 22 22\n",
    );
    assert_source_statement(
        "class M {\n    constructor() {}\n    public foo() {}\n\n    private bar() {\n        return 1;\n    }\n    get baz() {\n        return 2;\n    }\n    set baz(v) {}\n    static qux = 1;\n    #priv = 2;\n}\n",
        0,
        "class M {\n    constructor() { }\n    public foo() { }\n    private bar() {\n        return 1;\n    }\n    get baz() {\n        return 2;\n    }\n    set baz(v) { }\n    static qux = 1;\n    #priv = 2;\n}",
        "ClassDeclaration 0 192\n Identifier 5 7\n List 9 190\n  Constructor 9 31\n   List 26 26\n   Block 27 31\n    List 29 29\n  MethodDeclaration 31 52\n   List -1 -1\n    PublicKeyword 31 42\n   Identifier 42 46\n   List 47 47\n   Block 48 52\n    List 50 50\n  MethodDeclaration 52 96\n   List -1 -1\n    PrivateKeyword 52 64\n   Identifier 64 68\n   List 69 69\n   Block 70 96\n    List 72 90\n     ReturnStatement 72 90\n      NumericLiteral 87 89\n  GetAccessor 96 136\n   Identifier 104 108\n   List 109 109\n   Block 110 136\n    List 112 130\n     ReturnStatement 112 130\n      NumericLiteral 127 129\n  SetAccessor 136 155\n   Identifier 144 148\n   List 149 150\n    Parameter 149 150\n     Identifier 149 150\n   Block 151 155\n    List 153 153\n  PropertyDeclaration 155 175\n   List -1 -1\n    StaticKeyword 155 166\n   Identifier 166 170\n   NumericLiteral 172 174\n  PropertyDeclaration 175 190\n   PrivateIdentifier 175 185\n   NumericLiteral 187 189\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        0,
        "function f() { a(); b(); }",
        "FunctionDeclaration 0 26\n Identifier 8 10\n List 11 11\n Block 12 26\n  List 14 24\n   ExpressionStatement 14 19\n    CallExpression 14 18\n     Identifier 14 16\n     List 17 17\n   ExpressionStatement 19 24\n    CallExpression 19 23\n     Identifier 19 21\n     List 22 22\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        1,
        "function g() {\n    a();b();\n    c();\n}",
        "FunctionDeclaration 0 38\n Identifier 8 10\n List 11 11\n Block 12 38\n  List 14 36\n   ExpressionStatement 14 23\n    CallExpression 14 22\n     Identifier 14 20\n     List 21 21\n   ExpressionStatement 23 27\n    CallExpression 23 26\n     Identifier 23 24\n     List 25 25\n   ExpressionStatement 27 36\n    CallExpression 27 35\n     Identifier 27 33\n     List 34 34\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        3,
        "if (x) {\n    a();b();} else {\n    c();}",
        "IfStatement 0 39\n Identifier 4 5\n Block 6 22\n  List 8 21\n   ExpressionStatement 8 17\n    CallExpression 8 16\n     Identifier 8 14\n     List 15 15\n   ExpressionStatement 17 21\n    CallExpression 17 20\n     Identifier 17 18\n     List 19 19\n Block 27 39\n  List 29 38\n   ExpressionStatement 29 38\n    CallExpression 29 37\n     Identifier 29 35\n     List 36 36\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        4,
        "{\n    a();}",
        "Block 0 11\n List 1 10\n  ExpressionStatement 1 10\n   CallExpression 1 9\n    Identifier 1 7\n    List 8 8\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        5,
        "{\n}",
        "Block 0 3\n List 1 1\n",
    );
    assert_source_statement(
        "function f() { a(); b(); }\nfunction g() {\n    a(); b();\n\n    c();\n}\nif (x) { a(); }\nif (x) { a(); b(); } else { c(); }\n{ a(); }\n{\n}\n{ }\n",
        6,
        "{ }",
        "Block 0 3\n List 1 1\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        0,
        "class A {\n    a = 1;b = 2;}",
        "ClassDeclaration 0 27\n Identifier 5 7\n List 9 26\n  PropertyDeclaration 9 20\n   Identifier 9 15\n   NumericLiteral 17 19\n  PropertyDeclaration 20 26\n   Identifier 20 21\n   NumericLiteral 23 25\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        1,
        "class B {\n    a = 1;b = 2;\n    m() { return 1; }\n}",
        "ClassDeclaration 0 50\n Identifier 5 7\n List 9 48\n  PropertyDeclaration 9 20\n   Identifier 9 15\n   NumericLiteral 17 19\n  PropertyDeclaration 20 26\n   Identifier 20 21\n   NumericLiteral 23 25\n  MethodDeclaration 26 48\n   Identifier 26 32\n   List 33 33\n   Block 34 48\n    List 36 46\n     ReturnStatement 36 46\n      NumericLiteral 43 45\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        2,
        "interface I {\n    a: string;b: number;}",
        "InterfaceDeclaration 0 39\n Identifier 9 11\n List 13 38\n  PropertySignature 13 28\n   Identifier 13 19\n   StringKeyword 20 27\n  PropertySignature 28 38\n   Identifier 28 29\n   NumberKeyword 30 37\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        3,
        "interface J {\n    a: string;b: number;\n}",
        "InterfaceDeclaration 0 40\n Identifier 9 11\n List 13 38\n  PropertySignature 13 28\n   Identifier 13 19\n   StringKeyword 20 27\n  PropertySignature 28 38\n   Identifier 28 29\n   NumberKeyword 30 37\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        4,
        "type T = {\n    a: string;b: number;};",
        "TypeAliasDeclaration 0 37\n Identifier 4 6\n TypeLiteral 8 36\n  List 10 35\n   PropertySignature 10 25\n    Identifier 10 16\n    StringKeyword 17 24\n   PropertySignature 25 35\n    Identifier 25 26\n    NumberKeyword 27 34\n",
    );
    assert_source_statement(
        "class A { a = 1; b = 2; }\nclass B {\n    a = 1; b = 2;\n    m() { return 1; }\n}\ninterface I { a: string; b: number }\ninterface J {\n    a: string; b: number;\n}\ntype T = { a: string; b: number };\ntype U = {\n    a: string; b: number;\n};\n",
        5,
        "type U = {\n    a: string;b: number;\n};",
        "TypeAliasDeclaration 0 38\n Identifier 4 6\n TypeLiteral 8 37\n  List 10 35\n   PropertySignature 10 25\n    Identifier 10 16\n    StringKeyword 17 24\n   PropertySignature 25 35\n    Identifier 25 26\n    NumberKeyword 27 34\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        0,
        "enum E {\n    A,B}",
        "EnumDeclaration 0 17\n Identifier 4 6\n List 8 16\n  EnumMember 8 14\n   Identifier 8 14\n  EnumMember 15 16\n   Identifier 15 16\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        1,
        "enum F {\n    A,B,\n    C\n}",
        "EnumDeclaration 0 25\n Identifier 4 6\n List 8 23\n  EnumMember 8 14\n   Identifier 8 14\n  EnumMember 15 16\n   Identifier 15 16\n  EnumMember 17 23\n   Identifier 17 23\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        2,
        "enum G {\n    A = 1}",
        "EnumDeclaration 0 19\n Identifier 4 6\n List 8 18\n  EnumMember 8 18\n   Identifier 8 14\n   NumericLiteral 16 18\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        3,
        "namespace N {\n    export const a = 1;export const b = 2;}",
        "ModuleDeclaration 0 57\n Identifier 9 11\n ModuleBlock 11 57\n  List 13 56\n   VariableStatement 13 37\n    List -1 -1\n     ExportKeyword 13 24\n    VariableDeclarationList 24 36\n     List 30 36\n      VariableDeclaration 30 36\n       Identifier 30 32\n       NumericLiteral 34 36\n   VariableStatement 37 56\n    List -1 -1\n     ExportKeyword 37 43\n    VariableDeclarationList 43 55\n     List 49 55\n      VariableDeclaration 49 55\n       Identifier 49 51\n       NumericLiteral 53 55\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        4,
        "namespace M {\n    const a = 1;const b = 2;\n}",
        "ModuleDeclaration 0 44\n Identifier 9 11\n ModuleBlock 11 44\n  List 13 42\n   VariableStatement 13 30\n    VariableDeclarationList 13 29\n     List 23 29\n      VariableDeclaration 23 29\n       Identifier 23 25\n       NumericLiteral 27 29\n   VariableStatement 30 42\n    VariableDeclarationList 30 41\n     List 35 41\n      VariableDeclaration 35 41\n       Identifier 35 37\n       NumericLiteral 39 41\n",
    );
    assert_source_statement(
        "enum E { A, B }\nenum F {\n    A, B,\n    C\n}\nenum G { A = 1 }\nnamespace N { export const a = 1; export const b = 2; }\nnamespace M {\n    const a = 1; const b = 2;\n}\ndeclare module \"m\" { export const x: number; }\n",
        5,
        "declare module \"m\" {\n    export const x: number;}",
        "ModuleDeclaration 0 49\n List -1 -1\n  DeclareKeyword 0 7\n StringLiteral 14 18\n ModuleBlock 18 49\n  List 20 48\n   VariableStatement 20 48\n    List -1 -1\n     ExportKeyword 20 31\n    VariableDeclarationList 31 47\n     List 37 47\n      VariableDeclaration 37 47\n       Identifier 37 39\n       NumberKeyword 40 47\n",
    );
    assert_source_statement(
        "switch (x) { case 1: a(); break; case 2: b(); }\nswitch (y) {\n    case 1: a(); break;\n    default:\n        b();\n        c();\n}\nswitch (z) {\n}\n",
        0,
        "switch (x) {\n    case 1:\n        a();break;case 2: b();}",
        "SwitchStatement 0 56\n Identifier 8 9\n CaseBlock 10 56\n  List 12 55\n   CaseClause 12 43\n    NumericLiteral 21 23\n    List 24 43\n     ExpressionStatement 24 37\n      CallExpression 24 36\n       Identifier 24 34\n       List 35 35\n     BreakStatement 37 43\n   CaseClause 43 55\n    NumericLiteral 47 49\n    List 50 55\n     ExpressionStatement 50 55\n      CallExpression 50 54\n       Identifier 50 52\n       List 53 53\n",
    );
    assert_source_statement(
        "switch (x) { case 1: a(); break; case 2: b(); }\nswitch (y) {\n    case 1: a(); break;\n    default:\n        b();\n        c();\n}\nswitch (z) {\n}\n",
        1,
        "switch (y) {\n    case 1:\n        a();break;\n    default:\n        b();\n        c();\n}",
        "SwitchStatement 0 84\n Identifier 8 9\n CaseBlock 10 84\n  List 12 82\n   CaseClause 12 43\n    NumericLiteral 21 23\n    List 24 43\n     ExpressionStatement 24 37\n      CallExpression 24 36\n       Identifier 24 34\n       List 35 35\n     BreakStatement 37 43\n   DefaultClause 43 82\n    List 56 82\n     ExpressionStatement 56 69\n      CallExpression 56 68\n       Identifier 56 66\n       List 67 67\n     ExpressionStatement 69 82\n      CallExpression 69 81\n       Identifier 69 79\n       List 80 80\n",
    );
    assert_source_statement(
        "switch (x) { case 1: a(); break; case 2: b(); }\nswitch (y) {\n    case 1: a(); break;\n    default:\n        b();\n        c();\n}\nswitch (z) {\n}\n",
        2,
        "switch (z) {\n}",
        "SwitchStatement 0 14\n Identifier 8 9\n CaseBlock 10 14\n  List 12 12\n",
    );
    assert_source_statement(
        "const o = { a: 1, b: 2 };\nconst p = {\n    a: 1, b: 2,\n    c: 3 };\nconst arr = [1, 2,\n    3];\nconst arrow = () => { a(); b(); };\nconst fnExpr = function () { return 1; };\nclass C { constructor(private a: number) { } }\n",
        0,
        "const o = { a: 1, b: 2 };",
        "VariableStatement 0 25\n VariableDeclarationList 0 24\n  List 5 24\n   VariableDeclaration 5 24\n    Identifier 5 7\n    ObjectLiteralExpression 9 24\n     List 11 22\n      PropertyAssignment 11 16\n       Identifier 11 13\n       NumericLiteral 14 16\n      PropertyAssignment 17 22\n       Identifier 17 19\n       NumericLiteral 20 22\n",
    );
    assert_source_statement(
        "const o = { a: 1, b: 2 };\nconst p = {\n    a: 1, b: 2,\n    c: 3 };\nconst arr = [1, 2,\n    3];\nconst arrow = () => { a(); b(); };\nconst fnExpr = function () { return 1; };\nclass C { constructor(private a: number) { } }\n",
        1,
        "const p = {\n    a: 1, b: 2,\n    c: 3\n};",
        "VariableStatement 0 39\n VariableDeclarationList 0 38\n  List 5 38\n   VariableDeclaration 5 38\n    Identifier 5 7\n    ObjectLiteralExpression 9 38\n     List 11 36\n      PropertyAssignment 11 20\n       Identifier 11 17\n       NumericLiteral 18 20\n      PropertyAssignment 21 26\n       Identifier 21 23\n       NumericLiteral 24 26\n      PropertyAssignment 27 36\n       Identifier 27 33\n       NumericLiteral 34 36\n",
    );
    assert_source_statement(
        "const o = { a: 1, b: 2 };\nconst p = {\n    a: 1, b: 2,\n    c: 3 };\nconst arr = [1, 2,\n    3];\nconst arrow = () => { a(); b(); };\nconst fnExpr = function () { return 1; };\nclass C { constructor(private a: number) { } }\n",
        3,
        "const arrow = () => { a(); b(); };",
        "VariableStatement 0 34\n VariableDeclarationList 0 33\n  List 5 33\n   VariableDeclaration 5 33\n    Identifier 5 11\n    ArrowFunction 13 33\n     List 15 15\n     EqualsGreaterThanToken 16 19\n     Block 19 33\n      List 21 31\n       ExpressionStatement 21 26\n        CallExpression 21 25\n         Identifier 21 23\n         List 24 24\n       ExpressionStatement 26 31\n        CallExpression 26 30\n         Identifier 26 28\n         List 29 29\n",
    );
    assert_source_statement(
        "const o = { a: 1, b: 2 };\nconst p = {\n    a: 1, b: 2,\n    c: 3 };\nconst arr = [1, 2,\n    3];\nconst arrow = () => { a(); b(); };\nconst fnExpr = function () { return 1; };\nclass C { constructor(private a: number) { } }\n",
        4,
        "const fnExpr = function () { return 1; };",
        "VariableStatement 0 41\n VariableDeclarationList 0 40\n  List 5 40\n   VariableDeclaration 5 40\n    Identifier 5 12\n    FunctionExpression 14 40\n     List 25 25\n     Block 26 40\n      List 28 38\n       ReturnStatement 28 38\n        NumericLiteral 35 37\n",
    );
    assert_source_statement(
        "const o = { a: 1, b: 2 };\nconst p = {\n    a: 1, b: 2,\n    c: 3 };\nconst arr = [1, 2,\n    3];\nconst arrow = () => { a(); b(); };\nconst fnExpr = function () { return 1; };\nclass C { constructor(private a: number) { } }\n",
        5,
        "class C {\n    constructor(private a: number) { }}",
        "ClassDeclaration 0 49\n Identifier 5 7\n List 9 48\n  Constructor 9 48\n   List 26 43\n    Parameter 26 43\n     List -1 -1\n      PrivateKeyword 26 33\n     Identifier 33 35\n     NumberKeyword 36 43\n   Block 44 48\n    List 46 46\n",
    );
}

#[track_caller]
fn assert_source_statement(text: &str, index: usize, printed: &str, expected: &str) {
    let source = parse_source_file(
        "/a.ts",
        text.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
        unreachable!()
    };
    let statement = source.arena.node_array(data.statements.unwrap()).nodes[index];
    let mut arena = TransformArena::new();
    let id = arena.add_source(&source, None);
    let mut transformation = transform_nodes(arena, Vec::new(), Vec::new(), true).unwrap();
    let positioned = print_and_position(
        &mut transformation,
        TransformNode::new(id, statement),
        PositionedPrint {
            new_line: NewLineKind::LineFeed,
            indent_size: 4,
            with_source_file: true,
            language_variant: LanguageVariant::Standard,
            snippet: None,
        },
    )
    .unwrap();
    assert_eq!(positioned.text, printed, "{text}");
    let tree = Tree::detached(&positioned.file);
    let mut out = String::new();
    dump(&tree, View::Node(positioned.node), 0, &mut out);
    assert_eq!(out, expected, "{text}");
}
