"""Capture isolated GetMovement cases; original code and images are not distributed."""
import hashlib
import json
import os
from pathlib import Path
from oracle_replay import ROOT, run
p = Path(os.environ.get('VRISING_GAME_ASSEMBLY', ROOT.parent / 'GameAssembly.dll'))
image = p.read_bytes()
expected = next(f['sha256'] for f in json.loads((ROOT / 'reference.lock.json').read_text())['files'] if f['path'] == 'GameAssembly.dll')
digest = hashlib.sha256(image).hexdigest()
if digest != expected:
    raise SystemExit('pinned original binary mismatch')
driver=r'''#include <stdio.h>
#include <stdint.h>
#include <string.h>
#define MS __attribute__((ms_abi))
extern void *getmovement(void*, const void*, void*) MS;
static uint32_t bits(float x){uint32_t u;memcpy(&u,&x,4);return u;}
int main(void){
 float inputs[][2]={{0,0},{.001f,0},{.01f,0},{.009999f,0},{.010001f,0},{.1f,0},{.24999f,0},{.25f,0},{.25001f,0},{.5f,.5f},{1.f,0},{1.00001f,0},{2.f,0},{-1.f,.5f}};
 float rotations[][4]={{0,0,0,1},{0,.70710677f,0,.70710677f},{0,-.70710677f,0,.70710677f},{.5f,.5f,.5f,.5f},{0,0,0,0}};
 for(int mode=0;mode<4;mode++)for(int flag=0;flag<2;flag++)for(unsigned qn=0;qn<5;qn++)for(unsigned i=0;i<14;i++){
  float out[3]={9,9,9}, previous[3]={7,8,9}, stored[3];uint8_t s[80]={0};
  memcpy(s+24,inputs[i],8);memcpy(s+44,previous,12);s[40]=mode;s[42]=flag;
  getmovement(out,rotations[qn],s);memcpy(stored,s+44,12);
  printf("%d\t%d",mode,flag);for(int j=0;j<2;j++)printf("\t%08x",bits(inputs[i][j]));
  for(int j=0;j<4;j++)printf("\t%08x",bits(rotations[qn][j]));
  for(int j=0;j<3;j++)printf("\t%08x",bits(previous[j]));
  for(int j=0;j<3;j++)printf("\t%08x",bits(out[j]));
  for(int j=0;j<3;j++)printf("\t%08x",bits(stored[j]));printf("\n");
 }return 0;}

'''
overrides={0xdfc8558: bytes([1])+bytes(15),0xda46480:('.quad Lklass','.quad 0')}
extra='''\n.data\n.balign 16\nLklass:\n.zero 184\n.quad Lzero\nLzero:\n.zero 16\n'''
rows = run(image,[('getmovement',0x7d907d0,False,0x2b9)],driver,stubs={0x16729f0:('bad_init','ud2'),0x1b28e70:('square_root','sqrtss %xmm0, %xmm0\nret')},rip_overrides=overrides,extra_assembly=extra)
if len(rows) != 560:
    raise SystemExit('incomplete GetMovement capture')
header = f'# Pinned client SHA256={digest}; GetMovement RVA=0x7d907d0\n# isolated x86-64 replay; metadata/float3.zero supplied; sqrt wrapper replaced with sqrtss; varied camera quaternions\n# mode flag input2 camera_quaternion4 previous_direction3 return3 stored_direction3; float bits in hex\n'
(ROOT / 'evidence/oracles/player_get_movement.tsv').write_text(header + '\n'.join(rows) + '\n')
print(f'captured {len(rows)} cases')
