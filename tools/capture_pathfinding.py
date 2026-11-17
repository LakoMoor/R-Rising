#!/usr/bin/env python3
"""Capture the original movement waypoint selector, with no helper stubs.

Native function is PathfindingUtility.TryGetTargetPositionFromWaypoints, not
an execution of the ECS system or navigation service. Goldens contain only
inputs, output and return value. Original instruction copies are temporary.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

ENTRIES = [('try_target', 0x75c6390)]
DRIVER = XORSHIFT + r'''
#define MS __attribute__((ms_abi))
extern uint8_t try_target(const void*,uint64_t,uint64_t,float,float*) MS;
static float f(uint32_t bits){float v;memcpy(&v,&bits,4);return v;}
static uint32_t u(float v){uint32_t bits;memcpy(&bits,&v,4);return bits;}
static uint64_t pair(float x,float y){return ((uint64_t)u(y)<<32)|u(x);}
int main(void) {
  const uint32_t specials[]={0,0x80000000,0x3e800000,0xbf000000,0x7f800000,0xff800000,0x7fc01234};
  for(int i=0;i<4096;i++) {
    uint8_t way[32];memset(way,0xa5,sizeof way);
    int32_t coords[4];float pos[2],dir[2],radius;
    for(int k=0;k<4;k++)coords[k]=(int32_t)(rnd()%65536);
    for(int k=0;k<2;k++){pos[k]=(int32_t)(rnd()%100000)/8.0f-6250.0f;dir[k]=(int32_t)(rnd()%2001)/1000.0f-1.0f;}
    radius=(float)(rnd()%1000)/100.0f;
    if(i<64){coords[0]=6400;coords[1]=6400;coords[2]=6402;coords[3]=6398;
       pos[0]=0.05f;pos[1]=0.05f;dir[0]=(i&4)?1:0;dir[1]=(i&8)?-1:0;radius=0;
       if(i&16)pos[0]+=1; if(i&32)pos[1]-=1;}
    if(i>=64&&i<256){for(int k=0;k<4;k++)coords[k]=(int32_t)rnd();radius=f(specials[(i/4)%7]);
       pos[0]=f(specials[(i/7)%7]);dir[1]=f(specials[(i/13)%7]);}
    if(i>=256&&i<384){for(int k=0;k<4;k++)coords[k]=(i&4)?INT32_MIN:INT32_MAX;}
    memcpy(way,coords,sizeof coords);way[16]=(i&1)?1:0;way[17]=(i&2)?1:0;
    if(i>=384 && i<512){way[16]=(uint8_t)rnd();way[17]=(uint8_t)rnd();}
    uint8_t before[32]; memcpy(before,way,sizeof way);
    float out[4]={f(0x4f123456),f(0xcf123456),f(0x5a123456),f(0xda123456)};
    uint8_t ok=try_target(way,pair(pos[0],pos[1]),pair(dir[0],dir[1]),radius,out);
    printf("%08x\t%08x\t%08x\t%08x\t%u\t%u\t%08x\t%08x\t%08x\t%08x\t%08x\t%u\t%08x\t%08x\n",
      (uint32_t)coords[0],(uint32_t)coords[1],(uint32_t)coords[2],(uint32_t)coords[3],way[16],way[17],
      u(pos[0]),u(pos[1]),u(dir[0]),u(dir[1]),u(radius),ok,u(out[0]),u(out[1]));
    if(u(out[2])!=0x5a123456 || u(out[3])!=0xda123456 || memcmp(way,before,sizeof way))return 2;
  }
  return 0;
}
'''


def main():
    image, digest = load_image()
    rows = run(image, ENTRIES, DRIVER)
    if len(rows) != 4096:
        raise SystemExit('incomplete original waypoint output')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# x86_64/Rosetta replay, no helper stubs; includes noncanonical bool bytes\n'
              '# RVAs: TryGetTargetPositionFromWaypoints=75c6390\n'
              '# current_int2 next_int2 (hex i32), current_flag next_flag (decimal u8), '
              'position_float2 direction_float2 radius (hex f32), found, target_float2 (hex f32)\n')
    (ROOT / 'evidence/oracles/pathfinding.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'pathfinding.tsv: {len(rows)} native waypoint cases, write guards intact')


if __name__ == '__main__':
    main()
