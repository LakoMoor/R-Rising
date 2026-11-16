#!/usr/bin/env python3
"""Replay two leaf functions' actual original x86-64 bytes using clang + Rosetta.

No original code or extracted executable is retained in the repository. This is
an isolated function oracle, not execution of the original game or CLR.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile


def code_at_rva(image, rva, size):
    pe = struct.unpack_from('<I', image, 0x3c)[0]
    assert image[pe:pe+4] == b'PE\0\0', 'not a PE image'
    count, optional_size = struct.unpack_from('<H', image, pe+6)[0], struct.unpack_from('<H', image, pe+20)[0]
    sections = pe + 24 + optional_size
    for i in range(count):
        header = sections + i * 40
        virtual_size, address, raw_size, raw_at = struct.unpack_from('<IIII', image, header+8)
        if address <= rva and rva + size <= address + min(virtual_size, raw_size):
            return image[raw_at+rva-address:raw_at+rva-address+size]
    raise ValueError(f'RVA {rva:x} is not file backed')


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=root.parent/'GameAssembly.dll')
    parser.add_argument('--output', type=Path, default=root/'tests/golden/type_flags.tsv')
    args = parser.parse_args()
    image = args.binary.read_bytes()
    digest = hashlib.sha256(image).hexdigest()
    reference = json.loads((root/'reference.lock.json').read_text())
    expected = next(f['sha256'] for f in reference['files'] if f['path'] == 'GameAssembly.dll')
    if digest != expected:
        raise SystemExit('Reference build mismatch; refuse to capture against another binary')
    assembly = ['.text']
    for name, rva in [('original_refs', 0x8fcda80), ('original_serializable', 0x8fcda90)]:
        code = code_at_rva(image, rva, 10)
        if code[-1] != 0xc3:
            raise SystemExit('Expected a bounded leaf function ending in RET')
        assembly += [f'.globl _{name}', f'_{name}:', '.byte '+','.join(hex(b) for b in code)]
    c_source = '''#include <stdint.h>
#include <stdio.h>
extern uint8_t original_refs(const uint32_t*) __attribute__((ms_abi));
extern uint8_t original_serializable(const uint32_t*) __attribute__((ms_abi));
int main(void) {
 uint32_t state=0x925fe11u;
 for(int i=0;i<1024;i++) {
  uint32_t value;
  if(i<4) value=1u | ((uint32_t)i << 17);
  else {state ^= state << 13; state ^= state >> 17; state ^= state << 5; value=state;}
  printf("%08x\\t%u\\t%u\\n",value,(unsigned)original_refs(&value),(unsigned)original_serializable(&value));
 }
 return 0;
}
'''
    with tempfile.TemporaryDirectory(prefix='rrising-original-oracle-') as scratch:
        scratch = Path(scratch)
        (scratch/'original.S').write_text('\n'.join(assembly)+'\n')
        (scratch/'driver.c').write_text(c_source)
        subprocess.run(['/usr/bin/clang', '-arch', 'x86_64', '-O2', str(scratch/'driver.c'), str(scratch/'original.S'), '-o', str(scratch/'oracle')], check=True)
        result = subprocess.run(['/usr/bin/arch', '-x86_64', str(scratch/'oracle')], check=True, text=True, capture_output=True, timeout=10)
    rows = result.stdout.splitlines()
    if len(rows) != 1024:
        raise SystemExit('Incomplete oracle output')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(f'# GameAssembly.dll sha256={digest}\n# Actual leaf bytes at RVA 0x8fcda80 and 0x8fcda90, x86_64/Rosetta.\n# flags_hex\\thas_entity_references\\tis_chunk_serializable\n'+result.stdout)
    print(f'Captured {len(rows)} original-function inputs -> {args.output}')

if __name__ == '__main__':
    main()
