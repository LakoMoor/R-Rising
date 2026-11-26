#!/usr/bin/env python3
"""Replay bounded original instruction slices selecting command movement.

The caller supplies successful Movement lookup and IsSpellControlled lookup
results; controller validity and the real lookup calls are outside this replay.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
from oracle_replay import ROOT, code_at_rva

p = Path(os.environ.get('VRISING_SERVER_ASSEMBLY', '/Users/lakomoor/Library/Application Support/Steam/steamapps/common/VRising/VRising_Server/GameAssembly.dll'))
image = p.read_bytes()
digest = hashlib.sha256(image).hexdigest()
if digest != '41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7':
    raise SystemExit('server GameAssembly digest mismatch')

raw = code_at_rva(image, 0x7e4e804, 4)
present = code_at_rva(image, 0x7e4e83b, 0x30)
if raw.hex() != 'f20f1037' or present.hex() != '84c0742380bc248007000000750a80bc248107000000740f33c04889442430f20f10742430eb09f20f10b4247c020000':
    raise SystemExit('unexpected instruction slice')
emit = lambda b: '.byte ' + ','.join(hex(v) for v in b)
assembly = '''.text
.globl _select_raw
_select_raw:
pushq %rdi
movq %rcx, %rdi
subq $16, %rsp
movups %xmm6, (%rsp)
''' + emit(raw) + '''
movq %xmm6, %rax
movups (%rsp), %xmm6
addq $16, %rsp
popq %rdi
retq
.globl _select_present
_select_present:
subq $0x800, %rsp
movups %xmm6, 0x700(%rsp)
movq %rcx, 0x27c(%rsp)
movw %dx, 0x780(%rsp)
movb %r8b, %al
''' + emit(present) + '''
movq %xmm6, %rax
movups 0x700(%rsp), %xmm6
addq $0x800, %rsp
retq
'''
driver = r'''#include <stdint.h>
#include <stdio.h>
#define MS __attribute__((ms_abi))
uint64_t select_raw(const uint64_t*) MS;
uint64_t select_present(uint64_t,uint16_t,uint8_t) MS;
int main(void) {
  uint64_t vals[]={0,0x3f800000bf000000ULL,0x7fc0123480000000ULL,0xff8000007f800000ULL};
  for(int v=0;v<4;v++) {
    printf("raw\t%016llx\t%016llx\n",vals[v],select_raw(&vals[v]));
    for(int present=0;present<2;present++) for(int a=0;a<2;a++) for(int b=0;b<2;b++) {
      uint16_t flags=a|(b<<8);
      printf("present\t%016llx\t%d\t%d\t%d\t%016llx\n", vals[v],present,a,b,select_present(vals[v],flags,present));
    }
  }
}
'''
with tempfile.TemporaryDirectory(prefix='rrising-command-selection-') as tmp:
    tmp = Path(tmp)
    (tmp / 'slice.S').write_text(assembly)
    (tmp / 'driver.c').write_text(driver)
    subprocess.run(['/usr/bin/clang', '-arch', 'x86_64', '-O1', '-Wno-format', str(tmp / 'driver.c'), str(tmp / 'slice.S'), '-o', str(tmp / 'replay')], check=True)
    rows = subprocess.run(['/usr/bin/arch', '-x86_64', str(tmp / 'replay')], check=True, capture_output=True, text=True).stdout.splitlines()
if len(rows) != 36:
    raise SystemExit(f'incomplete capture: {len(rows)}')
header = f'# server GameAssembly SHA256={digest}\n# slices RVA=0x7e4e804 (4 bytes), RVA=0x7e4e83b..0x7e4e86b; lookup results supplied, not live ECS\n# raw: value result; present: movement lookup_present movement_controlled rotation_controlled result; vectors encoded as raw 64-bit hex\n'
(ROOT / 'evidence/oracles/command_move_selection.tsv').write_text(header + '\n'.join(rows) + '\n')
print(f'captured {len(rows)} cases')
