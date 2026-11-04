# checker の data 層の書き直し：計画（2026-11-04）

利用者の決定（2026-11-04）：「書き直し（data 層）に進みましょう。そのために必要な変更をまずは計画してください」。
前提となる事実と判断は [README](README.md) の「計測台帳」と、戦略 v2.1 の再見積もり
（`target/claude-tools/handover/perf-tsrs/strategy-rebaseline-2026-11-04.md`）にある。要点：census では型・symbol・instantiation の
数も checker の主要関数の入口回数も tsgo と同じ（1.00〜1.05×）なのに、VS Code 1 thread で命令数は tsrs の 1.57×、peak は 1.78×。
差は仕事の量ではなく **1 操作あたりの費用と record の形**にあり、front end や memo（C-0 は VS Code で ±0）では縮まらなかった。

## 0. 範囲と原則

**変えるもの（data 層）**：Symbol / Type / Signature / members / mapper / inference の record と tail、links の表、member 表、
id → record の経路（routing）、list の持ち方と複製、文字列、flow node、leaf の解放。
**変えないもの**：checker のロジック（conformance 一致を支える細部、型の生成順、診断の順序、cache の鍵の意味）。record の形が
変わっても、生成される型・symbol の **順序と個数**（`--extendedDiagnostics` の Types / Symbols / Instantiations）は不変。

原則（利用者の指示 2026-11-02「data model」、2026-11-04「計測」）：
1. tsgo / tsrs の形に合わせる：固定 address の store（id = 位置）、header + 種類別の tail、flag は bit、必要な object は一括確保、
   heap に不要な物を置かない（`Box` / `Arc` / `Vec` の record 内の所有は原則なし）。mode flag・並行実装・互換 shim は作らない。
2. unsafe は最小。store の中で、生存と初期化の証明がある読みだけ（`store::get` の形）。
3. slice ごとに着地前の予想（MiB と G）と着地後の `serial.py` の実測・memory report の行を台帳に残す。帯を下回ったら同じ track の
   次の step に進まず原因を書く（profile の差）。予想の作法（D1 の教訓、2026-11-04）：memory 帯域に縛られる code（relocation、
   publish の複製、tail の書き出し、leaf の解放）は profile の ms × 平均の命令率では見積もらず、CPU 時間（複数 round）か
   word あたりの命令数で見積もる。出力の一致（4-checker の conformance 15,339 構成、8 project、Types/Symbols の行）は
   全 step の gate。

## 1. 現状（VS Code 1 thread、check 終了時の live、MiB）と tsrs の形

| store | 今の record | 個数 | live MiB | tsrs の形 | 根拠 |
|---|---:|---:|---:|---|---|
| node（parser） | 32 B + tail | 1,720 万 | 525 | 24 B + 種類別 | A-1a part 2 |
| binder Symbol | 48 B | 227 万 | 104（表 85、extras 29） | 16 B header + 16 B data + 24 B tables tail | symbol-report §1, tsrs-layouts §2 |
| transient Symbol（checker） | 48 B、別 arena、id の上位 bit で routing | 589 万 | 270（予約 384） | 同じ struct、checker の arena | symbol-report §4 |
| SymbolLinks | 28 B、PagedTable（4 load）+ cold 37 map | 711 万 | 254 + 28.5 | value links 16 B、checker 直下の dense 表 | links-routing §2, tsrs-layouts §6 |
| NodeLinks | 16 B、PagedTable + cold | — | 150 + 6 | 8 B、hash（node に id なし） | links-routing §2 |
| Type | 40 B（TypeData 24）+ list 表 | 258 万 | 256 + 45 | 24 B header + 種類別 tail（40〜72 B） | slice 11, tsrs-layouts §3 |
| TypeLinks | 24 B + cold | — | 61 + 11 | 型の field に inline | links-routing §7.4 |
| Signature | 136 B（予約 2^20） | 60 万 | 85（行 147） | 48 B + 24 B rare tail | b3 §2.1, tsrs-layouts §4 |
| ResolvedMembers | 80 B + extras Box + properties Arc | 122 万 | 287（行 364） | StructuredMembers 32 B、properties は保存 slice | b3 §2.1 |
| TypeMapper | 24 B enum | 258 万 | 62（行 96） | 16 B（2 word、kind は下位 bit） | b3 §3.3 |
| InferenceContext / Info | 152 / 88 B、Vec 3 本 | 92 万 | 103（行 120） | 48 / 24 B、再利用 | b3 §3.5–3.6 |
| 関係 cache | Pair + Generic の 2 表（#872） | — | 34 | 8 B key | b4 |
| 文字列 literal の表 | 同じ文を 3 回保持 | — | 56 | — | strings §0 |
| flow node | 32 B + Vec（余り 32） | — | 116 | 16 B + pool | links-routing §7.3 |
| source text・binder per-node 表 | — | — | 136 + 148 | leaf は check 後に解放 | leaf-freeing §0 |

費用の所在（instruction audit・census）：`PagedTable::get` 2.3%（1.5 + 0.95 億回）、file routing 1.2〜1.8%、複製と確保
（list の `to_vec` 149 か所、`get_properties_of_type` 等の返り値の複製、`Signature` の clone）sample の約 12%、allocator の outlined
fragment 218.6 ms、名前表の読み（`as_js` 5,200 万回）、member 表の probe。tsrs の台帳は「peak 1% 減につき命令数 +0.16%」で、
memory の圧縮は CPU を少し払う——命令数は **routing・複製・cache miss** から取る。

## 2. 目標の形（record ごとの契約。正確な layout は各設計報告に従う）

共通（store 契約、B-2 PR0 / B3-0 を拡張）：
- **claimed room**：checker ごとに同じ region の room を claim し、zero-valid な Copy record（Symbol・Type・Signature header）を
  id = 位置で置く。publish 不要（room が最終の場所）、成長は新しい chunk（発行済みの id は動かない）。
- **frozen span**：所有者の寿命を持つ append-only の span（declarations、type list、signature list、member の view）。binder の pool
  は program の寿命、checker の pool は checker の寿命。
- **recycle**：checker の scratch（inference context・mapper）だけ、B-3 の escape bit つき。
- **生成器**：`scripts/generate_nodes.py` と同じく layout JSON から record・tail・bit field・size assertion・memory 行を生成する
  （`records.layout.json` → `generate_records.py`）。手書きの offset 計算をなくす。

| # | record | 契約 | 設計報告 |
|---|---|---|---|
| S1 | **Symbol 32 B、binder と transient を統一** | 同じ struct、同じ id 空間（transient は checker の claimed room、上位 bit の tag と `symbol()` の分岐を廃止）。field：flags 4、name 4、value_declaration 4、parent 4、declarations span 8（len は u16 + overflow か flags の空き bit）、tail か inline word（members / exports / export_symbol。VS Code の実測（Opus、2026-11-04）：binder symbol 227 万のうち members 12.6%・exports 1.2%・export_symbol 2.2%、いずれか 15.2% = 34.5 万、今の extras Box 29.6 万 ≈ 11〜12 MiB；transient 589 万では 0.016%、ただし declarations > 2 が 19.5 万（union/intersection の property）。15% は「稀な tail」には多すぎるので、TableId 4 B × 2 を inline に置く案と、checker 生成 record ではその word を transient 専用の links（S2）に使う案を S1 の memo で決める）。**順序 id**（R29 の lazy id、tsgo の `GetSymbolId`）は checker ごとの links に dense に置く（record には入れない）。API / LS の symbol handle は transient bit を見ない。 | symbol-report §8（Option C 寄り）、Opus の入力 |
| S2 | **SymbolLinks を population で分割** | 永続 symbol：12 B（type_of_symbol、declared_type、is_referenced）の 2 段表。transient：checker 生成 record の room に 16 B（check_flags、target、mapper、name_type）を **inline**（tsrs と同じ）。`get_check_flags` は永続 id に lookup なしで 0。cold 37 map は `IdSlot` 4 B に。 | links-routing §7.1 |
| S3 | **NodeLinks の 2 段表と routing の除去** | 分岐なしの 2 段表（256 id 粒度）、far path は cold。`is_in_js_file` 等は node の flag、JSDoc は JS file だけ、`resolve_name` の locals は `HAS_LOCALS` bit + program 全体の map。 | links-routing §4–§7 |
| S4 | **Type 24 B header + 種類別 tail、TypeLinks の dense 化** | header：flags、object_flags、id、tag、symbol/alias word。tail は union/intersection 24、object 12、reference 24、mapped 44、conditional 44…（tsrs の表）。hot な TypeLinks 24 B（resolved_members、constraint、base_constraint、instantiated target/mapper、deferred_node）は型の lease と同じ room に zero 初期化、残りは sparse。 | tsrs-layouts §3、links-routing §7.4 |
| S5 | **Signature 40 B header + 40 B lazy tail** | parameters は frozen span、type_parameters は TypeListId、return は Vacant/Resolving/Resolved の bit、tail は this_parameter・各 cache・composite。`signature_of` は by-value の view。 | b3 §3.2 |
| S6 | **ResolvedMembers 48 B、PropertiesView、IndexInfo 20 B** | symbols span + 名前 index + properties（AllMembers / Mask32 / List）+ calls / constructs / index_infos の span。member entry は `(SymbolId, EscapedName)` で名前を並べて probe（audit row 3）。 | b3 §3.4、audit |
| S7 | **TypeMapper 12 B、InferenceContext 48 B、InferenceInfo 56 B、scratch の recycle** | 3 word の mapper（kind は下位 bit、ESCAPED bit）、連続した info block、inline の候補 2 個、walk buffer の pool、候補ごとの allocation log の reset。 | b3 §3.3–3.7、§5 |
| S8 | **list と複製** | 保存した list を返す（properties / signatures / index infos、tsrs R18）、`to_vec` 149 か所を range に、TypeListId を cache の plumbing に通す、per-expression の String と node list の複製を消す。 | allocations §8、audit rows 5・7 |
| S9 | **文字列** | 文字列 literal 型の鍵を `JsText` に（3 重保持を 1 つに）、名前表を store 内の平らな表（flag bit つき）、known-name 定数、薄い不変 `JsString` + builder。 | strings §8 |
| S10 | **flow node 16 B + pool、global な flow id** | per-document の antecedent pool、query ごとの `(file, FlowId)` 鍵と label の `to_vec` を消す。 | links-routing §7.3 |
| S11 | **leaf の解放（CLI）** | check 済み leaf の parse / bind 出力（source text、per-node 表、locals）を document 単位で返す。読み graph の 9 条項 + 本 port 固有の 3 経路（undefined_properties cache、recovery resolver、…）を閉じてから。 | leaf-freeing §0–§4 |

## 3. 順序・担当・予想・gate

一度に動かす heavy な job は 1 つ（lane は直列、Opus の build は別枠）なので、writer は **Opus（binder / front end 側の record）と
Codex lane（checker 側の store）の 2 本**、Fable が統合・検証・台帳。各 step は PR 1 本（出力一致 + 計測つき）。予想は VS Code
1 thread、live MiB と命令数。帯の根拠は右の報告。

| phase | step | 担当 | 予想（MiB / 命令数） | 根拠 |
|---|---|---|---|---|
| 0 | storage contract（claimed room・frozen span・recycle の primitive、crates/store）| lane `b2audit`（SPEC 拡張済み） | 0 / 0 | b2 §2, b3 §3.1 |
| 0 | record の生成器（layout JSON → record / tail / bit / assertion / memory 行） | lane | 0 / 0 | generate_nodes.py の流儀 |
| 0 | store ごとの memory 行の正規化（個数・live・予約を分けて出す）と動的 census（b3-census lane の結果） | Fable + lane | — | b3 §2.1 |
| 1 | S1 Symbol 32 B 統一（A-2 を拡張：binder record → transient を同じ room に → routing 廃止） | Opus（record・binder）+ lane（checker 側の生成 54 か所と API） | −120〜−130 / −0.2〜−0.5% | symbol-report §8 |
| 1 | S2 SymbolLinks 分割（永続 12 B / transient inline 16 B） | lane | −62〜−69 / −1.0〜−1.4% | links-routing §7.1 |
| 1 | S3 NodeLinks 2 段表 + routing 除去 + locals bit | lane | 0 / −1.0〜−1.9% | links-routing §7.2, 7.5, 7.6 |
| 2 | S4 Type 24 B + tail、TypeLinks dense | lane | −40〜−60 / −0.5〜−1.3% | tsrs-layouts §3, links-routing §7.4 |
| 2 | S5 Signature 40 + 40 | lane | −55 / 0 ± 0.3% | b3 §3.2, §6 |
| 2 | S6 members 48 + PropertiesView + IndexInfo 20 + 名前つき entry | lane | −60〜−100 / −0.3〜−0.6% | b3 §3.4, audit row 3 |
| 3 | S7 mapper 12 / context 48 / info 56 + recycle | lane | −110〜−170 / 0 ± 0.5% | b3 §3.3–3.7, §6 |
| 3 | S8 list と複製（R18、`to_vec`、TypeListId） | lane | −10〜−20 / −2〜−3% | allocations §8, tsrs R18 −1.6% |
| 4 | S9 文字列 | lane | −60〜−70 / −0.6〜−1.9% | strings §8 |
| 4 | S10 flow 16 B + global id | Opus | −69 / −0.2〜−0.5% | links-routing §7.3 |
| 5 | S11 leaf の解放 | lane | peak −280〜−380 / ≈ 0 | leaf-freeing §0 |
| 5 | 後始末：C-0 step 1 の register（memo 停止で不要、+0.3%）の削除、残る `LinkSlot` 8 B の cold map、`generic_object_flags` を型の bit に | lane | 小 / −0.3% | links-routing §7.7 |

gate（全 step 共通）：変えた crate の test；release build；`scripts/conformance_ts71.py --workers 1 --checkers 4` が main の
4-checker 報告と 15,339 構成一致；`perf11/diffout.py` で 8 project の出力一致；`--extendedDiagnostics` の Types / Symbols /
Instantiations の行が不変；`serial.py` の命令数と peak footprint（同じ run で main と比較）と `TSRS_MEMORY_REPORT` の該当行；
workspace test・one-checker conformance・suites は hosted `ci`。LS / API：suites の一致（symbol handle の変更は S1 の gate）。

**S1 の設計で決めること（Opus の入力 2026-11-04）**：(a) transient は checker ごとの claimed room（id は checker ごとに決定的だが
checker を跨いで dense ではない）→ id の順序を見る箇所（compare_types の tie-break、R29 の順序 id、ambient module の列挙）は raw id
ではなく順序 id を使う；(b) 順序 id は checker ごとの links（id で dense、page は必要時）；(c) declarations は checker ごとの
append-only pool（frozen span）、`clone_symbol` / `merge` は新しい flag bit を mask；(d) transient 専用の links 16 B を record の
room に inline するか（tsrs の形、S2）は S1 と同時に決める。

## 4. 止めるもの・続けるもの

- **止める**：C-0 flow memo（step 2 の branch は残すが PR にしない、step 3 なし、lane 停止）、C-1 inference memo（census 報告は残す）、
  checker file 内の micro な切り口（merge 済み分は残す）。R29 4–7 の修正 lane（`r29bfix`）は S1 に吸収（順序 id を links に置く設計
  で退行の原因そのものが消える）。
- **続ける**：front end（Opus：D1 → S1/A-2。node 種類ごとの直接 attach は計測で +0.7% だったので取り下げ）、B-5 の transaction 削除（3d–3e の lane、4–7：journal が減るほど S2 の
  links 層が単純になる）、storage contract lane（phase 0）、loader lane（A-3）、b3-census lane（phase 0 の入力）、copy-cuts lane
  （結果を S8 の入力にする）。
- **記録**：C-0 の計測（VS Code ±0、ts −10%）と不一致 1 件は台帳と `c0-step2-lane-record.md` に残す。将来 memo を戻すなら新しい
  data 層の上で設計し直す。

## 5. 見込み（正直に）

| 指標（VS Code 1 thread） | 今（PR 5 本着地後） | この計画の予想 | tsrs |
|---|---:|---:|---:|
| 命令数 | ≈ 137 G | ≈ 122〜130 G（−5〜−11%） | 87.3 G（PGO 75） |
| peak footprint | ≈ 3,590 MB | ≈ 2,900（S11 なし）〜 2,600 MB（S11 込み） | 2,021 MB |

memory は tsrs に近づく（1.3×）が、命令数はこの計画の priced 項目だけでは 1.4〜1.5× に留まる。残りの差を埋める材料は、record
が tsrs と同じ形になった **後に** 初めて 1:1 で比べられる「同じ論理の関数の機械語」（`fcmp.py` / `disasm.py`：self time 上位 30 関数の
命令数の比と形の差。audit では name resolution の static body が 4,920 対 1,468 slot など）で、これは phase 2 の終わりに D track として
計測し、結果で続行・目標変更・中止を判断する。判断点：**phase 1–2 の着地時（約 2 週間後）**と **phase 3–5 の着地時**。

## 6. 今週の一手

1. storage contract lane の SPEC に claimed room / frozen span / recycle の primitive を追加（済み：2026-11-04）、lane を起動。
2. record 生成器 lane の SPEC（layout JSON の語彙：record、tail、bit field、span、list id；生成物と assertion；memory 行）。
3. S1 の設計 memo（Opus。D1 着地後）：Symbol 32 B の field、tail、declarations pool、順序 id の置き場、API の handle。lane 用の SPEC は
   その memo から Fable が書く。
4. memory 行の正規化（個数・live・予約）を `TSRS_MEMORY_REPORT` に入れ、台帳の「今」の行を揃える。
5. 台帳に phase 0–1 の予想行を先に書く。
