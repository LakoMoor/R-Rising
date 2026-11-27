"""Isolated original event equality; ready metadata and valid UTF8 byte comparator supplied."""
import ctypes,hashlib,json,re,struct,subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;image=(root.parent/'GameAssembly.dll').read_bytes();expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(image).hexdigest()==expected
code=bytearray();functions={};extents=[(0x793c970,204),(0x793d030,188)]
for rva,size in extents:functions[rva]=len(code);code+=code_at_rva(image,rva,size)
ready=len(code);code+=b'\x01'+b'\0'*7;slot=len(code);code+=b'\0'*8;metadata=len(code);code+=b'\0'*64;code[metadata+56]=1
compare_type=ctypes.CFUNCTYPE(ctypes.c_int,ctypes.c_void_p,ctypes.c_int,ctypes.c_void_p,ctypes.c_int,ctypes.c_void_p)
@compare_type
def compare(left,n,right,m,unused):
 a=ctypes.string_at(left,n);b=ctypes.string_at(right,m);return (a>b)-(a<b)
callback=ctypes.cast(compare,ctypes.c_void_p).value
for rva,size in extents:
 listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
 for line in listing.splitlines():
  m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
  if not m:continue
  relative=int(m[1],16)-0x180000000-rva;at=functions[rva]+relative;raw=bytes.fromhex(m[2]);text=m[3]
  if '%rip' in text:
   target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000;dest=ready if target==0xdfd2a74 else slot;disp=2 if raw[0] in [0x80,0xc6] else len(raw)-4;struct.pack_into('<i',code,at+disp,dest-at-len(raw))
  if raw[0]==0xe8:
   target=rva+relative+5+struct.unpack_from('<i',raw,1)[0]
   if target in functions:dest=functions[target]
   elif target==0x8ec2da0:dest=len(code);code+=bytes.fromhex('48 b8')+struct.pack('<Q',callback)+bytes.fromhex('ff e0')
   else:assert target in [0x16729f0,0x163a730];continue
   struct.pack_into('<i',code,at+1,dest-at-5)
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
 struct.pack_into('<Q',code,slot,p+metadata);ctypes.memmove(p,bytes(code),len(code));run=ctypes.CFUNCTYPE(ctypes.c_uint8,ctypes.c_void_p,ctypes.c_void_p)(p);cases=[]
 base=bytearray(96);struct.pack_into('<H',base,0,4);base[2:6]=b'grow';struct.pack_into('<iiiiii',base,64,7,2,88,3,0,1)
 variants=[bytearray(base)]
 for i in range(96):
  if i in [0,1,32,33]:continue
  value=bytearray(base);value[i]^=1;variants.append(value)
 for n in range(30):
  value=bytearray(base);value[:32]=b'\0'*32;struct.pack_into('<H',value,0,n);value[2:2+n]=b'x'*n;variants.append(value)
 for value in variants:
  a=ctypes.create_string_buffer(bytes(base));b=ctypes.create_string_buffer(bytes(value));cases.append({'left':base.hex(),'right':value.hex(),'equal':bool(run(a,b))})
 (root/'evidence/oracles/chain_event_key.json').write_text(json.dumps({'scope':'isolated equality; metadata-ready and valid byte comparator supplied; malformed strings excluded','binary_sha256':expected,'cases':cases},separators=(',',':'))+'\n');print('captured',len(cases),'keys')
finally:k.VirtualFree(p,0,0x8000)
