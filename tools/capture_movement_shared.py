#!/usr/bin/env python3
"""Replay original shared steering body with supplied component lookup boundaries.

The job's control flow, waypoint selector and normalization body are original
instructions. Five ECS accessors are supplied-value callbacks, metadata is
already-initialized local state, and the sqrt wrapper is a sqrtss stub. This
does NOT execute the original EntityComponentStore, scheduler or whole game.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

ENTRIES = [('steer', 0x669a880, False, 0x33b),
           ('waypoint_target', 0x75c6390), ('normalize2', 0x63354f0)]
STUBS = {
    0x208f70: ('translation_lookup', 'jmp _get_translation'),
    0x2027b0: ('collision_lookup', 'jmp _get_collision'),
    0x20a300: ('velocity_lookup', 'jmp _get_velocity'),
    0x1fdbb0: ('input_lookup', 'jmp _get_input'),
    0x21e380: ('input_setter', 'jmp _set_input'),
    0x16729f0: ('unexpected_metadata_init', 'ud2'),
    0x1b28e70: ('sqrt_stub', 'sqrtss %xmm0, %xmm0\nret'),
}
OVERRIDES = {
    0xdfc124e: bytes([1]) + bytes(15),
    0xda77978: ('.quad Lfloat2_class', '.quad 0'),
}
EXTRA = '''
.data
.balign 16
Lfloat2_class:
.zero 184
.quad Lfloat2_zero
Lfloat2_zero:
.zero 8
'''

DRIVER = XORSHIFT + r'''
#include <stdlib.h>
#define MS __attribute__((ms_abi))
extern void steer(void*,const void*,const void*,const void*) MS;
extern uint64_t normalize2(uint64_t,uint64_t) MS;
static float owner_translation[3], owner_velocity[3];
static uint8_t collision[20], old_input[96], new_input[112];
static unsigned calls[5];
static const uint64_t target_entity=0x000000030000004dULL;
static void check(uint64_t entity){if(entity!=target_entity)exit(3);}
void* MS get_translation(void* out,const void* lookup,uint64_t entity,const void* method){check(entity);calls[0]++;memcpy(out,owner_translation,12);return out;}
void* MS get_collision(void* out,const void* lookup,uint64_t entity,const void* method){check(entity);calls[1]++;memcpy(out,collision,20);return out;}
void* MS get_velocity(void* out,const void* lookup,uint64_t entity,const void* method){check(entity);calls[2]++;memcpy(out,owner_velocity,12);return out;}
void* MS get_input(void* out,const void* lookup,uint64_t entity,const void* method){check(entity);calls[3]++;memcpy(out,old_input,96);return out;}
void MS set_input(const void* lookup,uint64_t entity,const void* in,const void* method){check(entity);calls[4]++;memcpy(new_input,in,96);}
static float f(uint32_t bits){float v;memcpy(&v,&bits,4);return v;}
static uint32_t u(float v){uint32_t bits;memcpy(&bits,&v,4);return bits;}
static uint64_t pair(float x,float y){return ((uint64_t)u(y)<<32)|u(x);}
int main(void){
 const uint32_t special[]={0,0x80000000,0x7f800000,0xff800000,0x7fc01234,0x20000000,0x5f800000};
 for(int i=0;i<4096;i++){
  uint8_t job[0x120]={0},buff[32]={0},move[40]={0};float translation[3],radius,threshold;
  int32_t coords[4];for(int k=0;k<4;k++)coords[k]=(int32_t)(rnd()%20001);
  for(int k=0;k<3;k++){translation[k]=(int32_t)(rnd()%20001)/4.0f-2500.0f;
    owner_translation[k]=(int32_t)(rnd()%20001)/4.0f-2500.0f;
    owner_velocity[k]=(int32_t)(rnd()%2001)/100.0f-10.0f;}
  radius=(float)(rnd()%101)/100.0f;
  float dx=translation[0]-owner_translation[0],dz=translation[2]-owner_translation[2];
  threshold=(i%3==0)?dx*dx+dz*dz:(i%3==1)?0.0f:1e8f;
  if(i<64){for(int k=0;k<3;k++){translation[k]=(i&4)?0.0f:1.0f;owner_translation[k]=0.0f;owner_velocity[k]=0.0f;}threshold=(i&8)?0.0f:3.0f;}
  if(i>=64&&i<512){translation[i%3]=f(special[(i/3)%7]);owner_translation[(i/7)%3]=f(special[(i/11)%7]);
    owner_velocity[0]=f(special[(i/5)%7]);owner_velocity[2]=f(special[(i/13)%7]);threshold=f(special[(i/17)%7]);radius=f(special[(i/19)%7]);}
  if(i>=512&&i<640){for(int k=0;k<3;k++)owner_translation[k]=translation[k];}
  memcpy(buff+8,&target_entity,8);memcpy(move,&threshold,4);memcpy(move+16,coords,16);
  move[32]=(i&1)?1:0;move[33]=(i&2)?1:0;memcpy(collision,&radius,4);
  for(int k=4;k<20;k++)collision[k]=(uint8_t)rnd();
  for(int k=0;k<96;k++)old_input[k]=(uint8_t)rnd();
  memset(new_input,0xa5,sizeof new_input);memset(calls,0,sizeof calls);
  uint8_t buff_before[32],move_before[40];memcpy(buff_before,buff,32);memcpy(move_before,move,40);
  steer(job,translation,buff,move);
  for(int k=0;k<5;k++)if(calls[k]!=1)return 4;
  for(int k=96;k<112;k++)if(new_input[k]!=0xa5)return 5;
  if(memcmp(buff,buff_before,32)||memcmp(move,move_before,40))return 6;
  if(memcmp(new_input+32,old_input+32,64))return 7;
  uint64_t norm=normalize2(pair(owner_velocity[0],owner_velocity[2]),pair(translation[0],translation[2]));
  for(int k=0;k<4;k++)printf("%08x\t",(uint32_t)coords[k]);
  printf("%u\t%u",move[32],move[33]);
  for(int k=0;k<3;k++)printf("\t%08x",u(translation[k]));
  for(int k=0;k<3;k++)printf("\t%08x",u(owner_translation[k]));
  for(int k=0;k<3;k++)printf("\t%08x",u(owner_velocity[k]));
  printf("\t%08x\t%08x\t",u(radius),u(threshold));
  for(int k=0;k<96;k++)printf("%02x",old_input[k]);
  printf("\t%08x\t%08x\t",(uint32_t)norm,(uint32_t)(norm>>32));
  for(int k=0;k<96;k++)printf("%02x",new_input[k]);printf("\n");
 }
 return 0;
}
'''


def main():
    image, digest = load_image()
    rows = run(image, ENTRIES, DRIVER, stubs=STUBS, rip_overrides=OVERRIDES, extra_assembly=EXTRA)
    if len(rows) != 4096:
        raise SystemExit('incomplete native steering capture')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# Controlled x86_64/Rosetta replay; not original ECS/runtime execution\n'
              '# RVAs: SharedSteering=669a880 NormalizeSafeFloat2=63354f0\n'
              '# Isolation: 5 supplied-value ECS accessor callbacks; initialized metadata; float2.zero static; sqrtss wrapper stub\n'
              '# inputs: waypoint int2,next int2 hex; flags decimal; source Translation3,target Translation3,target Velocity3,radius,DistanceSq f32 hex; '
              'old EntityInput96 byte hex; outputs: helper normalize2(target Velocity.xz,source Translation.xz) f32 hex, new EntityInput96 byte hex\n')
    (ROOT / 'evidence/oracles/movement_shared.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'movement_shared.tsv: {len(rows)} native steering and normalize cases; five lookup/write callbacks exactly once each; preserved tail and write guards intact')


if __name__ == '__main__':
    main()
