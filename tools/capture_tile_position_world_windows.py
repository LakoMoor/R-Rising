"""Pinned TilePosition baked-to-world replay; metadata initialization flag supplied as ready.
Temporary code is local to this process; no game startup or original code is distributed.
"""
import ctypes, hashlib, json, random, struct
from pathlib import Path
from capture_type_flags import code_at_rva
root=Path(__file__).resolve().parent.parent
image=(root.parent/'GameAssembly.dll').read_bytes()
expected=next(x['sha256'] for x in json.loads((root/'reference.lock.json').read_text())['files'] if x['path']=='GameAssembly.dll')
assert hashlib.sha256(image).hexdigest()==expected
code=bytearray(code_at_rva(image,0x794a3b0,217));assert code[-1]==0xc3
flag=len(code);code.append(1)
for at,disp,length,prefix in [(0x82,2,7,b'\x80\x3d'),(0x98,3,7,b'\x48\x8d\x0d'),(0xa9,2,7,b'\xc6\x05')]:
 assert code[at:at+len(prefix)]==prefix
 struct.pack_into('<i',code,at+disp,flag-at-length)
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.VirtualAlloc.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ctypes.c_void_p
k.VirtualFree.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
 ctypes.memmove(p,bytes(code),len(code))
 run=ctypes.CFUNCTYPE(ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint64,ctypes.c_uint8)(p)
 out=ctypes.create_string_buffer(12); source=ctypes.create_string_buffer(12)
 random.seed(99712)
 rows=[f'# pinned client sha256={expected}; RVA 0x794a3b0 complete function, metadata initialized flag=1; no game runtime', '# x z rotation height offset_x offset_z chunk_rotation out_x out_z out_rotation out_height']
 values=[-2147483648,-2147483647,-160,-1,0,1,160,2147483647]
 for rotation in range(256):
  for i in range(16):
   x=random.choice(values);z=random.choice(values);ox=random.choice(values);oz=random.choice(values);r=random.randrange(256);height=random.randrange(65536)
   struct.pack_into('<iiBxH',source,0,x,z,r,height)
   offset=(ox&0xffffffff)|((oz&0xffffffff)<<32)
   run(out,source,offset,rotation)
   a,b,c,d=struct.unpack_from('<iiBxH',out)
   rows.append(f'{x}\t{z}\t{r}\t{height}\t{ox}\t{oz}\t{rotation}\t{a}\t{b}\t{c}\t{d}')
 (root/'evidence/oracles/tile_position_world.tsv').write_text('\n'.join(rows)+'\n')
 print(f'captured {len(rows)-2} cases')
finally:k.VirtualFree(p,0,0x8000)
