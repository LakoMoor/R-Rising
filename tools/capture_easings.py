#!/usr/bin/env python3
"""Replay 19 original Motion2DEasings functions (double t, b, c, d) from the pinned client image.

Covers the polynomial, back and bounce families (client RVAs below). Sine/expo/circ/elastic call
the IL2CPP math wrappers and are not captured here. inBounce/inOutBounce call outBounce, which is
linked from the same capture. Controlled replay under Rosetta, not game execution.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

NAMES = {0x7c56550: 'linear', 0x7c56560: 'inQuad', 0x7c56580: 'outQuad', 0x7c565b0: 'inOutQuad',
         0x7c56610: 'inCubic', 0x7c56630: 'outCubic', 0x7c56660: 'inOutCubic', 0x7c566e0: 'inQuart',
         0x7c56700: 'outQuart', 0x7c56740: 'inOutQuart', 0x7c567d0: 'inQuint', 0x7c56800: 'outQuint',
         0x7c56840: 'inOutQuint', 0x7c57600: 'inBack', 0x7c57630: 'outBack', 0x7c57670: 'inOutBack',
         0x7c57710: 'inBounce', 0x7c57760: 'outBounce', 0x7c57830: 'inOutBounce'}
ENTRIES = [(f'e_{n}', rva) for rva, n in NAMES.items()]

DRIVER = XORSHIFT + '#define MS __attribute__((ms_abi))\n' + ''.join(
    f'extern double e_{n}(double,double,double,double) MS;\n' for n in NAMES.values()) + r'''
static double rd(double s){return ((int32_t)(rnd()%2000001)-1000000)/1000000.0*s;}
static uint64_t u(double v){uint64_t b;memcpy(&b,&v,8);return b;}
int main(void){
 for(int i=0;i<2048;i++){
  double d=1.0+(rnd()%1000)/100.0, t=((rnd()%1200)-100)/1000.0*d, b=rd(100), c=rd(100);
  if(i%64==0)d=0;
  printf("%016llx\t%016llx\t%016llx\t%016llx",(unsigned long long)u(t),(unsigned long long)u(b),(unsigned long long)u(c),(unsigned long long)u(d));
''' + ''.join(f'  printf("\\t%016llx",(unsigned long long)u(e_{n}(t,b,c,d)));\n' for n in NAMES.values()) + r'''
  printf("\n");
 }
 return 0;}
'''


def main():
    image, digest = load_image()
    rows = run(image, ENTRIES, DRIVER)
    if len(rows) != 2048:
        raise SystemExit(f'incomplete capture: {len(rows)} rows')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# Controlled x86_64/Rosetta replay of Motion2DEasings (doubles); no stubs\n'
              '# RVAs: ' + ' '.join(f'{n}={rva:x}' for rva, n in NAMES.items()) + '\n'
              '# columns (f64 hex): t b c d | results in the RVA order above\n')
    (ROOT / 'evidence/oracles/easings.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'easings.tsv: {len(rows)} cases')


if __name__ == '__main__':
    main()
