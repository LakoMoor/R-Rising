"""Isolated finite nonzero-divisor remainder helper; non-FMA CPU dispatch supplied."""
import ctypes,hashlib,json,re,struct,subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;image=(root.parent/'GameAssembly.dll').read_bytes();expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(image).hexdigest()==expected
rva=0x170fce0;size=692;code=bytearray(code_at_rva(image,rva,size));assert code[-1]==0xc3
listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
for line in listing.splitlines():
 m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
 if not m:continue
 at=int(m[1],16)-0x180000000-rva;raw=bytes.fromhex(m[2]);text=m[3]
 if '%rip' in text:
  target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000
  code+=b'\0'*((-len(code))%16);destination=len(code)
  code+=b'\0'*32 if target==0xdfa7f2c else code_at_rva(image,target,32)
  disp=2 if raw[0]==0x83 else len(raw)-4
  struct.pack_into('<i',code,at+disp,destination-at-len(raw))
 if raw[0]==0xe8:assert rva+at+5+struct.unpack_from('<i',raw,1)[0]==0x17036d0 # excluded domain-error branch
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
 ctypes.memmove(p,bytes(code),len(code));run=ctypes.CFUNCTYPE(ctypes.c_double,ctypes.c_double,ctypes.c_double)(p)
 bits=lambda f:struct.unpack('<Q',struct.pack('<d',f))[0]
 values=[-1e300,-100.,-1.,-.1,-0.,0.,5e-324,1e-300,.1,.25,1.,10.,100.,1e300]
 divisors=[-100.,-1.,-.1,5e-324,1e-300,.1,.25,1.,10.,100.,1e300]
 rows=[f'# client sha256={expected}; original helper RVA 0x170fce0; CPU dispatch=0; finite dividend and finite nonzero divisor only', '# dividend_bits divisor_bits remainder_bits']
 for value in values:
  for divisor in divisors:rows.append(f'{bits(value):016x}\t{bits(divisor):016x}\t{bits(run(value,divisor)):016x}')
 (root/'evidence/oracles/auto_chain_remainder.tsv').write_text('\n'.join(rows)+'\n');print('captured',len(rows)-2,'cases')
finally:k.VirtualFree(p,0,0x8000)
