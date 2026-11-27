"""Replay original spawn wrapper + TRS + quaternion matrix; supplied instantiate/ECB/attachment sinks."""
import ctypes
import hashlib
import json
import random
import re
import struct
import subprocess
from pathlib import Path
from capture_type_flags import code_at_rva

root=Path(__file__).resolve().parent.parent
image=(root.parent/'GameAssembly.dll').read_bytes()
expected=next(row['sha256'] for row in json.loads((root/'reference.lock.json').read_text())['files'] if row['path']=='GameAssembly.dll')
assert hashlib.sha256(image).hexdigest()==expected
code=bytearray();functions={};callbacks=[];trace=[]
extents=[(0x7ee1740,644),(0x5a7af10,488),(0x9338770,663)]
for rva,size in extents:
    functions[rva]=len(code);code+=code_at_rva(image,rva,size)
ptr=ctypes.c_void_p;u64=ctypes.c_uint64;i32=ctypes.c_int
def callback(target,result,args,body):
    fn=ctypes.CFUNCTYPE(result,*args)(body);callbacks.append(fn)
    functions[target]=len(code);code.extend(bytes.fromhex('48 b8')+struct.pack('<Q',ctypes.cast(fn,ptr).value)+bytes.fromhex('ff e0'))
def instantiate(command,prefab,method):
    trace.append({'op':'instantiate','prefab':prefab,'child':0x300000058})
    return 0x300000058
callback(0x8edb420,u64,[ptr,u64,ptr],instantiate)
for target,name,size in [(0xbfa30,'translation',12),(0xc3970,'rotation',16),(0xbf200,'local_transform',32),(0xc2f10,'local_to_world',64)]:
    def sink(command,entity,data,method,name=name,size=size):
        trace.append({'op':name,'entity':entity,'bytes':ctypes.string_at(data,size).hex()})
    callback(target,None,[ptr,u64,ptr,ptr],sink)
def attach(chain,index,child,prefab,command,config,forwarded,datas,method):
    trace.append({'op':'attach','chain':chain,'index':index,'child':child,'prefab':prefab,'auto':ctypes.string_at(config,48).hex(),'forwarded':struct.pack('<d',forwarded).hex(),'server_time':ctypes.string_at(datas,8).hex()})
callback(0x7ee19d0,None,[u64,i32,u64,u64,ptr,ptr,ctypes.c_double,ptr,ptr],attach)
trap=len(code);code+=b'\xcc\xc3'
ready=len(code);code+=b'\x01'+b'\0'*7
slot=len(code);code+=b'\0'*8
metadata=len(code);code+=b'\0'*0xe8;code[metadata+0xe0]=1
for rva,size in extents:
    listing=subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe','-d',f'--start-address={0x180000000+rva:#x}',f'--stop-address={0x180000000+rva+size:#x}',str(root.parent/'GameAssembly.dll')],capture_output=True,text=True,check=True).stdout
    for line in listing.splitlines():
        match=re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)',line)
        if not match:continue
        relative=int(match[1],16)-0x180000000-rva;at=functions[rva]+relative;raw=bytes.fromhex(match[2])
        if '%rip' in match[3]:
            target=int(re.search(r'# (0x[0-9a-f]+)',match[3])[1],16)-0x180000000
            if target==0xdfc8e75:dest=ready
            elif target==0xcf28c64:
                dest=len(code);code+=code_at_rva(image,target,4)
            else:dest=slot
            offset=2 if raw[0] in [0x80,0xc6,0x83] else len(raw)-4
            struct.pack_into('<i',code,at+offset,dest-at-len(raw))
        if raw[0]==0xe8:
            target=rva+relative+5+struct.unpack_from('<i',raw,1)[0]
            struct.pack_into('<i',code,at+1,functions.get(target,trap)-at-5)
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.VirtualAlloc.argtypes=[ptr,ctypes.c_size_t,ctypes.c_ulong,ctypes.c_ulong];k.VirtualAlloc.restype=ptr;k.VirtualFree.argtypes=[ptr,ctypes.c_size_t,ctypes.c_ulong]
p=k.VirtualAlloc(None,len(code),0x3000,0x40)
if not p:raise ctypes.WinError(ctypes.get_last_error())
try:
    struct.pack_into('<Q',code,slot,p+metadata);ctypes.memmove(p,bytes(code),len(code))
    run=ctypes.CFUNCTYPE(None,u64,u64,i32,ptr,ptr,ptr,ptr,ctypes.c_double,ptr,ptr)(p)
    rng=random.Random(99712);cases=[]
    for row in range(256):
        position=[rng.uniform(-1000,1000) for _ in range(3)]
        rotation=[rng.uniform(-2,2) for _ in range(4)] if row%3 else [0.,0.,0.,1.]
        if row%11==0:position=[-0.,0.,-0.]
        position_bytes=struct.pack('<3f',*position);rotation_bytes=struct.pack('<4f',*rotation)
        translation=ctypes.create_string_buffer(position_bytes);quaternion=ctypes.create_string_buffer(rotation_bytes)
        config=ctypes.create_string_buffer(48);config[0]=row%2
        if row%2:struct.pack_into('<f',config,44,2.)
        command=ctypes.create_string_buffer(32);datas=ctypes.create_string_buffer(struct.pack('<d',5.)+b'\0'*200)
        trace=[];index=rng.choice([-1,0,8]);forwarded=rng.choice([-2.,0.,1.,200.])
        run(0x200000007,0x100000063,index,translation,quaternion,command,config,forwarded,datas,None)
        cases.append({'position':position_bytes.hex(),'rotation':rotation_bytes.hex(),'index':index,'forwarded':forwarded,'auto':bool(row%2),'trace':trace})
    (root/'evidence/oracles/chain_spawn.json').write_text(json.dumps({'binary_sha256':expected,'rvas':[hex(rva) for rva,_ in extents],'scope':'isolated spawn wrapper, original TRS and quaternion matrix; metadata ready; instantiate, component writes and attachment are supplied sinks; no ECB playback','cases':cases},separators=(',',':'))+'\n')
    print('captured',len(cases),'spawn traces')
finally:k.VirtualFree(p,0,0x8000)
