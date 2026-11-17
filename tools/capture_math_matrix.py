#!/usr/bin/env python3
"""Replay original Unity.Mathematics float3x3/float4x4/quaternion leaves (see oracle_replay.py).

The functions are straight-line code. Two of them call other captured functions (the quaternion
ctor of float3x3), which the harness links to each other. quaternion normalize calls the il2cpp
`math.sqrt(float)` wrapper, which is not a leaf (lazy static initialisation); it is replaced by a
`sqrtss` stub here (the original computes the double sqrt and rounds to float, which is the same
correctly rounded result for every finite non-negative input).
"""
from oracle_replay import ROOT, load_image, run, XORSHIFT

FUNCS = [('q2m3', 0x9338770), ('q2m4', 0x934f920), ('trs', 0x9351b90), ('translate', 0x9351390),
         ('scale3', 0x9351310), ('mul44', 0x92bfb20), ('qmul', 0x92ccc50), ('qnorm', 0x92cc510)]
STUBS = {0x1b28e70: ('sqrt_stub', '\tsqrtss %xmm0, %xmm0\n\tret')}
N = 2048

DRIVER = XORSHIFT + r'''
#include <math.h>
#define MS __attribute__((ms_abi))
extern void q2m3(float*,const float*) MS; extern void q2m4(float*,const float*,const float*) MS;
extern void trs(float*,const float*,const float*,const float*) MS;
extern void translate(float*,const float*) MS; extern void scale3(float*,const float*) MS;
extern void mul44(float*,const float*,const float*) MS; extern void qmul(float*,const float*,const float*) MS;
extern void qnorm(float*,const float*) MS;
static float f(uint32_t b){float x;memcpy(&x,&b,4);return x;}
static uint32_t u(float x){uint32_t b;memcpy(&b,&x,4);return b;}
static double unit(void){return (rnd()>>8)/16777216.0;}
static const float SPEC[]={0.0f,-0.0f,1.0f,-1.0f,0.5f,-0.5f,0.70710677f,-0.70710677f,0.8660254f,0.25881904f,0.9659258f};
static void quat(int kind,float*q){
 switch(kind){
 case 0:{double a[4],n=0;for(int i=0;i<4;i++){a[i]=unit()*2-1;n+=a[i]*a[i];}n=sqrt(n);for(int i=0;i<4;i++)q[i]=(float)(a[i]/n);break;}
 case 1:{double ang=unit()*6.2831853;int ax=rnd()%3;q[0]=q[1]=q[2]=(rnd()&1)?0.0f:-0.0f;q[ax]=(float)sin(ang/2);q[3]=(float)cos(ang/2);break;}
 case 2:{for(int i=0;i<4;i++)q[i]=SPEC[rnd()%11];break;}
 case 3:{for(int i=0;i<4;i++)q[i]=(float)(unit()*4-2);break;}
 case 4:{double a[4],n=0;for(int i=0;i<4;i++){a[i]=unit()*2-1;n+=a[i]*a[i];}n=sqrt(n);for(int i=0;i<4;i++)q[i]=(float)(a[i]/n);
   if(rnd()&1)q[rnd()%3]=0.0f;if(rnd()&1)q[3]=fabsf(q[3]);break;}
 default:{for(int i=0;i<4;i++)q[i]=f(rnd());if(!isfinite(q[0]+q[1]+q[2]+q[3])){q[0]=1;}break;}}
}
static void vec(int kind,float*v){
 for(int i=0;i<3;i++){switch(kind){case 0:v[i]=0.0f;break;case 1:v[i]=(float)(unit()*2000-1000);break;
  case 2:v[i]=(float)((int)(rnd()%4001)-2000)/8.0f;break;case 3:v[i]=(float)(unit()*3+0.5);break;
  case 4:v[i]=1.0f;break;default:v[i]=(rnd()&1)?-0.0f:(float)(unit()*20-10);}}
}
int main(void){
 static float __attribute__((aligned(16))) m[16],n[16],o[16],t[4],q[4],q2[4],s[4],a[16],b[16],r[16],r2[16],r3[16],p[16],m3[16],tm[16],sm[16];
 for(int i=0;i<2048;i++){
  int qk=i%6; quat(qk,q); quat((i/6)%6,q2);
  int tk=(i%7==0)?0:(i%3==0)?2:1; vec(tk,t);
  int sk=(i%2==0)?4:3; vec(sk,s); if(i%5==0){s[1]=s[0];s[2]=s[0];} if(i%2==0&&i%8==0){s[0]=s[1]=s[2]=(float)(unit()*3+0.5);}
  if(i<16){for(int k=0;k<4;k++)q[k]=SPEC[(i+k*3)%11];}
  memset(m3,0,sizeof m3); q2m3(m3,q); q2m4(m,q,t); trs(r,t,q,s); translate(tm,t); scale3(sm,s); qnorm(o,q);
  qmul(n,q,q2);
  /* pipeline translate * (rotate * scale), rotate = float4x4(q, 0) */
  float z[4]={0,0,0,0}; q2m4(r2,q,z); mul44(r3,r2,sm); mul44(p,tm,r3);
  /* mul operands: scene-shaped (TRS of this and the next-row-ish transform) or general matrices */
  if(i%4<3){ float t2[4],qq[4],s2[4]; quat(rnd()%6,qq);vec(1,t2);vec(rnd()%2?4:3,s2); trs(a,t,q,s); trs(b,t2,qq,s2);}
  else {for(int k=0;k<16;k++){a[k]=f(rnd());b[k]=(float)(unit()*20-10); if(!isfinite(a[k]))a[k]=1;a[k]=(float)((int)(rnd()%2001)-1000)/16.0f;}}
  float mm[16] __attribute__((aligned(16))); mul44(mm,a,b);
  printf("%08x\t%08x\t%08x\t%08x",u(t[0]),u(t[1]),u(t[2]),0);
  for(int k=0;k<4;k++)printf("\t%08x",u(q[k])); for(int k=0;k<4;k++)printf("\t%08x",u(q2[k])); for(int k=0;k<3;k++)printf("\t%08x",u(s[k]));
  for(int k=0;k<16;k++)printf("\t%08x",u(a[k])); for(int k=0;k<16;k++)printf("\t%08x",u(b[k]));
  printf("\t|");
  for(int k=0;k<9;k++)printf("\t%08x",u(m3[k]));
  float tr[16]; trs(tr,t,q,s); float qn[4]; qnorm(qn,q); float qm[4]; qmul(qm,q,q2);
  for(int k=0;k<16;k++)printf("\t%08x",u(m[k])); for(int k=0;k<16;k++)printf("\t%08x",u(tr[k]));
  for(int k=0;k<16;k++)printf("\t%08x",u(tm[k])); for(int k=0;k<16;k++)printf("\t%08x",u(sm[k]));
  for(int k=0;k<16;k++)printf("\t%08x",u(p[k])); for(int k=0;k<16;k++)printf("\t%08x",u(mm[k]));
  for(int k=0;k<4;k++)printf("\t%08x",u(qn[k])); for(int k=0;k<4;k++)printf("\t%08x",u(qm[k]));
  printf("\n");
 }return 0;}
'''

def main():
    image, digest = load_image()
    rows = run(image, FUNCS, DRIVER, stubs=STUBS)
    if len(rows) != N:
        raise SystemExit('incomplete original output')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# x86_64/Rosetta replay of Unity.Mathematics matrix/quaternion leaves; floats as IEEE bits\n'
              '# RVAs: ' + ','.join(f'{n}={r:x}' for n, r in FUNCS) + ' (sqrt stubbed by sqrtss)\n'
              '# inputs: t(3) pad q(4) q2(4) s(3) a(16) b(16) | outputs: float3x3(q)(9) float4x4(q,t)(16) TRS(t,q,s)(16) '
              'Translate(t)(16) Scale(s)(16) Translate*(float4x4(q,0)*Scale)(16) mul(a,b)(16) normalize(q)(4) mul(q,q2)(4)\n')
    (ROOT / 'evidence/oracles/math_matrix.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'Captured {len(rows)} cases x {len(FUNCS)} original matrix/quaternion functions')

if __name__ == '__main__':
    main()
