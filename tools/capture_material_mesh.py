#!/usr/bin/env python3
"""Capture original MaterialMeshInfo leaf behavior, using isolated x86-64/Rosetta replay."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from capture_type_flags import code_at_rva


def main():
    root=Path(__file__).resolve().parent.parent
    image=(root.parent/'GameAssembly.dll').read_bytes()
    digest=hashlib.sha256(image).hexdigest()
    reference=json.loads((root/'reference.lock.json').read_text())
    expected=next(f['sha256'] for f in reference['files'] if f['path']=='GameAssembly.dll')
    if digest!=expected: raise SystemExit('pinned original binary mismatch')
    entries=[('original_to_static',0x8f46ef0,10),('original_to_array',0x8f46f00,12),
             ('original_submesh',0x8f46e70,13),('original_has_range',0x8f46ed0,11),
             ('original_set_submesh',0x8f46e80,7)]
    assembly=['.text']
    for name,rva,size in entries:
        code=code_at_rva(image,rva,size)
        if code[-1]!=0xc3: raise SystemExit('leaf does not end with RET')
        assembly += [f'.globl _{name}',f'_{name}:','.byte '+','.join(hex(b) for b in code)]
    source=r'''#include <stdint.h>
#include <stdio.h>
typedef struct {int32_t material,mesh; uint32_t packed;} Info;
extern int32_t original_to_static(int32_t) __attribute__((ms_abi));
extern int32_t original_to_array(int32_t) __attribute__((ms_abi));
extern uint16_t original_submesh(const Info*) __attribute__((ms_abi));
extern uint8_t original_has_range(const Info*) __attribute__((ms_abi));
extern void original_set_submesh(Info*,uint16_t) __attribute__((ms_abi));
int main(void){
 uint32_t state=0x8f46ef0u;
 const uint32_t boundaries[]={0,1,127,128,255,256,65535,0x7fffffff,0x80000000,0x80000001,0xfffffffe,0xffffffff};
 for(int i=0;i<1024;i++){
  uint32_t raw;
  if(i<12)raw=boundaries[i];else{state^=state<<13;state^=state>>17;state^=state<<5;raw=state;}
  Info info={42,17,raw};
  uint16_t sub=original_submesh(&info);uint8_t range=original_has_range(&info);
  original_set_submesh(&info,(uint16_t)(raw>>16));
  printf("%08x\t%d\t%d\t%u\t%u\t%08x\n",raw,original_to_static((int32_t)raw),original_to_array((int32_t)raw),(unsigned)sub,(unsigned)range,info.packed);
 }
}
'''
    with tempfile.TemporaryDirectory(prefix='rrising-mesh-oracle-') as scratch:
        scratch=Path(scratch)
        (scratch/'original.S').write_text('\n'.join(assembly)+'\n')
        (scratch/'driver.c').write_text(source)
        subprocess.run(['/usr/bin/clang','-arch','x86_64','-O2',str(scratch/'driver.c'),str(scratch/'original.S'),'-o',str(scratch/'oracle')],check=True)
        result=subprocess.run(['/usr/bin/arch','-x86_64',str(scratch/'oracle')],check=True,text=True,capture_output=True,timeout=10)
    if len(result.stdout.splitlines())!=1024:raise SystemExit('incomplete original output')
    output=root/'tests/golden/material_mesh.tsv'
    output.write_text(f'# Original GameAssembly.dll sha256={digest}\n# Actual leaf RVAs 8f46ef0,8f46f00,8f46e70,8f46ed0,8f46e80; x86_64/Rosetta.\n# raw_hex to_static to_array submesh has_range setter_result_hex\n'+result.stdout)
    print(f'Captured 1024 cases from five original MaterialMeshInfo functions -> {output}')

if __name__=='__main__': main()
