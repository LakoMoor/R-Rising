"""Shared helpers for original-function replay oracles.

Original x86-64 bytes are read from the pinned GameAssembly.dll, assembled into a
temporary Mach-O executable with a C driver and executed with the Windows x64
calling convention under Rosetta. Neither the bytes nor the executable is kept;
only input/output rows are written by the capture scripts.

By default only leaves or explicitly linked calls are accepted; no jumps out of
the declared extent. Original functions and helper substitutions are caller-selected.
The shared movement capture uses an explicit audited function extent and isolated
runtime globals. These substitutions must be reported alongside its outputs.
RIP-relative reads of constants are supported by copying the read-only bytes
next to the function and patching the displacement in the temporary copy.
"""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
from capture_type_flags import code_at_rva

ROOT = Path(__file__).resolve().parent.parent
LLVM_MC = '/opt/homebrew/opt/llvm/bin/llvm-mc'


def load_image():
    image = (ROOT.parent / 'GameAssembly.dll').read_bytes()
    digest = hashlib.sha256(image).hexdigest()
    reference = json.loads((ROOT / 'reference.lock.json').read_text())
    expected = next(f['sha256'] for f in reference['files'] if f['path'] == 'GameAssembly.dll')
    if digest != expected:
        raise SystemExit('pinned original binary mismatch')
    return image, digest


def _decode(code):
    text = ' '.join('0x%02x' % b for b in code)
    out = subprocess.run([LLVM_MC, '--disassemble', '-triple=x86_64', '-output-asm-variant=1', '--show-encoding'],
                         input=text, capture_output=True, text=True, check=True).stdout
    insns = []
    for line in out.splitlines():
        m = re.match(r'\s+(\S.*?)\s+# encoding: \[(.*)\]', line)
        if m:
            raw = bytes(int(x, 16) for x in m.group(2).split(','))
            insns.append((m.group(1), raw))
    return insns


def analyse(image, rva, allow_noreturn_call=False, known_calls=(), max_jump_target=0x200):
    """Return (code, relocations) for the function at rva; relocations are
    (disp_offset_in_code, insn_end_offset, target_rva)."""
    window = code_at_rva(image, rva, 3000)
    insns = _decode(window)
    offset = 0
    limit = 0
    relocs = []
    end = None
    for index, (text, raw) in enumerate(insns):
        if text.split()[0] == 'call' and window[offset] == 0xe8:
            raw = window[offset:offset + 5]
        if window[offset:offset + len(raw)] != raw:
            # llvm-mc drops redundant prefixes (0x66, empty REX); recover them from the window
            for k in (1, 2, 3):
                if window[offset + k:offset + k + len(raw)] == raw:
                    raw = window[offset:offset + k + len(raw)]
                    break
        if text.split()[0].startswith('j'):
            b0 = window[offset]
            size = 2 if (0x70 <= b0 <= 0x7f or b0 == 0xeb) else 5 if b0 == 0xe9 else 6 if b0 == 0x0f else 0
            if not size:
                raise SystemExit(f'unknown jump encoding in {rva:x}')
            raw = window[offset:offset + size]
        if window[offset:offset + len(raw)] != raw:
            raise SystemExit(f'decode mismatch at {rva:x}+{offset}')
        nxt = offset + len(raw)
        mnem = text.split()[0]
        if mnem.startswith('call'):
            following = insns[index + 1][0] if index + 1 < len(insns) else ''
            mc = re.match(r'call\s+(-?\d+)$', text)
            if mc and rva + nxt + int(mc.group(1)) in known_calls and raw[0] == 0xe8:
                relocs.append((offset + 1, nxt, rva + nxt + int(mc.group(1)), 'call'))
            elif not (allow_noreturn_call and following.startswith('int3')):
                raise SystemExit(f'call in {rva:x}: {text}')
        m = re.search(r'\[rip \+ (-?\d+)\]', text)
        if m:
            rel = int(m.group(1))
            pat = rel.to_bytes(4, 'little', signed=True)
            at = raw.rfind(pat)
            if at < 0:
                raise SystemExit(f'cannot find displacement in {text}')
            relocs.append((offset + at, nxt, rva + nxt + rel, 'rip'))
        if mnem.startswith('j'):
            m2 = re.match(r'j\w+\s+(-?\d+)$', text)
            if not m2:
                raise SystemExit(f'indirect jump in {rva:x}: {text}')
            target = nxt + int(m2.group(1))
            if target < 0 or target > max_jump_target:
                raise SystemExit(f'jump leaves function {rva:x}: {text}')
            limit = max(limit, target)
        if (mnem in ('ret', 'jmp') and nxt > limit) or (mnem == 'int3' and offset >= limit):
            end = nxt if mnem != 'int3' else offset
            break
        offset = nxt
    if end is None:
        raise SystemExit(f'no end found for {rva:x}')
    return window[:end], [r for r in relocs if r[1] <= end]


def assemble(image, entries, stubs=None, rip_overrides=None, extra_assembly=''):
    """entries: (symbol, rva[, allow_noreturn_call[, max_jump_target]]).

    rip_overrides maps original data RVAs to local bytes or tuples of assembly
    directives. extra_assembly defines the corresponding local context. Neither
    option changes the supplied original image on disk.
    """
    lines = ['.text']
    data = []
    stubs = stubs or {}
    rip_overrides = rip_overrides or {}
    known = {e[1]: e[0] for e in entries}
    known.update({rva: s[0] for rva, s in stubs.items()})
    for entry in entries:
        name, rva = entry[0], entry[1]
        code, relocs = analyse(image, rva, len(entry) > 2 and entry[2], known,
                               max_jump_target=entry[3] if len(entry) > 3 else 0x200)
        relocs = sorted(relocs)
        code = bytearray(code)
        for k, (disp_at, insn_end, target, kind) in enumerate(relocs):
            if kind == 'rip':
                const = rip_overrides[target] if target in rip_overrides else code_at_rva(image, target, 16)
                label = f'{name}_const{k}'
                data.append((label, const))
            # displacement is resolved by the assembler from the label
            code[disp_at:disp_at + 4] = b'\0\0\0\0'
        # emit with label-relative displacement fixups
        pieces = []
        cursor = 0
        for k, (disp_at, insn_end, target, kind) in enumerate(relocs):
            pieces.append('.byte ' + ','.join(hex(b) for b in code[cursor:disp_at]) if disp_at > cursor else '')
            sym = f'{name}_const{k}' if kind == 'rip' else f'_{known[target]}'
            pieces.append(f'.long {sym} - (_{name} + {insn_end})')
            cursor = disp_at + 4
        if cursor < len(code):
            pieces.append('.byte ' + ','.join(hex(b) for b in code[cursor:]))
        lines += ['.balign 16', f'.globl _{name}', f'_{name}:']
        lines += [p for p in pieces if p]
    for rva, (sname, body) in stubs.items():
        lines += ['.balign 16', f'.globl _{sname}', f'_{sname}:', body]
    if rip_overrides:
        # Isolated runtime globals can contain relocated pointers or mutable flags.
        # Mach-O forbids such absolute pointer relocations in the text section.
        lines.append('.data')
    lines.append('.balign 16')
    for label, const in data:
        lines += [f'{label}:']
        lines += list(const) if isinstance(const, tuple) else ['.byte ' + ','.join(hex(b) for b in const)]
    return '\n'.join(lines) + '\n' + extra_assembly


def run(image, entries, driver_source, timeout=60, stubs=None, rip_overrides=None, extra_assembly=''):
    with tempfile.TemporaryDirectory(prefix='rrising-oracle-') as scratch:
        scratch = Path(scratch)
        (scratch / 'original.S').write_text(assemble(image, entries, stubs, rip_overrides, extra_assembly))
        (scratch / 'driver.c').write_text(driver_source)
        subprocess.run(['/usr/bin/clang', '-arch', 'x86_64', '-O1', '-Wno-everything', str(scratch / 'driver.c'),
                        str(scratch / 'original.S'), '-o', str(scratch / 'oracle')], check=True)
        result = subprocess.run(['/usr/bin/arch', '-x86_64', str(scratch / 'oracle')], check=True, text=True,
                                capture_output=True, timeout=timeout)
    return result.stdout.splitlines()


XORSHIFT = r'''#include <stdint.h>
#include <stdio.h>
#include <string.h>
static uint32_t rng_state = 0x6f7261u;
static uint32_t rnd(void){rng_state^=rng_state<<13;rng_state^=rng_state>>17;rng_state^=rng_state<<5;return rng_state;}
'''
