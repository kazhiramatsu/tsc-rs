from pathlib import Path
import json,hashlib,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');plan=json.loads(Path('/tmp/emitter-delivery-plan-r227.json').read_text())
row_names={
'E-ENTRY':['emit','emitWorker','handleNoEmitOptions','emitFiles'],
'E-PLAN-SCRIPT':['getSourceFilesToEmit','sourceFileMayBeEmitted','getOutputPathsFor','forEachEmittedFile'],
'E-RESOLVER-IDENTITY-G':['getParseTreeNode','getEmitResolver','createResolver'],
'E-METADATA-BASE':['getOriginalNode','setOriginalNode','mergeEmitNode','cloneNode','propagateChildFlags'],
'E-METADATA-G-CLASS':['getClassFacts','transformNamedEvaluation','getAssignedName','visitClassDeclarationInNewClassLexicalEnvironment'],
'E-CAPTURE-BASE':['transformES2017','transformNodes'],
'E-CHECKER-FACTS-BASE':['getEmitResolver','createResolver','getReturnTypeOfSignature'],
'E-NAMES-BASE':['generateName','generateNameCached','generateUniqueName'],
'E-NAMES-CLASS-G':['createClassTempVar','visitClassExpressionInNewClassLexicalEnvironment','getClassFacts'],
'E-HELPERS-IMPORT-STATE':['getExternalHelpersModuleName','createExternalHelpersImportDeclarationIfNeeded','emitHelpers','requestEmitHelper'],
'E-PRINTER-G':['emitParametersForArrow','emitNodeListItems','pipelineEmitWithComments'],
'E-COMMENT-SCOPE-H':['pipelineEmitWithComments','emitNodeWithComments','emitLeadingComments','emitTrailingComments'],
'E-MAPS':['createTextWriter','createSourceMapGenerator','emitNodeWithSourceMap'],
'E-OUTPUT-FUTURE':['emitFiles','writeFile','writeFileEnsuringDirectories'],
'E-RECOVERY-FACTS':['nextToken','parseList','parseDelimitedList','parseSemicolon'],
'E-COMMENTS-G':['pipelineEmitWithComments','emitLeadingComments','emitTrailingComments'],
'E-COMMENT-PHASES-A36':['emitModifiers','emitSpreadElement','emitSourceFileWorker'],
'E-COMMENT-ELLIPSIS-A37':['emitParameter','emitSpreadAssignment','emitJsxExpression'],
}
assert set(row_names)==set(plan['delegated_rows'])
names={n for values in row_names.values() for n in values};found={n:{} for n in names};p=root/'ratchets/h1-owner-inventory.v1.json'
def walk(x):
 if isinstance(x,dict):
  if x.get('name') in names and 'lexical_path' in x and 'body_sha256' in x:
   found[x['name']][x['lexical_path']]={k:x[k] for k in ['id','name','lexical_path','source_range','body_range','body_sha256'] if k in x}
  for value in x.values():walk(value)
 elif isinstance(x,list):
  for value in x:walk(value)
walk(json.loads(p.read_text()))
result={'status':'preparation only; owner graph is prewalk and must be rechecked at final V. Multiple same-name candidates need scoped selection, missing owner names are not invented. No qualification claim.', 'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'inventory_path':str(p.relative_to(root)),'inventory_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'rows':row_names,'candidates':{n:list(v.values()) for n,v in sorted(found.items())}}
Path('/tmp/emitter-delivery-owners-candidates-r247.json').write_text(json.dumps(result,indent=2)+'\n')
for n,v in sorted(found.items()):print(n,len(v))
