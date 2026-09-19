from pathlib import Path
import json
rows={
'E-ENTRY':[('emit',123568),('emitWorker',123595),('handleNoEmitOptions',125636),('emitFiles',116530)],
'E-PLAN-SCRIPT':[('getSourceFilesToEmit',16600),('sourceFileMayBeEmitted',16617),('getOutputPathsFor',116373),('forEachEmittedFile',116312)],
'E-RESOLVER-IDENTITY-G':[('getParseTreeNode',11426),('getEmitResolver',47561),('createResolver',88545)],
'E-METADATA-BASE':[('getOriginalNode',11400),('setOriginalNode',25208),('mergeEmitNode',25218),('cloneNode',24436),('propagateChildFlags',25110)],
'E-METADATA-G-CLASS':[('getClassFacts',94410),('transformNamedEvaluation',93950),('getAssignedName',11566),('visitClassDeclarationInNewClassLexicalEnvironment',96971),('getFirstConstructorWithBody',16674),('transformClassMembers',94564)],
'E-CAPTURE-BASE':[('transformES2017',100810)],
'E-CHECKER-FACTS-BASE':[('createResolver',88545),('getEmitResolver',47561)],
'E-NAMES-BASE':[('generateName',120624),('generateNameCached',120633),('makeUniqueName',120741),('makeTempVariableName',120703),('generateNameForNode',120876)],
'E-NAMES-CLASS-G':[('getClassFacts',96844),('visitClassExpressionInNewClassLexicalEnvironment',97049),('createClassTempVar',97056)],
'E-HELPERS-IMPORT-STATE':[('getExternalHelpersModuleName',27603),('createExternalHelpersImportDeclarationIfNeeded',27613),('emitHelpers',117719),('requestEmitHelper',116243)],
'E-PRINTER-G':[('emitParametersForArrow',119983),('emitNodeListItems',120068),('canEmitSimpleArrowHead',119979),('shouldEmitBlockFunctionBodyOnSingleLine',118999),('emitCaseOrDefaultClauseRest',119486),('emitIdentifierName',117149)],
'E-COMMENT-SCOPE-H':[('pipelineEmitWithComments',120978),('emitCommentsBeforeNode',120987),('emitCommentsAfterNode',120995)],
'E-MAPS':[('createTextWriter',16365),('createSourceMapGenerator',92365),('pipelineEmitWithSourceMaps',121277),('emitSourceMapsBeforeNode',121283),('emitSourceMapsAfterNode',121294)],
'E-OUTPUT-FUTURE':[('emitFiles',116530),('writeFile',16644),('writeFileEnsuringDirectories',16663)],
'E-RECOVERY-FACTS':[('nextToken',29502),('parseList',30169),('parseDelimitedList',30428),('parseSemicolon',29770),('parseErrorAtPosition',29467),('abortParsingListOrMoveToNextToken',30356)],
'E-COMMENTS-G':[('emitLeadingComments',121123),('emitTrailingComments',121176),('iterateCommentRanges',8491),('forEachLeadingCommentToEmit',121219),('emitTokenWithComment',118731),('emitDetachedComments',16817),('emitComments',16794)],
'E-COMMENT-PHASES-A36':[('emitSpreadElement',118528),('emitSourceFileWorker',119753),('emitDecoratorsAndModifiers',119846),('emitModifierList',119903)],
'E-COMMENT-ELLIPSIS-A37':[('emitParameter',117855),('emitSpreadAssignment',119536),('emitJsxExpression',119448)],
}
plan=json.loads(Path('/tmp/emitter-delivery-plan-r227.json').read_text());assert set(rows)==set(plan['delegated_rows'])
Path('/tmp/emitter-delivery-owner-selections-r249.json').write_text(json.dumps({'status':'preparation only, actual Opus167 proposed scoped declarations; verify unique source ranges and body digests before use','rows':{k:[{'name':n,'line':line} for n,line in values] for k,values in rows.items()},'adjacent_semantic_correction':{'name':'getReturnTypeOfSignature','line':59810,'scope':'Adjacent semantic correction protected by checker regression and declaration/map complete commands; not a new architecture owner for E-CHECKER-FACTS-BASE.'}},indent=2)+'\n')
s=Path('/tmp/emitter-delivery-owner-identities-r248.mjs').read_text().replace("const names=new Set(Object.values(plan.rows).flat());","""const selected=JSON.parse(fs.readFileSync('/tmp/emitter-delivery-owner-selections-r249.json','utf8'));
const names=new Set([...Object.values(plan.rows).flat(),...Object.values(selected.rows).flat().map(x=>x.name),selected.adjacent_semantic_correction.name]);""")
s=s.replace("fs.writeFileSync('/tmp/emitter-delivery-owner-identities-r248.json',", """function resolve(owner){
 const matches=found[owner.name].filter(x=>x.source_range.start.line===owner.line);
 if(matches.length!==1)throw Error(`ambiguous/missing scoped owner ${owner.name}:${owner.line}`);
 return matches[0];
}
out.selected_rows=Object.fromEntries(Object.entries(selected.rows).map(([row,owners])=>[row,owners.map(resolve)]));
out.adjacent_semantic_correction={...resolve(selected.adjacent_semantic_correction),scope:selected.adjacent_semantic_correction.scope};
fs.writeFileSync('/tmp/emitter-delivery-owner-identities-r249.json',""")
Path('/tmp/emitter-delivery-owner-identities-r249.mjs').write_text(s)
