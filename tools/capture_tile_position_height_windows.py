"""Isolated Windows x64 replay of pinned TilePosition height getter leaves.
No game startup, runtime hooks or original instruction bytes are distributed.
"""
import ctypes, hashlib, json, re, struct, subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent
image=(root.parent/'GameAssembly.dll').read_bytes()
expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll')
assert hashlib.sha256(image).hexdigest()==expected
kernel=ctypes.WinDLL('kernel32',use_last_error=True)
kernel.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong]; kernel.VirtualAlloc.restype=ctypes.c_void_p
kernel.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
allocations=[]
def leaf(rva,size,result):
 code=bytearray(code_at_rva(image,rva,size)); assert code[-1]==0xc3
 listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
 for line in listing.splitlines():
  m=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
  if not m: continue
  at=int(m[1],16)-0x180000000-rva; raw=bytes.fromhex(m[2]); text=m[3]
  assert 'call' not in text
  if '%rip' in text:
   target=int(re.search(r'# (0x[0-9a-f]+)',text)[1],16)-0x180000000
   destination=len(code); code+=code_at_rva(image,target,4)
   struct.pack_into('<i',code,at+len(raw)-4,destination-at-len(raw))
 ptr=kernel.VirtualAlloc(None,len(code),0x3000,0x40)
 if not ptr: raise ctypes.WinError(ctypes.get_last_error())
 allocations.append(ptr); ctypes.memmove(ptr,bytes(code),len(code))
 return ctypes.CFUNCTYPE(result,ctypes.c_void_p)(ptr)
try:
 height=leaf(0x794a1b0,28,ctypes.c_float); level=leaf(0x794a130,113,ctypes.c_uint32)
 value=ctypes.create_string_buffer(12)
 lines=[f'# pinned client sha256={expected}; isolated complete leaf replay; no runtime substitutions', '# compressed_height height_f32_bits height_level']
 for n in range(65536):
  struct.pack_into('<H',value,10,n)
  h=height(value); l=level(value)
  lines.append(f'{n}\t{struct.unpack("<I",struct.pack("<f",h))[0]:08x}\t{l}')
 (root/'evidence/oracles/tile_position_height.tsv').write_text('\n'.join(lines)+'\n')
 print(f'captured {len(lines)-2} exhaustive inputs')
finally:
 for ptr in allocations: kernel.VirtualFree(ptr,0,0x8000)
