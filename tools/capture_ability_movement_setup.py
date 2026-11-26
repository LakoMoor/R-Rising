#!/usr/bin/env python3
"""Replay the bounded Movement setup instruction slice, not its ECS predicate."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
from oracle_replay import ROOT, code_at_rva

path = Path(os.environ.get('VRISING_SERVER_ASSEMBLY', '/Users/lakomoor/Library/Application Support/Steam/steamapps/common/VRising/VRising_Server/GameAssembly.dll'))
image = path.read_bytes()
digest = hashlib.sha256(image).hexdigest()
if digest != '41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7':
    raise SystemExit('server assembly digest mismatch')
parts = [(0x631b292, 11), (0x631b2a5, 0x33), (0x631b2dd, 16), (0x60780b0, 45)]
chunks = [code_at_rva(image, address, size) for address, size in parts]
expected = ['c6436400c743600000803f', 'f30f1046048b0689434cf30f1143500f57c90f110b0f114b100f114b200f114b30f30f104350f30f104b4c0f14c866480f7ec9', '84c0740c8b434c8943548b4350894358', '48895424100f57c948894c2410f30f104424100f2ec17a127510f30f104424140f2ec17a05750332c0c3b001c3']
if [x.hex() for x in chunks] != expected:
    raise SystemExit('unexpected original instruction bytes')
emit = lambda x: '.byte ' + ','.join(hex(b) for b in x)
assembly = '''.text
.globl _setup
_setup:
pushq %rbx
pushq %rsi
subq $40, %rsp
movq %rcx, %rbx
movq %rdx, %rsi
''' + emit(chunks[0]) + '\n' + emit(chunks[1]) + '''
callq _nonzero
''' + emit(chunks[2]) + '''
addq $40, %rsp
popq %rsi
popq %rbx
retq
_nonzero:
''' + emit(chunks[3]) + '\n'
driver = r'''#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define MS __attribute__((ms_abi))
void setup(void*,void*) MS;
int main(void) {
  uint32_t v[]={0,0x80000000,0x3f800000,0xbf000000,0x7fc01234,0x7f800000};
  for(int i=0;i<6;i++)for(int j=0;j<6;j++){
    uint8_t m[104];memset(m,0x5a,sizeof m);uint32_t input[]={v[i],v[j]};setup(m,input);
    printf("%08x\t%08x\t",v[i],v[j]);for(int k=0;k<104;k++)printf("%02x",m[k]);puts("");
  }return 0;
}'''
with tempfile.TemporaryDirectory(prefix='rrising-movement-setup-') as tmp:
    tmp = Path(tmp)
    (tmp/'slice.S').write_text(assembly)
    (tmp/'driver.c').write_text(driver)
    subprocess.run(['/usr/bin/clang','-arch','x86_64','-O1',str(tmp/'driver.c'),str(tmp/'slice.S'),'-o',str(tmp/'replay')],check=True)
    rows = subprocess.run(['/usr/bin/arch','-x86_64',str(tmp/'replay')],check=True,capture_output=True,text=True).stdout.splitlines()
if len(rows) != 36:
    raise SystemExit(f'incomplete capture: {len(rows)}')
header = f'# server GameAssembly SHA256={digest}\n# original slices 631b292..29d, 631b2a5..2d8, 631b2dd..2ed and helper 60780b0; initialized Movement=5a, supplied EntityInput; query/predicates excluded\n# input.x bits, input.y bits, resulting 104-byte Movement hex\n'
(ROOT/'evidence/oracles/ability_movement_setup.tsv').write_text(header+'\n'.join(rows)+'\n')
print(f'captured {len(rows)} cases')
