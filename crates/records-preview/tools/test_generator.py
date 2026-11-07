#!/usr/bin/env python3
"""Offline generator/source/Go audit and accounting regressions. No Cargo subprocesses."""
import importlib.util
import json
import re
from pathlib import Path
import subprocess
import sys
import unittest
from fractions import Fraction

sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[3]
PREVIEW=ROOT/'crates/records-preview'


def module(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    result=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


gen=module('gen2',ROOT/'scripts/generate_records.py')
report=module('memory_report',PREVIEW/'tools/memory_report.py')


class GeneratorTests(unittest.TestCase):
    def test_determinism_fresh_generation_and_check(self):
        first=gen.outputs()
        self.assertEqual(len(first),7)
        self.assertEqual(first,gen.outputs())
        for path,text in first.items():
            self.assertEqual(path.read_text(),text,path)
        result=subprocess.run([sys.executable,str(ROOT/'scripts/generate_records.py'),'--check'],capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)

    def test_transcribed_field_counts_include_multiline_types(self):
        records,_,_=gen.load()
        for name,count in [('Symbol',6),('SymbolExtras',7),('Type',5),('Signature',18),('InferenceInfo',8),('InferenceContext',12),('NodeLinksCold',18),('SymbolLinksCold',41),('TypeLinksCold',42)]:
            self.assertEqual(len(records['sources'][name]['fields']),count,name)
        for source in records['sources'].values():
            for field in source.get('fields',[]):
                self.assertTrue(field['placement'])
                self.assertNotIn('see fieldPlacements',field['placement'])

    def test_three_manually_audited_go_embedding_paths(self):
        kinds=gen.load()[2]
        derived=gen.binder_fields(kinds)['kinds']
        # Independent hand audit of ast_generated.go:621/1989 and ast.go:2446.
        expected={
            'Identifier':{'flow':'FlowNodeBase'},
            'FunctionDeclaration':{'flow':'FlowNodeBase','symbol':'DeclarationBase','local_symbol':'ExportableBase','locals':'LocalsContainerBase','next_container':'LocalsContainerBase','end_flow':'BodyBase','return_flow':'FunctionDeclaration'},
            'SourceFile':{'symbol':'DeclarationBase','locals':'LocalsContainerBase','next_container':'LocalsContainerBase','common_js_module_indicator':'SourceFile','external_module_indicator':'SourceFile'},
        }
        for kind,facts in expected.items():
            self.assertEqual({f['field']:f['goStruct'] for f in derived[kind]['fields']},facts)
        function=next(f for f in derived['FunctionDeclaration']['fields'] if f['field']=='locals')
        self.assertEqual(function['embeddingPath'],['FunctionDeclaration','FunctionLikeWithBodyBase','FunctionLikeBase','LocalsContainerBase'])

    def test_type_ceiling_decisions_are_per_kind_and_keep_hot_links(self):
        records=gen.load()[0]
        for kind,r in records['types'].items():
            full=24+8*((len(r['payload'])+len(r['links'])+1)//2)
            self.assertEqual(full,r['fullInlineBytes'])
            self.assertEqual(r['outlineLinks'],full>64)
            self.assertLessEqual(r['bytes'],64)
            self.assertNotIn('resolved_members',r['outlineFields'])
        self.assertEqual({k for k,r in records['types'].items() if r['outlineFields']},{'Mapped','Conditional'})

    def test_report_arithmetic_and_missing_population(self):
        _,counts,_,_,_,_,totals=report.calculate()
        self.assertEqual(totals['header'],Fraction(counts['total']*24*1033,1000))
        self.assertEqual(totals['listHeader'],3_119_430*16)
        self.assertEqual(totals['listItems'],5_407_345*4)
        self.assertEqual(totals['listPaddingMin']%8,totals['listPaddingMax']%8)
        self.assertIsNone(totals['populations'])
        self.assertEqual(report.render(),(PREVIEW/'REPORT.md').read_text())
        with self.assertRaises(ValueError):
            report.calculate({'Object':totals['typeCount']})

    def test_rare_call_pair_fails_joint_occupancy_gate(self):
        counts=json.loads((PREVIEW/'inputs/vscode-counts.json').read_text())['kinds']['CallExpression']
        fields=next(k['fields'] for k in json.loads(gen.v1.SCHEMA.read_text())['nodes'] if k['kindName']=='CallExpression')
        indexes=[i for i,f in enumerate(fields) if f['name'] in ('questionDotToken','typeArguments')]
        joint=sum(n for mask,n in counts['masks'].items() if any(mask[i]=='1' for i in indexes))
        self.assertEqual(joint,16_951)
        self.assertGreater(Fraction(joint,counts['count']),Fraction(1,100))

    def test_rust_writer_maps_and_sites_are_all_in_matrix(self):
        matrix=json.loads((PREVIEW/'binder-fields.derived.json').read_text())['rustWriters']
        maps=matrix['maps']
        expected_maps={'node_symbol','node_local_symbol','locals','next_container','node_flow',
            'node_end_flow','node_return_flow','node_flow_when_true','node_flow_when_false',
            'possibly_exhaustive','node_fallthrough_flow','node_flags_mut'}
        self.assertEqual(set(maps),expected_maps)
        # Independent scan: do not use binder_census.inventory or its audit list.
        # Includes multiline receivers, destructured aliases, entry insertion,
        # mutable table contents and relocation, not only .insert calls.
        pattern=r'\b('+'|'.join(expected_maps)+r')\s*\.\s*(insert|entry|remove|get_mut|values_mut|iter_mut|set|set_has_locals|extend|clear|retain|drain)\s*\('
        actual=set()
        for file in ('bind','declare','containers','flow','node_util'):
            path=f'crates/binder/src/{file}.rs'
            source=(ROOT/path).read_text()
            source=re.sub(r'//[^\n]*',lambda m:' '*len(m[0]),source)
            for m in re.finditer(pattern,source):
                actual.add((m[1],path,source[:m.start()].count('\n')+1,m[2]))
        recorded={(name,w['source'],w['line'],w['method']) for name,entry in maps.items() for w in entry['writers']}
        self.assertEqual(actual,recorded)
        self.assertEqual(len(actual),32)
        self.assertEqual(matrix['writerCount'],32)
        for entry in maps.values():
            self.assertTrue(entry['placement'])
            for writer in entry['writers']:
                self.assertTrue(writer['kinds'])
                self.assertTrue(writer['why'])
        for name,missing in [('node_end_flow',['SourceFile']),('possibly_exhaustive',['SwitchStatement']),
                             ('node_flow_when_true',['ConditionalExpression']),('node_flow_when_false',['ConditionalExpression'])]:
            self.assertEqual(maps[name]['status'],'port-specific')
            self.assertEqual(maps[name]['missingTailKinds'],missing)
            self.assertIn('Retain the Rust per-file map',maps[name]['placement'])
        flow=maps['node_flow']['writers']
        self.assertEqual(next(w for w in flow if w['line']==125)['kinds'],['Identifier','ThisKeyword'])
        self.assertEqual(next(w for w in flow if w['line']==1057)['kinds'],'any')
        self.assertEqual(maps['node_flow_when_true']['writers'][0]['kinds'],'any')
        # A non-returning data match must not be mistaken for a dominating guard.
        self.assertEqual(maps['possibly_exhaustive']['writers'][0]['kinds'],'any')

    def test_population_contract_and_cold_occupancy(self):
        records=gen.load()[0]
        totals=report.calculate()[-1]
        pop=dict.fromkeys(records['types'],0)
        pop.update(Object=totals['typeCount']-5,Mapped=3,Conditional=2)
        report.calculate(pop)  # Original 20-key format remains supported.
        complete=dict(pop,mapped_cold_materialized=2,conditional_cold_materialized=1)
        report.calculate(complete)
        common=sum(pop[k]*r['bytes'] for k,r in records['types'].items())
        self.assertIn(f'complete modeled Type record bytes: **{common+48:,}**',report.render(complete))
        for invalid in [dict(pop,mapped_cold_materialized=1),
                        dict(complete,mapped_cold_materialized=4),
                        dict(complete,conditional_cold_materialized=-1),
                        dict(complete,conditional_cold_materialized=True),
                        dict(complete,unknown=0),dict(complete,Object=0)]:
            with self.assertRaises(ValueError):
                report.calculate(invalid)

    def test_owned_decode_load_table_reads_generated_bodies(self):
        counts=json.loads((PREVIEW/'inputs/vscode-counts.json').read_text())
        rows=report.decode_load_rows(counts)
        self.assertEqual([(r['kind'],r['dataLoads']) for r in rows],[
            ('Identifier',1),('PropertyAccessExpression',3),('CallExpression',4),
            ('StringLiteral',1),('PropertyAssignment',5),('ExpressionStatement',1),
            ('TypeReference',2),('Block',1),('VariableDeclaration',4),('VariableDeclarationList',1)])
        for row in rows:
            self.assertEqual((row['tailLoads'],row['tailDebugHeaderLoads']),(0,1))
        source=(PREVIEW/'src/nodes.rs').read_text()
        body,_=report.method_body(source,'ExportAssignment','data')
        # Scalar-array construction reads binder words too, despite optimizer DCE.
        self.assertIn('self.tail.symbol',body)
        self.assertEqual(len(re.findall(r'self\.tail\.\w+',body)),7)


if __name__=='__main__':
    unittest.main(verbosity=2)
