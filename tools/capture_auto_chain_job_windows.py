"""Instrumented isolated auto job: metadata/blob helpers and ECB event sink supplied."""
import ctypes,hashlib,json,re,struct,subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;image=(root.parent/'GameAssembly.dll').read_bytes();expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(image).hexdigest()==expected
code=bytearray();functions={};extents=[(0x7eb56b0,1904),(0x7eb4cc0,879),(0x170fce0,692)]
for rva,size in extents:functions[rva]=len(code);code+=code_at_rva(image,rva,size)
for target,raw in [(0x9070b90,'c3'),(0x2eea0,'48 8b 01 c3'),(0x61ebc0,'48 63 01 48 01 c8 48 63 d2 48 6b d2 60 48 01 d0 c3'),(0x606320,'48 63 01 48 01 c8 48 63 d2 48 6b d2 2c 48 01 d0 c3'),(0x8ee5620,'48 b8 7b 00 00 00 01 00 00 00 c3')]:functions[target]=len(code);code+=bytes.fromhex(raw)
sink=len(code);functions[0x2e7d80]=sink;code+=bytes.fromhex('56 57 4c 89 ce 48 bf')+b'\0'*8+bytes.fromhex('b9 0c 00 00 00 f3 48 a5 c6 07 01 5f 5e c3')
ready=len(code);code+=b'\x01'+b'\0'*7;zero=len(code);code+=b'\0'*8;slot=len(code);code+=b'\0'*8;metadata=len(code);code+=b'\0'*40;klass=len(code);code+=b'\0'*0x136;code[klass+0x135]=1;capture=len(code);code+=b'\0'*97
for rva,size in extents:
 listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
 for line in listing.splitlines():
  m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
  if not m:continue
  relative=int(m[1],16)-0x180000000-rva;at=functions[rva]+relative;raw=bytes.fromhex(m[2]);text=m[3]
  if '%rip' in text:
   target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000
   if target in [0xdfc8d37,0xdfc8d33]:destination=ready
   elif target==0xdfa7f2c:destination=zero
   elif 0xcf20000<=target<0xcf40000:
    code+=b'\0'*((-len(code))%16);destination=len(code);code+=code_at_rva(image,target,32)
   else:destination=slot
   disp=2 if raw[0] in [0x80,0xc6,0x83] else len(raw)-4
   struct.pack_into('<i',code,at+disp,destination-at-len(raw))
  if raw[0]==0xe8:
   target=rva+relative+5+struct.unpack_from('<i',raw,1)[0]
   if target in functions:struct.pack_into('<i',code,at+1,functions[target]-at-5)
   # Remaining calls belong to excluded init/debug/domain-error branches.
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
 struct.pack_into('<Q',code,slot,p+metadata);struct.pack_into('<Q',code,metadata+32,p+klass);struct.pack_into('<Q',code,sink+7,p+capture);ctypes.memmove(p,bytes(code),len(code))
 run=ctypes.CFUNCTYPE(None,ctypes.c_void_p,ctypes.c_uint64,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p)(p+functions[0x7eb56b0]);cases=[]
 for enabled in [False,True]:
  for target_auto in [False,True]:
   for terminal in [False,True]:
    blob=bytearray(296);struct.pack_into('<iii',blob,0,16,2,1)
    for i in range(2):
     at=16+i*96;trans=208+i*44;struct.pack_into('<ii',blob,at,trans-at,1);blob[at+48]=enabled if i==0 else target_auto;blob[at+52:at+84]=bytes([i+1])*32;struct.pack_into('<iff',blob,at+84,0,.5,2. if i==0 else 3.);struct.pack_into('<i',blob,trans+32,1 if i==0 and not terminal else -1);blob[trans+40]=terminal if i==0 else 1
    data=ctypes.create_string_buffer(bytes(blob));constants=ctypes.c_void_p(ctypes.addressof(data));instance=ctypes.c_uint8(0)
    for index in [-1,0]:
     active=ctypes.create_string_buffer(struct.pack('<iii',index,88,3))
     for deadline in [-1.,0.,1.,2.,5.]:
      for now in [0.,1.,2.,3.,10.]:
       for count in [0,10]:
        job=ctypes.create_string_buffer(40);struct.pack_into('<d',job,0,now);struct.pack_into('<i',job,32,count);clock=ctypes.c_double(deadline);ctypes.memset(p+capture,0,97)
        run(job,0x200000007,ctypes.byref(clock),ctypes.byref(constants),ctypes.byref(instance),active)
        captured=ctypes.string_at(p+capture,97)
        cases.append({'enabled':enabled,'target_auto':target_auto,'terminal':terminal,'index':index,'deadline':deadline,'now':now,'count':count,'clock_bits':f'{struct.unpack("<Q",struct.pack("<d",clock.value))[0]:016x}','event':captured[:96].hex() if captured[96] else None,'count_after':struct.unpack_from('<i',job,32)[0]})
 (root/'evidence/oracles/auto_chain_job.json').write_text(json.dumps({'scope':'isolated original job/progression/remainder; metadata-ready, blob helpers, ECB allocation/event sink supplied; debug disabled and finite domain only','binary_sha256':expected,'cases':cases},separators=(',',':'))+'\n');print('captured',len(cases),'job cases')
finally:k.VirtualFree(p,0,0x8000)
