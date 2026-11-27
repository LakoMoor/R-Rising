"""Original essential-attachment replay; supplied ECS/ECB boundary, optional static branches."""
import ctypes
import argparse
import hashlib
import itertools
import json
import random
import re
import struct
import subprocess
from pathlib import Path
from capture_type_flags import code_at_rva
from probe_type_registry import Probe

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--static', action='store_true', help='capture static and mega-static attachment branches')
static_mode = parser.parse_args().static
image = (root.parent / 'GameAssembly.dll').read_bytes()
expected = next(x['sha256'] for x in json.loads((root / 'reference.lock.json').read_text())['files'] if x['path'] == 'GameAssembly.dll')
assert hashlib.sha256(image).hexdigest() == expected
# Resolve the two generic ReadOnly<T> requests from pinned metadata, not guessed names.
metadata_path = root.parent / 'VRising_Data/il2cpp_data/Metadata/global-metadata.dat'
metadata_hash = next(x['sha256'] for x in json.loads((root / 'reference.lock.json').read_text())['files'] if x['path'].endswith('global-metadata.dat'))
assert hashlib.sha256(metadata_path.read_bytes()).hexdigest() == metadata_hash
probe = Probe(root.parent / 'GameAssembly.dll', metadata_path)
registration = struct.unpack_from('<16Q', probe.binary, probe.registration_offset)
for slot_rva, expected_type in [(0xda536a0, 'Unity.Entities.Disabled'), (0xd9deed0, 'ProjectM.SpawnChainChild'), (0xdac2320, 'ProjectM.GuaranteedStaticTransform'), (0xda65508, 'Unity.Transforms.Static'), (0xda95330, 'ProjectM.MegaStaticTag')]:
    encoded = struct.unpack('<Q', code_at_rva(image, slot_rva, 8))[0]
    assert encoded >> 29 == 6
    method, _, inst = struct.unpack_from('<iii', probe.binary, probe.offset(registration[9]) + ((encoded & 0x1ffffffe) >> 1) * 12)
    assert probe.string(struct.unpack_from('<I', probe.metadata, probe.methods + method * 36)[0]) == 'ReadOnly'
    inst_ptr = struct.unpack_from('<Q', probe.binary, probe.offset(registration[3]) + inst * 8)[0]
    count, args = struct.unpack_from('<QQ', probe.binary, probe.offset(inst_ptr))
    assert count == 1
    type_ptr = struct.unpack_from('<Q', probe.binary, probe.offset(args))[0]
    typedef, bits = struct.unpack_from('<QI', probe.binary, probe.offset(type_ptr))
    assert (bits >> 16) & 255 == 17 and probe.full_name(typedef) == expected_type
rva, size = 0x7ee19d0, 1597
code = bytearray(code_at_rva(image, rva, size))
functions = {}
callbacks = []
trace = []
networked = False
component_set = []
has_static_index = False
mega_compatible = False
static_index = b'\0' * 4
static_payload = b'\0' * 24
type_names = {4: 'disabled', 5: 'child', 6: 'guaranteed_static', 7: 'static', 8: 'mega_static'}
def callback(target, result, args, body):
    fn = ctypes.CFUNCTYPE(result, *args)(body)
    callbacks.append(fn)
    functions[target] = len(code)
    code.extend(bytes.fromhex('48 b8') + struct.pack('<Q', ctypes.cast(fn, ctypes.c_void_p).value) + bytes.fromhex('ff e0'))
ptr, u64, integer = ctypes.c_void_p, ctypes.c_uint64, ctypes.c_int
def scalar(command, entity, value, method):
    trace.append({'op': 'clock', 'entity': entity, 'bytes': struct.pack('<Q', value).hex()})
def payload(command, entity, data, method):
    trace.append({'op': {1: 'child', 2: 'network', 3: 'active'}[method], 'entity': entity, 'bytes': ctypes.string_at(data, 12).hex()})
callback(0xbea30, None, [ptr, u64, u64, u64], scalar)
callback(0xbe2a0, None, [ptr, u64, ptr, u64], payload)
callback(0x8edbd90, None, [ptr, u64, ptr, ptr], lambda command, entity, types, method: trace.append({'op': 'add_set', 'entity': entity, 'types': component_set}))
callback(0xaa0e0, None, [ptr, u64, u64, ptr], lambda command, entity, chain, method: trace.append({'op': 'changed', 'entity': entity, 'bytes': struct.pack('<Q', chain).hex()}))
callback(0x8edb250, u64, [ptr, integer, ptr], lambda *unused: 0x10000007b)
callback(0xce6990, ctypes.c_uint8, [ptr, u64], lambda *unused: static_mode)
callback(0xce6d20, ctypes.c_uint8, [ptr, u64], lambda *unused: has_static_index)
callback(0xce6f40, ctypes.c_uint32, [ptr, u64, ptr], lambda *unused: struct.unpack('<I', static_index)[0])
callback(0xc06120, ctypes.c_uint8, [ptr, u64], lambda *unused: mega_compatible)
def read_static(output, lookup, entity, method):
    ctypes.memmove(output, static_payload, 24)
    return output
callback(0xce6bd0, ptr, [ptr, ptr, u64, ptr], read_static)
callback(0xbd8d0, None, [ptr, u64, ptr, ptr], lambda command, entity, data, method: trace.append({'op': 'static_compatible', 'entity': entity, 'bytes': ctypes.string_at(data, 24).hex()}))
callback(0xc2ed30, ctypes.c_uint8, [ptr, u64], lambda *unused: networked)
callback(0xf15f0, u64, [u64], lambda token: token)
def construct_set(output, first, second, unused):
    global component_set
    component_set = [type_names[first], type_names[second]]
    ctypes.memset(output, 0, 72)
callback(0x9122af0, None, [ptr, u64, u64, ptr], construct_set)
def construct_static_set(output, *args):
    global component_set
    component_set = [type_names[token] for token in args[:-1]]
    ctypes.memset(output, 0, 72)
callback(0x9122d30, None, [ptr, u64, u64, u64, u64, ptr], construct_static_set)
callback(0x9122ea0, None, [ptr, u64, u64, u64, u64, u64, ptr], construct_static_set)
callback(0x128dc10, u64, [], lambda: 6)
callback(0x10b8ca0, u64, [], lambda: 7)
trap = len(code)
code += b'\xcc\xc3'
ready = len(code)
code += b'\x01' + b'\0' * 7
metadata_slot = len(code)
code += b'\0' * 8
metadata = len(code)
code += b'\0' * 0xe8
code[metadata + 0xe0] = 1
slots = {}
for target, token in [(0xda4e668, 1), (0xdaaaa70, 2), (0xdad1760, 3), (0xda536a0, 4), (0xd9deed0, 5), (0xda95330, 8)]:
    slots[target] = len(code)
    code += struct.pack('<Q', token)
listing = subprocess.run(['C:/Program Files/LLVM/bin/llvm-objdump.exe', '-d', f'--start-address={0x180000000+rva:#x}', f'--stop-address={0x180000000+rva+size:#x}', str(root.parent / 'GameAssembly.dll')], capture_output=True, text=True, check=True).stdout
for line in listing.splitlines():
    match = re.match(r'\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(.*)', line)
    if not match:
        continue
    at = int(match[1], 16) - 0x180000000 - rva
    raw = bytes.fromhex(match[2])
    if '%rip' in match[3]:
        target = int(re.search(r'# (0x[0-9a-f]+)', match[3])[1], 16) - 0x180000000
        dest = ready if target in [0xdfc8e76, 0xdfd2c9b] else slots.get(target, metadata_slot)
        offset = 2 if raw[0] in [0x80, 0xc6, 0x83] else len(raw) - 4
        struct.pack_into('<i', code, at + offset, dest - at - len(raw))
    if raw[0] == 0xe8:
        target = rva + at + 5 + struct.unpack_from('<i', raw, 1)[0]
        struct.pack_into('<i', code, at + 1, functions.get(target, trap) - at - 5)
k = ctypes.WinDLL('kernel32', use_last_error=True)
k.VirtualAlloc.argtypes = [ptr, ctypes.c_size_t, ctypes.c_ulong, ctypes.c_ulong]
k.VirtualAlloc.restype = ptr
k.VirtualFree.argtypes = [ptr, ctypes.c_size_t, ctypes.c_ulong]
p = k.VirtualAlloc(None, len(code), 0x3000, 0x40)
if not p:
    raise ctypes.WinError(ctypes.get_last_error())
try:
    struct.pack_into('<Q', code, metadata_slot, p + metadata)
    ctypes.memmove(p, bytes(code), len(code))
    run = ctypes.CFUNCTYPE(None, u64, integer, u64, u64, ptr, ptr, ctypes.c_double, ptr)(p)
    cases = []
    rng = random.Random(99712)
    scenarios = []
    if static_mode:
        for enabled, networked, has_index, mega, variant in itertools.product([False, True], [False, True], [False, True], [False, True], range(32)):
            source = bytes([variant & 1]) + bytes(rng.randrange(256) for _ in range(23))
            scenarios.append({'auto': enabled, 'networked': networked, 'duration': rng.choice([0., 0.5, 100.]), 'now': rng.choice([0., 5., 1e6]), 'forwarded': rng.choice([-2., 0., 1., 200.]), 'index': rng.choice([-1, 0, 8]), 'has_index': has_index, 'mega': mega, 'source_static': source.hex(), 'static_index': bytes(rng.randrange(256) for _ in range(4)).hex()})
    else:
        for enabled, networked, duration, now, forwarded, index in itertools.product([False, True], [False, True], [0., 0.5, 100.], [0., 5., 1e6], [-2., 0., 1., 200.], [-1, 0, 8]):
            scenarios.append({'auto': enabled, 'networked': networked, 'duration': duration, 'now': now, 'forwarded': forwarded, 'index': index})
    for case in scenarios:
        networked = case['networked']
        has_static_index = case.get('has_index', False)
        mega_compatible = case.get('mega', False)
        static_payload = bytes.fromhex(case.get('source_static', '00' * 24))
        static_index = bytes.fromhex(case.get('static_index', '00' * 4))
        config = ctypes.create_string_buffer(48)
        config[0] = int(case['auto'])
        struct.pack_into('<f', config, 44, case['duration'])
        datas = ctypes.create_string_buffer(208)
        struct.pack_into('<d', datas, 0, case['now'])
        command = ctypes.create_string_buffer(32)
        trace = []
        run(0x200000007, case['index'], 0x300000058, 0x100000063, command, config, case['forwarded'], datas)
        cases.append(dict(case, trace=trace))
    name = 'chain_attachment_static.json' if static_mode else 'chain_attachment.json'
    scope = ('static/mega-static' if static_mode else 'non-static') + ' original attachment body; metadata ready; lookups, type-set construction and ECB command sinks supplied; no ECB playback or placement'
    (root / 'evidence/oracles' / name).write_text(json.dumps({'binary_sha256': expected, 'rva': hex(rva), 'scope': scope, 'cases': cases}, separators=(',', ':')) + '\n')
    print('captured', len(cases), 'attachment traces')
finally:
    k.VirtualFree(p, 0, 0x8000)
