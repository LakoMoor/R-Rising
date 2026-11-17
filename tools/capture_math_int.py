#!/usr/bin/env python3
"""Replay original Unity.Mathematics integer leaf functions (see oracle_replay.py)."""
from oracle_replay import ROOT, load_image, run, XORSHIFT

CASES = 2048
FUNCS = [  # name, rva, C return type, arity description
    ('lzcnt_u32', 0x92b61e0), ('reversebits_u32', 0x92b68b0), ('ceilpow2_i32', 0x4740e90),
    ('ceilpow2_u32', 0x92b7e80), ('ceillog2_i32', 0x8e97d30), ('floorlog2_u32', 0x8e97ce0),
    ('ispow2_i32', 0x92a58e0), ('abs_i32', 0x92a8830), ('min_i32', 0x184efe0),
    ('max_i32', 0x184efd0), ('clamp_i32', 0x1a100f0)]

DRIVER = XORSHIFT + r'''
#include <stdint.h>
#include <stdio.h>
#define MS __attribute__((ms_abi))
extern int32_t lzcnt_u32(uint32_t) MS; extern uint32_t reversebits_u32(uint32_t) MS;
extern int32_t ceilpow2_i32(int32_t) MS; extern uint32_t ceilpow2_u32(uint32_t) MS;
extern int32_t ceillog2_i32(int32_t) MS; extern int32_t floorlog2_u32(uint32_t) MS;
extern uint8_t ispow2_i32(int32_t) MS; extern int32_t abs_i32(int32_t) MS;
extern int32_t min_i32(int32_t,int32_t) MS; extern int32_t max_i32(int32_t,int32_t) MS;
extern int32_t clamp_i32(int32_t,int32_t,int32_t) MS;
int main(void){
 for(int i=0;i<2048;i++){
  uint32_t x,a,b;
  if(i<192){int k=i/6; uint32_t p=1u<<(k%32); switch(i%6){case 0:x=p;break;case 1:x=p-1;break;case 2:x=p+1;break;case 3:x=-p;break;case 4:x=~p;break;default:x=(p<<1)-1;break;} a=rnd(); b=rnd();}
  else if(i<200){const uint32_t s[]={0,1,0x7fffffff,0x80000000,0x80000001,0xffffffff,2,3}; x=s[i-192]; a=s[(i-192+3)&7]; b=s[(i-192+5)&7];}
  else {x=rnd(); a=rnd(); b=rnd(); if(i&1){x>>=rnd()&31;} if(i%3==0){a>>=rnd()&31; b>>=rnd()&31;} if(i%7==0){a=x; } if(i%11==0){b=x;}}
  printf("%08x\t%08x\t%08x\t%d\t%08x\t%d\t%08x\t%d\t%d\t%u\t%d\t%d\t%d\t%d\n",x,a,b,
   lzcnt_u32(x),reversebits_u32(x),ceilpow2_i32((int32_t)x),ceilpow2_u32(x),ceillog2_i32((int32_t)x),floorlog2_u32(x),
   (unsigned)ispow2_i32((int32_t)x),abs_i32((int32_t)x),min_i32((int32_t)a,(int32_t)b),max_i32((int32_t)a,(int32_t)b),clamp_i32((int32_t)x,(int32_t)a,(int32_t)b));
 }
 return 0;
}
'''
# note: ceilpow2_u32 is printed as %08x via the uint path; keep column order in sync with the Rust test.

def main():
    image, digest = load_image()
    rows = run(image, FUNCS, DRIVER)
    if len(rows) != CASES:
        raise SystemExit('incomplete original output')
    header = ('# Original GameAssembly.dll sha256=%s\n# x86_64/Rosetta replay of Unity.Mathematics.math scalar leaves\n'
              '# RVAs: ' % digest + ','.join(f'{n}={r:x}' for n, r in FUNCS) + '\n'
              '# x a b lzcnt_u32 reversebits_u32 ceilpow2_i32 ceilpow2_u32 ceillog2_i32 floorlog2_u32 ispow2_i32 abs_i32 min_i32(a,b) max_i32(a,b) clamp_i32(x,a,b)\n')
    out = ROOT / 'evidence/oracles/math_int.tsv'
    out.write_text(header + '\n'.join(rows) + '\n')
    print(f'Captured {len(rows)} cases x {len(FUNCS)} original integer math functions -> {out}')

if __name__ == '__main__':
    main()
