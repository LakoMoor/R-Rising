"""Isolated pinned prefab-selection oracle, with metadata/blob/prefab lookup supplied.

Extracted executable bytes remain in memory only. This is not a live ECS world.
"""
import ctypes
import hashlib
import json
import random
import re
import struct
import subprocess
from pathlib import Path
from capture_type_flags import code_at_rva

root = Path(__file__).resolve().parent.parent
image = (root.parent / 'GameAssembly.dll').read_bytes()
expected = next(row['sha256'] for row in json.loads((root / 'reference.lock.json').read_text())['files'] if row['path'] == 'GameAssembly.dll')
assert hashlib.sha256(image).hexdigest() == expected
code = bytearray()
functions = {}
extents = [(0x7ee4940, 1644), (0x793d030, 188), (0x170fce0, 692)]
for rva, size in extents:
    functions[rva] = len(code)
    code += code_at_rva(image, rva, size)
compare_type = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p)
@compare_type
def compare(a, n, b, m, unused):
    a, b = ctypes.string_at(a, n), ctypes.string_at(b, m)
    return (a > b) - (a < b)
lookup_type = ctypes.CFUNCTYPE(ctypes.c_uint8, ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)
available = set()
@lookup_type
def lookup(unused, guid, include_disabled, unused2, out, unused3):
    if guid not in available:
        return 0
    ctypes.memmove(out, struct.pack('<ii', guid + 1000, 3), 8)
    return 1
for target, callback in [(0x8ec2da0, compare), (0x8616d20, lookup)]:
    functions[target] = len(code)
    code += bytes.fromhex('48 b8') + struct.pack('<Q', ctypes.cast(callback, ctypes.c_void_p).value) + bytes.fromhex('ff e0')
for target, raw in [(0x9070b90, 'c3'), (0x605ba0, 'c3'), (0x61ebc0, '48 63 01 48 01 c8 48 63 d2 48 6b d2 60 48 01 d0 c3')]:
    functions[target] = len(code)
    code += bytes.fromhex(raw)
trap = len(code)
code += b'\xcc\xc3'
ready = len(code)
code += b'\x01' + b'\0' * 7
zero = len(code)
code += b'\0' * 8
slot = len(code)
code += b'\0' * 8
metadata = len(code)
code += b'\0' * 64
code[metadata + 56] = 1
klass = len(code)
code += b'\0' * 0x138
code[klass + 0x135] = 1
context = len(code)
code += b'\0' * 16
for rva, size in extents:
    listing = subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe', '-d', f'--start-address={0x180000000+rva:#x}', f'--stop-address={0x180000000+rva+size:#x}', str(root.parent / 'GameAssembly.dll')], capture_output=True, text=True, check=True).stdout
    for line in listing.splitlines():
        match = re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)', line)
        if not match:
            continue
        relative = int(match[1], 16) - 0x180000000 - rva
        at = functions[rva] + relative
        raw = bytes.fromhex(match[2])
        if '%rip' in match[3]:
            target = int(re.search(r'# (0x[0-9a-f]+)', match[3])[1], 16) - 0x180000000
            if target in [0xdfc8e82, 0xdfd2a74]:
                dest = ready
            elif target == 0xdfa7f2c:
                dest = zero
            elif 0xcf20000 <= target < 0xcf40000:
                code += b'\0' * ((-len(code)) % 16)
                dest = len(code)
                code += code_at_rva(image, target, 32)
            else:
                dest = slot
            offset = 2 if raw[0] in [0x80, 0xc6, 0x83] else len(raw) - 4
            struct.pack_into('<i', code, at + offset, dest - at - len(raw))
        if raw[0] == 0xe8:
            target = rva + relative + 5 + struct.unpack_from('<i', raw, 1)[0]
            struct.pack_into('<i', code, at + 1, functions.get(target, trap) - at - 5)

def fixed(text):
    raw = text.encode('utf-8')
    assert len(raw) <= 29
    return struct.pack('<H', len(raw)) + raw + b'\0' * (30 - len(raw))

k = ctypes.WinDLL('kernel32', use_last_error=True)
k.VirtualAlloc.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong, ctypes.c_ulong]
k.VirtualAlloc.restype = ctypes.c_void_p
k.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong]
p = k.VirtualAlloc(None, len(code), 0x3000, 0x40)
if not p:
    raise ctypes.WinError(ctypes.get_last_error())
try:
    struct.pack_into('<Q', code, slot, p + metadata)
    struct.pack_into('<Q', code, metadata + 32, p + klass)
    struct.pack_into('<Q', code, klass + 0xc0, p + context)
    ctypes.memmove(p, bytes(code), len(code))
    run = ctypes.CFUNCTYPE(ctypes.c_uint8, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_void_p)(p)
    rng = random.Random(99712)
    cases = []
    for case in range(400):
        count = rng.randrange(0, 8)
        transitions = [{'id': rng.choice(['grow', 'grow', 'other']), 'target': rng.randrange(3), 'importance': rng.choice([-2147483648, -2, 0, 1, 1, 5, 2147483647]), 'end': bool(rng.randrange(2))} for _ in range(count)]
        elements = [{'guid': 100 + i, 'auto': bool(rng.randrange(2)), 'duration': rng.choice([0., 0.5, 2., 8.])} for i in range(3)]
        main = rng.randrange(3)
        start = rng.randrange(3)
        forwarded = rng.choice([0., 1., 3., 20.])
        loop = bool(rng.randrange(2))
        failed = rng.choice([None, -2, 0, 2, 2147483647])
        requested = '' if case % 17 == 0 else 'grow'
        available = {element['guid'] for element in elements}
        # Missing prefab logging/exception paths are intentionally outside this oracle.
        blob = bytearray(16 + 3 * 96 + count * 44)
        struct.pack_into('<iii', blob, 0, 16, 3, main)
        for i, element in enumerate(elements):
            at = 16 + i * 96
            struct.pack_into('<ii', blob, at, 16 + 3 * 96 - at, count if i == start else 0)
            struct.pack_into('<ii', blob, at + 40, i, element['guid'])
            blob[at + 48] = element['auto']
            blob[at + 52:at + 84] = fixed('auto')
            struct.pack_into('<iff', blob, at + 84, 0, 0.5, element['duration'])
        for i, transition in enumerate(transitions):
            at = 16 + 3 * 96 + i * 44
            blob[at:at + 32] = fixed(transition['id'])
            struct.pack_into('<iiB', blob, at + 32, transition['target'], transition['importance'], transition['end'])
        data = ctypes.create_string_buffer(bytes(blob))
        event = ctypes.create_string_buffer(96)
        struct.pack_into('<id', event, 84, start, forwarded)
        instance = ctypes.c_uint8(loop)
        matrix = ctypes.create_string_buffer(struct.pack('<16f', *range(16)))
        transition_id = ctypes.create_string_buffer(fixed(requested))
        priority = ctypes.c_int(123)
        result = ctypes.create_string_buffer(b'\xa5' * 136)
        job = ctypes.create_string_buffer(0x208)
        nullable = 0 if failed is None else (1 | ((failed & 0xffffffff) << 32))
        success = bool(run(job, transition_id, event, data, ctypes.byref(instance), matrix, nullable, ctypes.byref(priority), result))
        cases.append({'elements': elements, 'transitions': transitions, 'main': main, 'start': start, 'forwarded': forwarded, 'loop': loop, 'failed': failed, 'requested': requested, 'success': success, 'importance': priority.value, 'result': result.raw[:136].hex()})
    output = {'binary_sha256': expected, 'rva': '0x7ee4940', 'scope': 'isolated original selection, ID equality and finite remainder; metadata, blob accessors and successful prefab lookup supplied; no placement/ECS scheduling', 'cases': cases}
    (root / 'evidence/oracles/transition_selection.json').write_text(json.dumps(output, separators=(',', ':')) + '\n')
    print('captured', len(cases), 'selection cases')
finally:
    k.VirtualFree(p, 0, 0x8000)
