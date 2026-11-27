"""Isolated weighted selection replay; metadata ready, bounded blob accessors substituted.
Original instruction bytes remain process-local and are not distributed.
"""
import ctypes,hashlib,json,random,re,struct,subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;b=(root.parent/'GameAssembly.dll').read_bytes()
expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(b).hexdigest()==expected
rva=0x79381a0;size=441;code=bytearray(code_at_rva(b,rva,size));assert code[-1]==0xc3
# Supplied valid-array accessors: relative start plus index * 16 or * 8. No bounds exceptions.
stubs={}
for target,shift in [(0x6061a0,4),(0x2f09a0,3)]:
 stubs[target]=len(code);code+=bytes.fromhex('48 63 01 48 01 c8 48 63 d2 48 c1 e2')+bytes([shift])+bytes.fromhex('48 01 d0 c3')
ready=len(code);code+=b'\x01'+b'\0'*7
one=len(code);code+=struct.pack('<f',1.)
listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
for line in listing.splitlines():
 m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
 if not m:continue
 at=int(m[1],16)-0x180000000-rva;raw=bytes.fromhex(m[2]);text=m[3]
 if '%rip' in text:
  target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000
  dest=one if target==0xcf28c64 else ready
  disp=2 if raw[0] in (0x80,0xc6) else len(raw)-4
  struct.pack_into('<i',code,at+disp,dest-at-len(raw))
 if raw[0]==0xe8:
  target=at+rva+5+struct.unpack_from('<i',raw,1)[0]
  if target in stubs:struct.pack_into('<i',code,at+1,stubs[target]-at-5)
  else:assert target==0x16729f0 # skipped metadata initialization only
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
f32=lambda v:struct.unpack('<f',struct.pack('<f',v))[0]
def total(xs):
 v=0.
 for x in xs:v=f32(v+x)
 return v
try:
 ctypes.memmove(p,bytes(code),len(code));run=ctypes.CFUNCTYPE(ctypes.c_int32,ctypes.c_void_p,ctypes.c_void_p)(p)
 rng=random.Random(99712);rows=[]
 for i in range(1024):
  groups=[]
  for g in range(i%5):
   chains=[{'guid':100+g*10+c,'weight':rng.choice([0.,.25,1.,2.,5.])} for c in range((i+g)%5)]
   groups.append({'weight':rng.choice([0.,.5,1.,3.]),'chains':chains,'total_chain_weight':total([c['weight'] for c in chains])})
  blob=bytearray(16+16*len(groups));struct.pack_into('<iif',blob,0,16,len(groups),total([g['weight'] for g in groups]))
  for g,group in enumerate(groups):
   at=16+16*g;start=len(blob)
   struct.pack_into('<iiff',blob,at,start-at,len(group['chains']),group['weight'],group['total_chain_weight'])
   for c in group['chains']:blob+=struct.pack('<if',c['guid'],c['weight'])
  seed=[0,1,0xffffffff,rng.randrange(1<<32)][i%4];state=ctypes.c_uint32(seed);data=ctypes.create_string_buffer(bytes(blob));guid=run(data,ctypes.byref(state))
  rows.append({'groups':groups,'total_group_weight':total([g['weight'] for g in groups]),'seed':seed,'guid':guid,'state':state.value})
 (root/'evidence/oracles/weighted_spawn_chain.json').write_text(json.dumps({'binary_sha256':expected,'scope':'isolated selection; metadata-ready flag and bounded array accessors supplied; no exception paths','cases':rows},separators=(',',':'))+'\n')
 print('captured',len(rows),'cases')
finally:k.VirtualFree(p,0,0x8000)
