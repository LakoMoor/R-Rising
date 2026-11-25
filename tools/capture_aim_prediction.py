#!/usr/bin/env python3
"""Replay the original AimPrediction.FirstOrderIntercept and FirstOrderInterceptTime.

Original x86-64 bytes (client RVAs 0x7f8f010 and 0x7f8ef70) run under Rosetta with the Windows x64
calling convention. The only substitution is the sqrt wrapper (sqrtss stub). Constants are read from
the pinned image. This is a controlled function replay, not original game execution.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

ENTRIES = [('intercept_time', 0x7f8f010), ('intercept', 0x7f8ef70)]
STUBS = {0x1b28e70: ('sqrt_stub', 'sqrtss %xmm0, %xmm0\nret')}

DRIVER = XORSHIFT + r'''
#define MS __attribute__((ms_abi))
extern float intercept_time(float,uint64_t,uint64_t,void*) MS;
extern uint64_t intercept(uint64_t,uint64_t,float,uint64_t,uint64_t,void*) MS;
static float f(uint32_t b){float v;memcpy(&v,&b,4);return v;}
static uint32_t u(float v){uint32_t b;memcpy(&b,&v,4);return b;}
static uint64_t pair(float x,float y){return ((uint64_t)u(y)<<32)|u(x);}
static float rf(float scale){return ((int32_t)(rnd()%200001)-100000)/100000.0f*scale;}
int main(void){
 const uint32_t special[]={0,0x80000000,0x7f800000,0xff800000,0x7fc01234,0x00000001,0x3f800000,0xbf800000,0x358637bd,0x5f800000};
 for(int i=0;i<4096;i++){
  float sp,px,py,vx,vy,sx,sy,svx,svy,tx,ty,tvx,tvy;
  sp=fabsf(rf(30.0f)); px=rf(50.0f);py=rf(50.0f);vx=rf(10.0f);vy=rf(10.0f);
  sx=rf(50.0f);sy=rf(50.0f);svx=rf(10.0f);svy=rf(10.0f);tx=rf(50.0f);ty=rf(50.0f);tvx=rf(10.0f);tvy=rf(10.0f);
  if(i%7==0){vx=svx;vy=svy;sp=sqrtf(vx*vx+vy*vy);}              /* equal speeds */
  if(i%11==0){vx=0;vy=0;}                                       /* static target */
  if(i%13==0){px=py=0;}                                          /* coincident */
  if(i>=2048){float *a[]={&sp,&px,&py,&vx,&vy};for(int k=0;k<5;k++)if(rnd()%3==0)*a[k]=f(special[rnd()%10]);}
  float t=intercept_time(sp,pair(px,py),pair(vx,vy),0);
  uint64_t r=intercept(pair(sx,sy),pair(svx,svy),sp,pair(tx,ty),pair(tvx,tvy),0);
  printf("%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\t%08x\n",
   u(sp),u(px),u(py),u(vx),u(vy),u(t),u(sx),u(sy),u(svx),u(svy),u(tx),u(ty),u(tvx),u(tvy),0);
  printf("R\t%08x\t%08x\n",(uint32_t)r,(uint32_t)(r>>32));
 }
 return 0;}
'''


def main():
    image, digest = load_image()
    driver = DRIVER.replace('#include <string.h>', '#include <string.h>\n#include <math.h>')
    rows = run(image, ENTRIES, driver, stubs=STUBS)
    if len(rows) != 8192:
        raise SystemExit(f'incomplete capture: {len(rows)} rows')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# Controlled x86_64/Rosetta replay of AimPrediction (RVAs 7f8f010, 7f8ef70); sqrt wrapper stubbed\n'
              '# row A: speed relPos.xy relVel.xy time | shooterPos shooterVel targetPos targetVel (f32 hex); row R: intercept xy\n')
    (ROOT / 'evidence/oracles/aim_prediction.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'aim_prediction.tsv: {len(rows) // 2} cases')


if __name__ == '__main__':
    main()
