追加読み取りは不要です。27 行を「即入れられる小修正」「isolatedModules の欠落分岐群」「基盤作業」に分けます。

## A. 単独で小さく閉じる 4 件

**1. TS2597 vs TS2616 (importNonExportedMember9)**
- upstream `reportInvalidImportEqualsExportMember` (`_tsc.js:48945-48958`) は module<ES2015 のとき `isInJSFile(node)` で 2597/2598 系を選ぶ。
- Rust `modules.rs:3007-3034` は「isInJSFile 中間 arm は JS-only (constant-dead)」として省略し、常に 2616/2617。JS ファイルは checkJs で到達する。
- 修正: `else if self.is_in_js_file(name)` arm を追加し、`_0_can_only_be_imported_by_using_a_require_call_or_by_using_a_default_import` / `..._or_by_turning_on_the_esModuleInterop_flag_...` を選ぶ (`merge.rs:272` の `is_in_js_file` を使用)。
- 対照: b.js / b.ts × esModuleInterop 有無 (4 通り) と module=es2015 (2595/2596 系が不変)。

**2. TS8017 の span (jsFileCompilationConstructorOverloadSyntax)**
- upstream `getErrorSpanForNode` (`14080-14099`) は Constructor に特例があり、`skipTrivia(pos)` から scanner で `constructor` キーワードまで進めて `[start, keywordEnd)` を返す (期待長 11 = `constructor`)。
- Rust `js_grammar.rs:133-160` にこの arm がなく node 全体 (長 14)。
- 修正: `error_span_for_node` に Constructor arm を追加。`token_span_at` (193-208) と同じ `scan_tokens` で ConstructorKeyword まで走査。同関数を使う他の JS grammar 診断 (8009/8012 など、constructor 上のもの) も同時に upstream と揃う。
- 対照: `constructor();`、`/* c */ constructor();`、`public constructor();` (8009 との併発)。

**3. TS2343 誤検出 (tslibReExportHelpers2)**
- upstream `checkExternalEmitHelpers` (`88920`) は `getSymbol(getExportsOfModule(m), name, Value)` を使い、`getSymbol` (`47904-47919`) は **Alias なら resolve 先の flags で判定**する。
- Rust `modules.rs:6277-6285` は resolve 前に raw `SymbolFlags::VALUE` で filter するため、`export { __classPrivateFieldGet } from "./index.js"` の alias を落として 2343 を出す。
- 修正: filter を `self.get_symbol_in_table(&exports, name, VALUE)` (`resolve.rs:43-56`、同 port) に置き換えてから `resolve_symbol_ex`。
- 対照: 直接 export / `export {} from` / `export * from` の各 tslib、および `export type __awaiter` (値なし → 依然 2343)。

**4. TS2343 `__rest` 未報告 (tslibMultipleMissingHelper)**
- upstream は Rest helper を 2 箇所で要求: `checkVariableLikeDeclaration` の object binding rest (`83423-83424`) と分割代入の spread property (`79649`)、いずれも target < ES2018。
- Rust は helper 4 の要求が 0 件 (grep)。`operators.rs:2154` に「unmodeled」と明記。
- 修正: `statements.rs:1200` 付近 (dot_dot_dot 分岐、親が ObjectBindingPattern) と `operators.rs:2156` に `check_external_emit_helpers(node, 4)` を target ゲート付きで追加。
- 対照: parameter rest / variable rest / `({a, ...r} = o)` × target es2017 (報告) と es2018 (非報告)。importHelpers 未指定では no-op なので影響範囲は狭い。`__assign` (spread 式) の要求有無も同時に確認推奨。

## B. isolatedModules 群 (8 行): 移植済み関数の欠落分岐、個々は小さいが同時に入れる

| 診断 | upstream | Rust 所在 | 欠落 |
| --- | --- | --- | --- |
| 2865 | `86073-86083` | `modules.rs:8862-8871` の直後 | `node.kind != ExportSpecifier && isolatedModules && !findAncestor(type-only) && symbol.flags & (Value\|ExportValue)` の else-if arm |
| 1269 | `86102-86104` | `modules.rs:8884-8933` の ImportEquals arm | `is_type && ImportEquals && export 修飾子` の error |
| 1280 | `85877-85879` | `modules.rs:8253` (「outside this slice」と明記) | `get_isolated_modules && external_module_indicator.is_none()` |
| 1281 | `19608-19620` | `resolve.rs:428` EnumDeclaration case (167-168 で意図的省略) | `name_not_found_message.is_some() && isolated && !ambient && file(location) != file(result.valueDeclaration)` |
| 2866 | `48194-48203` | `resolve.rs:2093-2133` の後 | `isInExternalModule` フラグ (SourceFile 到達時に計算) + globals 一致 + `last_location.locals` の非 Value 側 lookup + import 宣言特定 |

2865/1269/1280 は各 5-15 行。1281/2866 は `resolve_name_full` 内で、2866 は `is_in_external_module` を loop に追加する必要があり最も触る範囲が広い。いずれも診断のみで emit に影響なし。共有リスクは isolatedModules 帯全体の FP なので、focused 対照に加えて hosted の該当 band FP=0 を必ず再確認。対照は fixture の bad/good 対に、次を追加: `verbatimModuleSyntax` のみ (2865 は出ない)、value namespace への `export import` (1269 なし)、非 instantiated namespace の script (1280 なし)、同一ファイル enum member (1281 なし)、型としてのみ使う global 名 (2866 なし)。

## C. 基盤作業として分類 (15 行)

- **関係/深さ/表示**: deeplyNestedMappedTypes (欠落 3 件 + 型表示 `{} & {} & …`)、recursiveConditionalCrash4 (2589)、relationComplexityError (2859)、mappedArrayTupleIntersections、jsxIntrinsic…TemplateLiteral (literal 表示)。`isDeeplyNestedType`/instantiation depth/alias 表示の共通基盤。
- **relatedInformation の elaboration**: excessPropertyCheckIntersectionWithRecursiveType (Rust が余分な 6501)、jsdocArrayObjectPromiseNoImplicitAny・typeSatisfaction_errorLocations1・checkJsdocSatisfiesTag9 (6500)。4 行は `elaborateError` 系の lib 宣言に対する related-info 付与規則という **単一原因**の可能性が高く、まとめて 1 slice。
- **JS 固有**: genericDefaultsJs・jsExtendsImplicitAny (JS class heritage の construct signature)、contextuallyTypedParametersOptionalInJSDoc、jsExportMemberMergedWithModuleAugmentation2、jsdocTypedefBeforeParenthesizedExpression、jsDeclarationsTypedefAndLatebound。JS binder/契約型付けの基盤。

推奨順は A1→A3→A2→A4 (それぞれ独立、行ごとに exact 化可能)、次に B を 1 つの isolatedModules train として。
