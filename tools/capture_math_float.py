#!/usr/bin/env python3
"""Replay original Unity.Mathematics scalar float leaves (see oracle_replay.py)."""
from oracle_replay import ROOT, load_image, run, XORSHIFT

FUNCS = [('f32tof16', 0x7654b90), ('f16tof32', 0x612f520), ('radians', 0x5b4f850), ('degrees', 0x92b8ac0),
         ('unlerp', 0x92a6a90), ('remap', 0x92a6e40), ('step', 0x92b3230), ('sign', 0x5b10e60),
         ('clampf', 0x5b10ed0), ('minf', 0x1a5fac0), ('maxf', 0x1a5fae0), ('smoothstep', 0x92b2040)]

DRIVER = XORSHIFT + r'''
#define MS __attribute__((ms_abi))
extern uint32_t f32tof16(float) MS; extern float f16tof32(int32_t) MS;
extern float radians(float) MS; extern float degrees(float) MS;
extern float unlerp(float,float,float) MS; extern float remap(float,float,float,float,float) MS;
extern float step(float,float) MS; extern float sign(float) MS;
extern float clampf(float,float,float) MS; extern float minf(float,float) MS; extern float maxf(float,float) MS;
extern float smoothstep(float,float,float) MS;
static float f(uint32_t b){float x;memcpy(&x,&b,4);return x;}
static uint32_t u(float x){uint32_t b;memcpy(&b,&x,4);return b;}
static uint32_t gen(int i,int k){
 static const uint32_t sp[]={0,0x80000000,0x3f800000,0xbf800000,0x7f800000,0xff800000,0x7fc00000,0xffc00000,0x7f7fffff,0x00000001,0x007fffff,0x00800000,0x38800000,0x477fe000,0x477ff000,0x47800000,0x33000000,0x7fa00000,0x3f000000,0x40490fdb};
 uint32_t r=rnd();
 switch((i+k)%5){case 0:return sp[rnd()%20];case 1:return r;case 2:return (r&0x80000000)|(((rnd()%60)+97)<<23)|(rnd()&0x7fffff);case 3:return u((float)((int32_t)(r%2001)-1000)/8.0f);default:return r&0xbfffffff;}
}
int main(void){for(int i=0;i<2048;i++){
 uint32_t a=gen(i,0),b=gen(i,1),c=gen(i,2),d=gen(i,3),e=gen(i,4); if(i<20){a=((uint32_t[]){0,0x80000000,0x3f800000,0xbf800000,0x7f800000,0xff800000,0x7fc00000,0xffc00000,0x7f7fffff,0x00000001,0x007fffff,0x00800000,0x38800000,0x477fe000,0x477ff000,0x47800000,0x33000000,0x7fa00000,0x3f000000,0x40490fdb})[i];}
 int32_t h=(int32_t)rnd(); if(i<2048&&(i&3)==0)h&=0xffff; if(i<65536&&i<1100&&i>=1000)h=(i-1000)*655;
 printf("%08x\t%08x\t%08x\t%08x\t%08x\t%08x",a,b,c,d,e,(uint32_t)h);
 printf("\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\n",
  f32tof16(f(a)),u(f16tof32(h)),u(radians(f(a))),u(degrees(f(a))),u(unlerp(f(a),f(b),f(c))),
  u(remap(f(a),f(b),f(c),f(d),f(e))),u(step(f(a),f(b))),u(sign(f(a))),u(clampf(f(a),f(b),f(c))),
  u(minf(f(a),f(b))),u(maxf(f(a),f(b))),u(smoothstep(f(a),f(b),f(c))));
 }return 0;}
'''

def main():
    image, digest = load_image()
    rows = run(image, FUNCS, DRIVER)
    if len(rows) != 2048:
        raise SystemExit('incomplete original output')
    header = (f'# Original GameAssembly.dll sha256={digest}\n# x86_64/Rosetta replay of scalar float math leaves; floats as IEEE bits\n'
              '# RVAs: ' + ','.join(f'{n}={r:x}' for n, r in FUNCS) + '\n'
              '# a b c d e half | f32tof16(a) f16tof32(half) radians(a) degrees(a) unlerp(a,b,c) remap(a,b,c,d,e) step(a,b) sign(a) clamp(a,b,c) min(a,b) max(a,b) smoothstep(a,b,c)\n')
    (ROOT / 'evidence/oracles/math_float.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'Captured {len(rows)} cases x {len(FUNCS)} original float math functions')

if __name__ == '__main__':
    main()
