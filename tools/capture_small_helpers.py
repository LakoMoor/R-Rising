#!/usr/bin/env python3
"""Replay three small original helpers: SunUtility.IsInSun, BloodBuffUtility.GetProgress(float,float)
and SoundVolumetricMath.Lerp (clamped float3 lerp).

Original bytes (client RVAs 0x7c98bc0, 0x811e700, 0x7f7cd00) run under Rosetta with the Windows x64
calling convention; constants are read from the pinned image. No stubs are needed. A controlled
function replay, not game execution.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

ENTRIES = [('is_in_sun', 0x7c98bc0), ('get_progress', 0x811e700), ('sound_lerp', 0x7f7cd00)]

DRIVER = XORSHIFT + r'''
#define MS __attribute__((ms_abi))
extern uint8_t is_in_sun(float) MS;
extern float get_progress(float,float) MS;
extern float* sound_lerp(float*,const float*,const float*,float) MS;
static float f(uint32_t b){float v;memcpy(&v,&b,4);return v;}
static uint32_t u(float v){uint32_t b;memcpy(&b,&v,4);return b;}
static float rf(float s){return ((int32_t)(rnd()%200001)-100000)/100000.0f*s;}
int main(void){
 const uint32_t sp[]={0,0x80000000,0x7f800000,0xff800000,0x7fc01234,0x3f000000,0x3effffff,0x3f000001,0x42c80000,0x3f800000};
 for(int i=0;i<4096;i++){
  float s=rf(1.0f),a=rf(150.0f),b=rf(150.0f),p[3],q[3],t=rf(2.0f),o[3];
  for(int k=0;k<3;k++){p[k]=rf(500.0f);q[k]=rf(500.0f);}
  if(i>=3000){float*x[]={&s,&a,&b,&t,&p[0],&q[1]};for(int k=0;k<6;k++)if(rnd()%3==0)*x[k]=f(sp[rnd()%10]);}
  uint8_t r1=is_in_sun(s); float r2=get_progress(a,b); sound_lerp(o,p,q,t);
  printf("%08x\t%u\t%08x\t%08x\t%08x",u(s),r1&1,u(a),u(b),u(r2));
  for(int k=0;k<3;k++)printf("\t%08x",u(p[k]));
  for(int k=0;k<3;k++)printf("\t%08x",u(q[k]));
  printf("\t%08x",u(t));
  for(int k=0;k<3;k++)printf("\t%08x",u(o[k]));
  printf("\n");
 }
 return 0;}
'''


def main():
    image, digest = load_image()
    rows = run(image, ENTRIES, DRIVER)
    if len(rows) != 4096:
        raise SystemExit(f'incomplete capture: {len(rows)} rows')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# Controlled x86_64/Rosetta replay of three small helpers; no stubs\n'
              '# RVAs: IsInSun=7c98bc0 GetProgress=811e700 SoundLerp=7f7cd00\n'
              '# columns: sunIntensity f32, isInSun 0/1 | quality threshold progress (f32) | from.xyz to.xyz t out.xyz (f32 hex)\n')
    (ROOT / 'evidence/oracles/small_helpers.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'small_helpers.tsv: {len(rows)} cases')


if __name__ == '__main__':
    main()
