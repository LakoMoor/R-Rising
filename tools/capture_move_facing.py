#!/usr/bin/env python3
"""Replay the nonzero-input, no-mouse-facing instruction slice of the pinned server.

Entry predicate is assumed; square-root wrapper is replaced with sqrtss and the
constant loaded by the original RIP-relative instruction is supplied as 1.0.
This is neither the full direction function nor a game/runtime observation.
"""
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
    raise SystemExit('unexpected GameAssembly')
a = code_at_rva(image, 0x83aa956, 3)
b = code_at_rva(image, 0x83aa95d, 0x2c)
c = code_at_rva(image, 0x83aa997, 0x23)
assert a.hex() == '0f103b'
assert b.hex() == 'f30f1073040f28c70f28ce410f14c4f20f11070f28c7f30f59c7f30f59cef3410f58c4f30f117708f30f58c1'
assert c.hex() == '0f57c9410f28d5f30f5ed0f30f59faf30f59caf30f59f20f14f9f20f113ff30f117708'
emit = lambda b: '.byte ' + ','.join(hex(v) for v in b)
assembly = '''.text
.globl _facing
_facing:
pushq %rbx
pushq %rdi
subq $72, %rsp
movups %xmm6, 0(%rsp)
movups %xmm7, 16(%rsp)
movups %xmm12, 32(%rsp)
movups %xmm13, 48(%rsp)
movq %rcx, %rbx
movq %rdx, %rdi
xorps %xmm12, %xmm12
''' + emit(a) + '\n' + emit(b) + '''
sqrtss %xmm0, %xmm0
movl $0x3f800000, %eax
movd %eax, %xmm13
''' + emit(c) + '''
movups 0(%rsp), %xmm6
movups 16(%rsp), %xmm7
movups 32(%rsp), %xmm12
movups 48(%rsp), %xmm13
addq $72, %rsp
popq %rdi
popq %rbx
retq
'''
driver = r'''#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define MS __attribute__((ms_abi))
void facing(float*,float*) MS;
static uint32_t bits(float f){uint32_t x;memcpy(&x,&f,4);return x;}
int main(void){float v[]={-12.5f,-1.f,-.5f,-.001f,0.f,.001f,.5f,1.f,12.5f};
for(int i=0;i<9;i++)for(int j=0;j<9;j++){float in[4]={v[i],v[j],0,0},out[3];if(!v[i]&&!v[j])continue;facing(in,out);printf("%08x\t%08x\t%08x\t%08x\t%08x\n",bits(in[0]),bits(in[1]),bits(out[0]),bits(out[1]),bits(out[2]));}return 0;}
'''
with tempfile.TemporaryDirectory(prefix='rrising-facing-') as d:
    d=Path(d); (d/'slice.S').write_text(assembly); (d/'driver.c').write_text(driver)
    subprocess.run(['/usr/bin/clang','-arch','x86_64','-O1',str(d/'driver.c'),str(d/'slice.S'),'-o',str(d/'replay')],check=True)
    rows=subprocess.run(['/usr/bin/arch','-x86_64',str(d/'replay')],check=True,capture_output=True,text=True).stdout.splitlines()
if len(rows)!=80: raise SystemExit('missing replay rows')
(ROOT/'evidence/oracles/move_facing.tsv').write_text(f'# server sha256={digest}; slices RVA 0x83aa956..959, 0x83aa95d..989, 0x83aa997..9ba\n# nonzero finite input and mouse-facing disabled assumed; sqrtss stub; 1.0 constant supplied; no speed computation\n# input.x input.y direction.x direction.y direction.z (f32 bits)\n'+'\n'.join(rows)+'\n')
print(f'captured {len(rows)} cases')
