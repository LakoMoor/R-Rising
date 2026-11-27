"""Complete isolated client auto-chain clock leaves on Windows x64, no runtime stubs."""
import ctypes,hashlib,json,struct
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent;image=(root.parent/'GameAssembly.dll').read_bytes()
expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll');assert hashlib.sha256(image).hexdigest()==expected
k=ctypes.WinDLL('kernel32',use_last_error=True);k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p;k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong];alloc=[]
def load(rva,size,signature,relocation=None):
 code=bytearray(code_at_rva(image,rva,size));assert code[-1]==0xc3
 if relocation:
  at,target=relocation;dest=len(code);code+=code_at_rva(image,target,8);struct.pack_into('<i',code,at,dest-at-4)
 p=k.VirtualAlloc(None,len(code),0x3000,0x40)
 if not p:raise ctypes.WinError(ctypes.get_last_error())
 alloc.append(p);ctypes.memmove(p,bytes(code),len(code));return signature(p)
bits=lambda f:struct.unpack('<Q',struct.pack('<d',f))[0]
try:
 remaining=load(0x793d720,9,ctypes.CFUNCTYPE(ctypes.c_double,ctypes.c_void_p,ctypes.c_double))
 passed=load(0x793d730,21,ctypes.CFUNCTYPE(ctypes.c_double,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_double))
 create=load(0x793d750,55,ctypes.CFUNCTYPE(ctypes.c_uint64,ctypes.c_uint8,ctypes.c_void_p,ctypes.c_double,ctypes.c_double),(46,0xcf299f0))
 rows=[f'# client sha256={expected}; complete isolated clock leaves; relocated read-only constant; no runtime substitutions', '# enabled duration_f32_bits server_f64_bits surplus_f64_bits next_f64_bits remaining_f64_bits passed_f64_bits']
 constants=ctypes.create_string_buffer(44)
 for enabled in [0,1]:
  for duration in [-10.,-0.,0.,.1,10.,1e6]:
   struct.pack_into('<f',constants,40,duration)
   for now in [-100.,0.,1.,1000.,1e16]:
    for surplus in [-3.,0.,.1,100.]:
     next_bits=create(enabled,constants,now,surplus);instance=ctypes.c_uint64(next_bits)
     rows.append(f'{enabled}\t{struct.unpack("<I",struct.pack("<f",duration))[0]:08x}\t{bits(now):016x}\t{bits(surplus):016x}\t{next_bits:016x}\t{bits(remaining(ctypes.byref(instance),now)):016x}\t{bits(passed(ctypes.byref(instance),constants,now)):016x}')
 (root/'evidence/oracles/auto_chain_clock.tsv').write_text('\n'.join(rows)+'\n');print('captured',len(rows)-2,'cases')
 print('disabled next',rows[2].split('\t')[4])
finally:
 for p in alloc:k.VirtualFree(p,0,0x8000)
