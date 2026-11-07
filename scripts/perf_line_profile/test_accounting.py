#!/usr/bin/env python3
import gzip
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("analyze_lines", Path(__file__).with_name("analyze_lines.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class AccountingTests(unittest.TestCase):
    def fixture(self, directory):
        path = directory / "test.json.gz"
        binary = directory / "tsc-rs"
        binary.write_bytes(b"fixture")
        profile = {
            "meta":{"sampleUnits":{"threadCPUDelta":"µs"}},
            "libs":[{"name":"tsc-rs","debugName":"tsc-rs","breakpadId":"1234567890ABCDEF1234567890ABCDEF0","arch":"arm64"}],
            "threads":[{
                "pid":"1","tid":"1","name":"tsc-rs-main",
                "frameTable":{"length":3,"func":[0,1,2],"address":[16,32,48]},
                "funcTable":{"resource":[0,0,0]}, "resourceTable":{"lib":[0]},
                "stackTable":{"prefix":[None,0,1,0],"frame":[0,1,0,2]},
                "samples":{"stack":[0,1,2,3,1,1,None],"threadCPUDelta":[100,200,300,400,0,None,50],
                           "time":[1,2,3,4,1000,1001,1002],"weight":[1,1,1,1,999999,1,1]}
            }]
        }
        with gzip.open(path,"wt") as f: json.dump(profile,f)
        sidecar = {"string_table":["root","child","other"],"data":[{
            "debug_name":"tsc-rs","debug_id":"12345678-90ab-cdef-1234-567890abcdef",
            "known_addresses":[[16,0],[32,1],[48,2]],
            "symbol_table":[{"symbol":0},{"symbol":1},{"symbol":2}]
        }]}
        Path(str(path).replace(".json.gz",".json.syms.json")).write_text(json.dumps(sidecar))
        source = "/private/tmp/tsc-rs-test/crates/checker/src/test.rs"
        # Two inline functions on the same source line must not duplicate line CPU.
        raw = (f"root (in tsc-rs) ({source}:10)\n\n"
               f"child (in tsc-rs) ({source}:20)\n\n"
               f"child (in tsc-rs) ({source}:20)\n"
               f"other (in tsc-rs) ({source}:20)\n")
        return path,binary,raw

    def test_inclusive_self_recursion_zero_wait_and_inline_line(self):
        with tempfile.TemporaryDirectory() as directory:
            path,binary,raw=self.fixture(Path(directory))
            with patch.object(module.subprocess,"check_output",return_value="UUID: 12345678-90AB-CDEF-1234-567890ABCDEF (arm64)\n"), \
                 patch.object(module.subprocess,"run",return_value=subprocess.CompletedProcess([],0,raw,"")):
                result=module.analyze(path,binary)
            c=result["counters_us"]; prefix="crates/checker/src/test.rs"
            self.assertEqual(result["cpu_ms"],1)
            self.assertEqual(c["inclusive_functions"][prefix+" | root"],1000)
            self.assertEqual(c["inclusive_functions"][prefix+" | child"],900)
            self.assertEqual(c["self_functions"][prefix+" | root"],400)
            self.assertEqual(c["self_functions"][prefix+" | child"],600)
            self.assertEqual(c["inclusive_source_lines"][prefix+":20"],900)
            self.assertEqual(c["self_source_lines"][prefix+":20"],600)
            self.assertEqual(result["threads"][0]["zero_cpu_samples"],1)
            self.assertEqual(result["threads"][0]["missing_samples"],2)
            self.assertEqual(sum(c["owner_lines"].values()),1000)
            self.assertEqual(c["caller_edges"][prefix+" | child <- "+prefix+":10 | root"],500)

    def test_reject_mismatched_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            path,binary,_=self.fixture(Path(directory))
            with patch.object(module.subprocess,"check_output",return_value="UUID: 00000000-0000-0000-0000-000000000000 (arm64)\n"):
                with self.assertRaisesRegex(ValueError,"UUID"):
                    module.analyze(path,binary)

    def test_all_threads_contribute(self):
        with tempfile.TemporaryDirectory() as directory:
            path,binary,raw=self.fixture(Path(directory))
            with gzip.open(path,"rt") as f:
                profile=json.load(f)
            other=json.loads(json.dumps(profile["threads"][0]))
            other["tid"]="2"
            other["name"]="checker-worker"
            profile["threads"].append(other)
            with gzip.open(path,"wt") as f:
                json.dump(profile,f)
            with patch.object(module.subprocess,"check_output",return_value="UUID: 12345678-90AB-CDEF-1234-567890ABCDEF (arm64)\n"), \
                 patch.object(module.subprocess,"run",return_value=subprocess.CompletedProcess([],0,raw,"")):
                result=module.analyze(path,binary)
            self.assertEqual(result["cpu_ms"],2)
            self.assertEqual(len(result["threads"]),2)
            self.assertEqual(result["counters_us"]["inclusive_functions"]["crates/checker/src/test.rs | root"],2000)

if __name__=="__main__":
    unittest.main()
