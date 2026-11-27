"""Isolated automatic progression original replay; supplied metadata-ready and valid blob accessors."""
import ctypes,hashlib,json,re,struct,subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;image=(root.parent/'GameAssembly.dll').read_bytes();expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(image).hexdigest()==expected
rva=0x7eb4cc0;size=879;code=bytearray(code_at_rva(image,rva,size))
stubs={}
for target,raw in [(0x9070b90,'c3'),(0x2eea0,'48 8b 01 c3'),(0x61ebc0,'48 63 01 48 01 c8 48 63 d2 48 6b d2 60 48 01 d0 c3'),(0x606320,'48 63 01 48 01 c8 48 63 d2 48 6b d2 2c 48 01 d0 c3')]:
 stubs[target]=len(code);code+=bytes.fromhex(raw)
ready=len(code);code+=b'\x01'+b'\0'*7
slot=len(code);code+=b'\0'*8
metadata=len(code);code+=b'\0'*40
klass=len(code);code+=b'\0'*0x136;code[klass+0x135]=1
listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
for line in listing.splitlines():
 m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
 if not m:continue
 at=int(m[1],16)-0x180000000-rva;raw=bytes.fromhex(m[2]);text=m[3]
 if '%rip' in text:
  target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000
  dest=ready if target==0xdfc8d33 else slot
  disp=2 if raw[0] in (0x80,0xc6) else len(raw)-4
  struct.pack_into('<i',code,at+disp,dest-at-len(raw))
 if raw[0]==0xe8:
  target=rva+at+5+struct.unpack_from('<i',raw,1)[0]
  if target in stubs:struct.pack_into('<i',code,at+1,stubs[target]-at-5)
  else:assert target in [0x16729f0,0x163a6b0,rva]
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
 struct.pack_into('<Q',code,slot,p+metadata);struct.pack_into('<Q',code,metadata+32,p+klass);ctypes.memmove(p,bytes(code),len(code))
 run=ctypes.CFUNCTYPE(ctypes.c_uint8,ctypes.c_int32,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint8,ctypes.c_uint8,ctypes.c_double,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p)(p);cases=[]
 for n in [1,2,4]:
  for mode in range(2):
   elements=[{'enabled':mode==0 or i%2==0,'duration':float(i+1)/4,'target':i+1 if i+1<n else -1,'end':i+1==n} for i in range(n)]
   main=n//2
   blob=bytearray(16+n*96+n*44);struct.pack_into('<iii',blob,0,16,n,main)
   for i,e in enumerate(elements):
    at=16+i*96;trans=16+n*96+i*44
    struct.pack_into('<ii',blob,at,trans-at,1);blob[at+48]=e['enabled'];blob[at+52:at+84]=bytes([i+1])*32
    struct.pack_into('<i',blob,at+84,0);struct.pack_into('<f',blob,at+92,e['duration']);struct.pack_into('<i',blob,trans+32,e['target']);blob[trans+40]=e['end']
   data=ctypes.create_string_buffer(bytes(blob))
   for loop in [False,True]:
    instance=ctypes.c_uint8(loop)
    for stop in [False,True]:
     for skip in [False,True]:
      for surplus in [-1.,0.,.25,1.,10.]:
       for current in [-1]+list(range(n))+[n]:
        origin=ctypes.c_int32(-99);target=ctypes.c_int32(-99);event=ctypes.create_string_buffer(32);duration=ctypes.c_double(-999.)
        success=bool(run(current,data,ctypes.byref(instance),stop,skip,surplus,ctypes.byref(origin),ctypes.byref(target),event,ctypes.byref(duration)))
        cases.append({'main':main,'elements':elements,'current':current,'loop':loop,'stop':stop,'skip':skip,'surplus':surplus,'success':success,'origin':origin.value,'target':target.value,'event':event.raw.hex(),'duration_bits':f'{struct.unpack("<Q",struct.pack("<d",duration.value))[0]:016x}'})
 (root/'evidence/oracles/auto_chain_progress.json').write_text(json.dumps({'scope':'isolated original recursive auto progression; metadata and blob accessors supplied; array exceptions excluded','binary_sha256':expected,'cases':cases},separators=(',',':'))+'\n');print('captured',len(cases),'progressions')

finally:k.VirtualFree(p,0,0x8000)
