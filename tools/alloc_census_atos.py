#!/usr/bin/env python3
"""Offline: python3 -I tools/alloc_census_atos.py SIDECAR BINARY > symbols.json.

Use the exact recorded Mach-O and its dSYM. atos receives the executable load
address recorded before work; ASLR never needs disabling. Raw return PCs stay
in the sidecar/map keys; atos looks up PC-4 (the arm64 call instruction).
System image addresses which atos cannot resolve stay raw. This tool executes
atos and LLVM Rust symbol demangling, offline; it never attaches to/runs a compiler process.
"""
import argparse
import json
import pathlib
import subprocess
import tempfile


def symbolize(sidecar, binary, run=subprocess.run):
    with pathlib.Path(sidecar).open() as stream:
        header = json.loads(next(stream))
        if header.get('unwinder') != 'darwin-arm64-frame-pointer-v1':
            raise ValueError('requires Darwin arm64 PCs')
        load = header['load_address']
        if int(load, 16) == 0:
            raise ValueError('missing executable load address')
        pcs = sorted({pc for line in stream for pc in json.loads(line).get('pcs', [])}, key=lambda pc: int(pc, 16))
    if not pcs:
        raise ValueError('no PCs')
    with tempfile.TemporaryDirectory() as directory:
        addresses = pathlib.Path(directory) / 'addresses'
        addresses.write_text(''.join(f'0x{int(pc, 16) - 4:x}\n' for pc in pcs))
        result = run(['atos', '-arch', 'arm64', '-o', str(binary), '-l', load, '-f', str(addresses)],
                     check=True, text=True, capture_output=True)
    frames = result.stdout.splitlines()
    # Apple's atos may leave Rust v0 names mangled. The SDK's LLVM demangler
    # supports them and preserves atos image/offset suffixes, one line per PC.
    if any(frame.startswith(('_R', '_ZN')) for frame in frames):
        result = run(['xcrun', 'llvm-cxxfilt', '--no-strip-underscore'],
                     input='\n'.join(frames) + '\n', check=True, text=True, capture_output=True)
        frames = result.stdout.splitlines()
    if len(frames) != len(pcs):
        raise ValueError('atos output count differs from PC count')
    return dict(exact=True, symbols={pc: frame if not frame.startswith('0x') else pc
                                    for pc, frame in zip(pcs, frames)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sidecar', type=pathlib.Path)
    parser.add_argument('binary', type=pathlib.Path)
    args = parser.parse_args()
    try:
        print(json.dumps(symbolize(args.sidecar, args.binary), indent=2, sort_keys=True))
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(2, f'alloc_census_atos: {error}\n')


if __name__ == '__main__':
    main()
