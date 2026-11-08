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

    def test_lowmode_summary(self):
        with tempfile.NamedTemporaryFile('w', suffix='.jsonl', delete=False) as handle:
            for label, counts in [('base', [100, 100, 101, 108, 100]), ('cand', [99, 99, 99, 107, 99])]:
                for count in counts:
                    handle.write(json.dumps(dict(case='x/noemit', label=label, instr=count * 10**9)) + '\n')
            path = handle.name
        try:
            out = subprocess.run([sys.executable, os.path.join(HERE, 'lowmode.py'), path],
                                 capture_output=True, text=True, check=True)
        finally:
            os.unlink(path)
        self.assertIn('-1.00%', out.stdout)  # low mode 100 -> 99; the high-mode rounds (108, 107) do not move it


if __name__ == '__main__':
    unittest.main()
