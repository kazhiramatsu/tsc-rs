#!/usr/bin/env python3
"""Generate a local, standalone, inline-aware CPU/source report from samply analyses."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--source-root", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("analyses", nargs="+", type=Path)
args = parser.parse_args()
source_commit = subprocess.check_output(["git", "-C", str(args.source_root), "rev-parse", "HEAD"], text=True).strip()
profiles = []
sources = {}
for path in args.analyses:
    profile = json.loads(path.read_text())
    profile["label"] = path.name.replace(".analysis.json", "")
    profile["iterations"] = len({x["pid"] for x in profile["threads"] if x["name"] == "tsc-rs-main"}) or 1
    profiles.append(profile)
    for key in profile["counters_us"]["inclusive_lines"]:
        file = key.split(" | ", 1)[0].rsplit(":", 1)[0]
        if not file.startswith("crates/") or file in sources:
            continue
        source = args.source_root / file
        if source.is_file():
            data = source.read_bytes()
            sources[file] = {"text":data.decode(), "sha256":hashlib.sha256(data).hexdigest()}
payload = json.dumps({"profiles":profiles, "sources":sources,
                     "source_commit":source_commit,
                     "method":"samply 1000 Hz + per-address atos inline/line resolution, CPU-delta weighted",
                     "note":"optimized release + line-tables-only; fat LTO, codegen-units=1; profiling at nice 20"},
                     ensure_ascii=False).replace("<", "\\u003c")
template = r"""<!doctype html>
<html lang="ja"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>tsc-rs · 行別 CPU プロファイル</title>
<style>
:root{color-scheme:light;--ink:#172638;--muted:#607085;--line:#dbe2e9;--accent:#186a68}
*{box-sizing:border-box}body{margin:0;font:14px/1.5 system-ui,sans-serif;color:var(--ink);background:#f3f5f7}
header{padding:24px 28px 16px;background:#122c40;color:#fff}h1{font-size:23px;margin:0 0 6px}header p{margin:4px 0;color:#c7d7e4}a{color:#087772}
.controls{display:flex;gap:12px;flex-wrap:wrap;padding:14px 28px;background:white;border-bottom:1px solid var(--line);align-items:end}
label{display:flex;flex-direction:column;gap:3px;color:var(--muted);font-size:12px}
select,input,button{font:14px system-ui;padding:8px;border:1px solid #bcc9d4;border-radius:5px;background:white;color:var(--ink)}
input{min-width:230px}button{cursor:pointer}button:hover{background:#e7f2f0}
.stats{display:flex;gap:24px;padding:12px 28px;color:var(--muted);flex-wrap:wrap}.stats strong{color:var(--ink);font-size:18px}
main{display:grid;grid-template-columns:minmax(350px,38%) minmax(0,62%);gap:14px;padding:0 18px 18px}
.panel{background:white;border:1px solid var(--line);border-radius:8px;overflow:hidden}.panel h2{font-size:15px;margin:0;padding:12px 14px;border-bottom:1px solid var(--line)}
#functions{height:72vh;overflow:auto}.function{display:block;padding:10px 14px;border-bottom:1px solid #edf0f3;cursor:pointer}.function:hover{outline:1px solid #589b99;outline-offset:-1px}.function.active{border-left:4px solid var(--accent)}
.fn{font-family:ui-monospace,monospace;font-size:12px;overflow-wrap:anywhere}.file{font-size:11px;color:var(--muted);overflow-wrap:anywhere}.numbers{display:flex;gap:12px;margin:3px 0;font-size:12px;font-variant-numeric:tabular-nums}
#selection{padding:12px 15px}.description{font-size:12px;color:var(--muted);margin:6px 0}
#source{overflow:auto;max-height:52vh;font:12px/1.7 ui-monospace,SFMono-Regular,monospace}
.code-row{display:grid;grid-template-columns:55px 85px 85px max-content;min-width:max-content}.code-row.selected{outline:2px solid #208b87;outline-offset:-2px}
.gutter,.metric{text-align:right;padding:0 10px;color:#607085;background:#f6f8fa;user-select:none}.metric{color:#3d566c;font-variant-numeric:tabular-nums}.code{white-space:pre;padding:0 14px}
.code-head{position:sticky;top:0;background:#e9eef3;z-index:1;font-family:system-ui}.code-head>div{padding:6px 10px}
nav{display:flex;gap:8px;padding:8px 12px;border-block:1px solid var(--line)}
.relations{display:grid;grid-template-columns:1fr 1fr;gap:12px;padding:12px}.relations h3{font-size:13px;margin:0 0 6px}.relation{display:block;width:100%;text-align:left;margin:4px 0;font-size:11px;padding:6px;overflow-wrap:anywhere}
footer{padding:0 28px 24px;font-size:12px;color:var(--muted)}details{margin:10px 0}#threadList{white-space:pre-wrap;font:12px ui-monospace,monospace}
@media(max-width:950px){main{display:block}.panel{margin-bottom:15px}#functions{height:35vh}.controls{padding:12px}.relations{grid-template-columns:1fr}}
</style>
<header><h1>tsc-rs · 関数とソース行の CPU プロファイル</h1><p>呼び出し先を含む時間から、データ構造の操作までたどる。</p><p>時間はサンプリングによる推定 CPU 時間です。実行回数・実時間の直接計測とは区別しています。</p></header>
<div class="controls">
<label>プロファイル<select id="profile"></select></label>
<label>順位の基準<select id="mode"><option value="inclusive_functions">合計：呼び出し先を含む</option><option value="self_functions">自身：その関数だけ</option><option value="owner_functions">操作の費用：標準ライブラリ等を帰属</option></select></label>
<label>関数・ファイルを検索<input id="search" placeholder="例：instantiate / member_table"></label>
<label>対象<select id="scope"><option value="crates/">プロジェクト内</option><option value="crates/checker/">checker</option><option value="">すべて</option></select></label>
<label>比較対象（CPU / 実行）<select id="comparison"><option value="">なし</option></select></label>
</div>
<div class="stats" id="stats"></div>
<main><section class="panel"><h2 id="listTitle">関数</h2><div id="functions"></div></section>
<section class="panel"><div id="selection"></div><nav><button id="previous">前の負荷行</button><button id="next">次の負荷行</button><button id="whole">ファイル全体 / 周辺</button></nav><div id="source"></div>
<div class="relations"><section><h3>呼び出し先（合計時間）</h3><div id="children"></div></section><section><h3>呼び出し元と呼び出し行</h3><div id="parents"></div></section></div></section></main>
<footer><details><summary>測定方法・読み方・ソースの出所</summary>
<p id="method"></p><p>合計時間（inclusive）は、呼び出し先で使った CPU 時間を含みます。関数自身の時間（self）は呼び出し先を除きます。「操作の費用」は各サンプルを最も内側のプロジェクト関数に帰属させ、標準ライブラリや allocator 内の仕事を含めます。</p>
<p>同一サンプル内で再帰した同じ関数・同じ行は一度だけ数えます。親子の合計時間は重なるため、表全体の時間を足さないでください。呼び出し元別の値も、再帰経路が重なる場合は足し合わせられません。</p>
<p>行番号はサンプルの各命令アドレスを atos で解決しています。最適化による命令の移動・融合や、サンプル間の処理の変化により、行への帰属には限界があります。サンプル数は呼び出し回数ではありません。待機中の CPU 増分ゼロは CPU 時間に加えません。プロファイラ付きの経過時間で高速化を判定しません。</p>
<p>比較欄はプロファイル内の実行数で割った CPU 費用の差です。同一 workload・設定の比較に使います。ソース行が変更された版では関数名から照合してください。この HTML はソースを内包し、ネットワークへ送信しません。</p>
<p id="identity"></p><div id="threadList"></div></details></footer>
<script id="data" type="application/json">__PAYLOAD__</script>
<script>
"use strict";
const DATA=JSON.parse(document.getElementById("data").textContent);
const $=id=>document.getElementById(id);
let current=0,chosen=null,center=1,whole=false;
const profiles=DATA.profiles;
const split=key=>{const at=key.indexOf(" | ");let file=key.slice(0,at),name=key.slice(at+3),m=file.match(/:(\d+)$/);return {file:m?file.slice(0,-m[0].length):file,line:m?+m[1]:null,name};};
const fnKey=key=>{const x=split(key);return x.file+" | "+x.name;};
const fmt=x=>(x/1000).toFixed(x>=100000?1:2)+" ms";
const elem=(tag,text,cls)=>{let e=document.createElement(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;};
const C=()=>profiles[current].counters_us;
for(let i=0;i<profiles.length;i++){let o=elem("option",profiles[i].label);o.value=i;$("profile").append(o);let b=o.cloneNode(true);$("comparison").append(b);}
$("method").textContent=DATA.method+" · "+DATA.note;
$("identity").textContent="ソース commit: "+DATA.source_commit;
function values(category){return C()[category]||{};}
function renderList(){
 let p=profiles[current],mode=$("mode").value,query=$("search").value.toLowerCase(),scope=$("scope").value;
 let rows=Object.entries(values(mode)).filter(([k])=>k.startsWith(scope)&&k.toLowerCase().includes(query)).sort((a,b)=>b[1]-a[1]);
 let comparison=$("comparison").value,base=comparison!==""?profiles[+comparison]:null;
 $("listTitle").textContent="関数 · "+rows.length+" 件（上位 300 件を表示）";
 $("functions").replaceChildren();
 for(let [key,value] of rows.slice(0,300)){
  let x=split(key),r=elem("div",undefined,"function"+(key===chosen?" active":""));
  r.style.background="linear-gradient(90deg,rgba(34,148,132,.10) "+Math.min(100,value/(p.cpu_ms*10))+"%,transparent 0)";
  r.append(elem("div",x.name,"fn"),elem("div",x.file,"file"));
  let n=elem("div",undefined,"numbers");
  n.append(elem("strong",fmt(value/p.iterations)+" / 実行"),elem("span",(value/(p.cpu_ms*10)).toFixed(2)+"%"));
  if(mode!=="self_functions")n.append(elem("span","自身 "+fmt((values("self_functions")[key]||0)/p.iterations)));
  if(base){let delta=value/p.iterations-((base.counters_us[mode]||{})[key]||0)/base.iterations;n.append(elem("span","差 "+(delta>=0?"+":"−")+fmt(Math.abs(delta))));}
  r.append(n);r.onclick=()=>select(key);$("functions").append(r);
 }
 let sourcePct=p.self_line_cpu_percent.toFixed(1);
 $("stats").replaceChildren(elem("span","推定 CPU 合計 "+(p.cpu_ms/1000).toFixed(3)+" s"),elem("span","実行数 "+p.iterations),elem("span","自身の行情報を解決 "+sourcePct+"%"),elem("span","ゼロ CPU の待機を除外"));
 $("threadList").textContent=p.threads.map(t=>t.name+" [pid "+t.pid+", tid "+t.tid+"]: "+t.cpu_ms.toFixed(2)+" ms; active samples "+t.active_samples).join("\n");
 if(!chosen&&rows.length)select(rows[0][0]); else if(chosen)renderSelection();
}
function select(key,line){chosen=key;whole=false;let candidates=Object.entries(values("inclusive_lines")).filter(([k])=>fnKey(k)===key&&split(k).line).sort((a,b)=>b[1]-a[1]);center=line||(candidates.length?split(candidates[0][0]).line:1);renderList();}
function lineCosts(file,category){let result={},sourceCategory=category==="inclusive_lines"?"inclusive_source_lines":category==="self_lines"?"self_source_lines":null;if(sourceCategory&&C()[sourceCategory]){for(let[k,v]of Object.entries(values(sourceCategory))){let m=k.match(/:(\d+)$/);if(m&&k.slice(0,-m[0].length)===file)result[+m[1]]=v;}return result;}for(let[k,v]of Object.entries(values(category))){let x=split(k);if(x.file===file&&x.line)result[x.line]=(result[x.line]||0)+v;}return result;}
function renderSelection(){
 let p=profiles[current],x=split(chosen),source=DATA.sources[x.file];
 $("selection").replaceChildren(elem("div",x.name,"fn"),elem("div",x.file,"file"),elem("div","合計 "+fmt((values("inclusive_functions")[chosen]||0)/p.iterations)+" / 自身 "+fmt((values("self_functions")[chosen]||0)/p.iterations)+"（1 実行あたり）","numbers"),elem("div","行の合計には呼び出し先を含みます。行をクリックすると周辺を表示します。","description"));
 let out=$("source");out.replaceChildren();
 if(!source){out.append(elem("p","このフレームのソースは内包していません。呼び出し元から確認できます。"));}
 else{
  let lines=source.text.split("\n"),incl=lineCosts(x.file,"inclusive_lines"),self=lineCosts(x.file,"self_lines");
  let hotMax=Math.max(1,...Object.values(incl)),lo=whole?1:Math.max(1,center-12),hi=whole?lines.length:Math.min(lines.length,center+35);
  let header=elem("div",undefined,"code-row code-head");for(let s of ["行","合計 / 実行","自身 / 実行","ソース"])header.append(elem("div",s));out.append(header);
  if(lo>1)out.append(elem("div","… "+(lo-1)+" 行省略","description"));
  let fragment=document.createDocumentFragment();
  for(let n=lo;n<=hi;n++){
   let r=elem("div",undefined,"code-row"+(n===center?" selected":"")),iv=incl[n]||0,sv=self[n]||0;
   r.append(elem("div",String(n),"gutter"),elem("div",iv?fmt(iv/p.iterations):"","metric"),elem("div",sv?fmt(sv/p.iterations):"","metric"));
   let code=elem("div",lines[n-1],"code");if(iv)code.style.background="rgba(245,135,65,"+(0.07+0.45*Math.sqrt(iv/hotMax))+")";r.append(code);r.onclick=()=>{center=n;renderSelection();};fragment.append(r);
  }out.append(fragment);
  if(hi<lines.length)out.append(elem("div","… "+(lines.length-hi)+" 行省略","description"));
 }
 renderRelations();
}
function renderRelations(){
 let children=[],parents=[],p=profiles[current];
 for(let[key,value]of Object.entries(values("caller_edges"))){let at=key.indexOf(" <- "),callee=key.slice(0,at),caller=key.slice(at+4);if(fnKey(caller)===chosen)children.push([callee,value,split(caller).line]);if(callee===chosen)parents.push([fnKey(caller),value,split(caller).line]);}
 for(let[id,rows]of [["children",children],["parents",parents]]){
  let box=$(id);box.replaceChildren();rows.sort((a,b)=>b[1]-a[1]);
  if(!rows.length)box.append(elem("p","記録された関係なし","description"));
  for(let[key,value,line]of rows.slice(0,18)){let x=split(key),r=elem("button",fmt(value/p.iterations)+" · "+x.name+"\n"+x.file+(line?" · 呼び出し行 "+line:""),"relation");r.onclick=()=>select(key,id==="parents"?line:undefined);box.append(r);}
 }
}
$("profile").onchange=()=>{current=+$("profile").value;chosen=null;renderList();};
for(let id of ["mode","scope","comparison"])$(id).onchange=renderList;
$("search").oninput=renderList;
$("whole").onclick=()=>{whole=!whole;renderSelection();};
function jump(direction){if(!chosen)return;let file=split(chosen).file;let hot=Object.keys(lineCosts(file,"inclusive_lines")).map(Number).sort((a,b)=>a-b);let next=direction>0?hot.find(x=>x>center):hot.filter(x=>x<center).pop();if(next){center=next;whole=false;renderSelection();}}
$("previous").onclick=()=>jump(-1);$("next").onclick=()=>jump(1);
renderList();
</script></html>"""
args.output.write_text(template.replace("__PAYLOAD__", payload))
print(json.dumps({"html":str(args.output), "profiles":len(profiles), "source_files":len(sources), "bytes":args.output.stat().st_size}))
