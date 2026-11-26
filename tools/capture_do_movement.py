#!/usr/bin/env python3
"""Replay the server DoMovement's ordinary, collision-free branch in isolation.

Original x86-64 instructions run under Rosetta. Debug functions and world-type
lookup are stubs; runtime-only branches trap if reached. This is not a full game
or a collision/ability replay.
"""
import hashlib
import os
from pathlib import Path
import re
import subprocess
import oracle_replay as oracle

p = Path(os.environ.get('VRISING_SERVER_ASSEMBLY', str(Path.home() / 'Library/Application Support/Steam/steamapps/common/VRising/VRising_Server/GameAssembly.dll')))
image = p.read_bytes()
digest = hashlib.sha256(image).hexdigest()
if digest != '41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7':
    raise SystemExit('server GameAssembly digest mismatch')

# The function is larger than the default leaf window. Keep the common decoder
# and relocation checks; explicitly reject any non-local call not listed below.
original_read = oracle.code_at_rva
def wide_read(data, rva, size):
    if rva == 0x83acd50 and size == 3000:
        return original_read(data, rva, 5200)
    return original_read(data, rva, size)
oracle.code_at_rva = wide_read
asm = subprocess.run(['/opt/homebrew/opt/llvm/bin/llvm-objdump','-d','--start-address=0x1883acd50','--stop-address=0x1883ae060',str(p)],check=True,capture_output=True,text=True).stdout
calls = {int(x,16)-0x180000000 for x in re.findall(r'callq\s+0x([0-9a-f]+)',asm)}
stubs = {a:(f'trap_{a:x}','ud2') for a in calls}
for a in (0x78951a0,0x78954a0,0x7895aa0):
    stubs[a] = (f'debug_{a:x}', 'ret')
stubs[0xcfe7c0] = ('world_type', 'xor %eax, %eax\nret')
stubs[0x1b2ba50] = ('square_root', 'sqrtss %xmm0, %xmm0\nret')
overrides = {0xe0d4cc5: bytes([1])+bytes(15)}

driver = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define MS __attribute__((ms_abi))
extern void movement(void*,void*,void*,void*,void*,void*,void*,void*,float,void*,void*,_Bool,_Bool,float,float,void*,void*,void*,void*,void*,void*,void*,void*,uint64_t,void*) MS;
static uint32_t bits(float f){uint32_t x;memcpy(&x,&f,4);return x;}
int main(void) {
  float inputs[][2]={{0,0},{1,0},{0,-1},{.6f,.8f},{-1.5f,2.f},{-0.0f,0.0f},{0.0001f,-0.0001f}};
  float speeds[]={-4,-0.0f,0,4,7.25f}; float mults[]={-1,-0.0f,0,.25f,1,2,__builtin_inff(),__builtin_nanf("")}; float dts[]={-0.016f,0,0.016f,.1f};
  for(int can=0;can<2;can++)for(int i=0;i<7;i++)for(int s=0;s<5;s++)for(int m=0;m<8;m++)for(int d=0;d<4;d++){
    uint8_t debug[32]={0}, ability[96]={0},casting[144]={0},curve[16]={0};
    float vel[3]={7,8,9},move=123,pos[3]={1,2,3},rot[4]={0,0,0,1}; uint64_t entity=1;
    memcpy(ability+0x58,&mults[m],4);
    movement(debug,inputs[i],&entity,vel,&move,pos,rot,curve,0,ability,casting,0,can,speeds[s],dts[d],0,0,0,0,0,0,0,0,0,0);
    printf("%d\t%08x\t%08x\t%08x\t%08x\t%08x",can,bits(inputs[i][0]),bits(inputs[i][1]),bits(speeds[s]),bits(mults[m]),bits(dts[d]));
    for(int k=0;k<3;k++)printf("\t%08x",bits(vel[k]));printf("\t%08x",bits(move));for(int k=0;k<3;k++)printf("\t%08x",bits(pos[k]));printf("\n");
  }return 0;
}
'''
rows=oracle.run(image,[('movement',0x83acd50,False,0x1310)],driver,stubs=stubs,rip_overrides=overrides)
if len(rows)!=2240: raise SystemExit(f'incomplete capture: {len(rows)}')
header=f'# server GameAssembly sha256={digest}; DoMovement RVA=83acd50\n# isolated x86-64/Rosetta replay; initialized metadata flag, debug and world type stubbed, sqrtss stub; no force position, move-stop, casting or logging\n# canMove input.x input.y speed multiplier dt velocity.x velocity.y velocity.z moveVelocity translation.x translation.y translation.z (floats as bits)\n'
(oracle.ROOT/'evidence/oracles/do_movement.tsv').write_text(header+'\n'.join(rows)+'\n')
print(f'captured {len(rows)} cases')
