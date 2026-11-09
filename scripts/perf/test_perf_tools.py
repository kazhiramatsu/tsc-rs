#!/usr/bin/env python3
"""Smoke checks for scripts/perf: every Python tool compiles, the shell scripts parse, the roots come from the environment,
and lowmode.py summarizes a record by its low mode. Run: python3 scripts/perf/test_perf_tools.py"""
import json
import os
import py_compile
import re
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


class PerfTools(unittest.TestCase):
    def test_python_tools_compile(self):
        for name in sorted(os.listdir(HERE)):
            if name.endswith('.py'):
                py_compile.compile(os.path.join(HERE, name), doraise=True)

    def test_shell_scripts_parse(self):
        for name in ['build-patches.sh', 'measure-patches.sh']:
            subprocess.run(['zsh', '-n', os.path.join(HERE, name)], check=True)

    def test_roots_come_from_the_environment(self):
        text = {name: open(os.path.join(HERE, name)).read() for name in ['binprov.py', 'serial.py', 'diffout.py',
                                                                           'build-patches.sh', 'measure-patches.sh']}
        self.assertIn("os.environ.get('TSRS_REPO')", text['binprov.py'])
        self.assertIn("os.environ.get('TSRS_PERF_ROOT')", text['binprov.py'])
        for name in ['serial.py', 'diffout.py']:
            self.assertIn("os.environ.get('TSRS_BENCH_ROOT'", text[name])
        for name in ['build-patches.sh', 'measure-patches.sh']:
            self.assertIn('${TSRS_REPO:-', text[name])
            self.assertIn('${TSRS_PERF_ROOT:-', text[name])
        # No machine-specific absolute paths remain in the tools.
        for name, body in text.items():
            self.assertNotRegex(body, re.compile(r'/Users/[a-z]+/'), name)

    def run_lowmode(self, rows):
        with tempfile.NamedTemporaryFile('w', suffix='.jsonl', delete=False) as handle:
            for label, counts in rows:
                for count in counts:
                    handle.write(json.dumps(dict(case='x/noemit', label=label, instr=round(count * 10**9))) + '\n')
            path = handle.name
        try:
            return subprocess.run([sys.executable, os.path.join(HERE, 'lowmode.py'), path],
                                  capture_output=True, text=True, check=True).stdout
        finally:
            os.unlink(path)

    def test_lowmode_summary(self):
        out = self.run_lowmode([('base', [100, 100, 101, 108, 100]), ('cand', [99, 99, 99, 107, 99])])
        self.assertIn('-1.00%', out)  # low mode 100 -> 99; the high-mode rounds (108, 107) do not move it
        self.assertNotIn('WARN', out)

    def test_lowmode_reports_the_lowest_cluster_and_flags_a_thin_one(self):
        # 7 of 9 base rounds sit in a mode 0.86 % up: the lower-half median would land there and report the
        # candidate at -0.83 %; the lowest cluster (2 rounds) reports 0.00 % and is flagged for more rounds.
        high = [100.86, 100.87, 100.88, 100.86, 100.87, 100.88, 100.89]
        out = self.run_lowmode([('base', [100.00, 100.02] + high), ('cand', [100.02] * 9)])
        self.assertIn('+0.00%', out)
        self.assertIn('2/9', out)
        self.assertIn('WARN low cluster below 3 rounds: x/noemit base (2/9)', out)


if __name__ == '__main__':
    unittest.main()
