#!/usr/bin/env python3
"""Count captured original-function evidence from golden headers, excluding duplicate RVAs.

This is evidence inventory, not behavioral coverage of the full application.
Only inputs/outputs are counted; the pinned original image is not needed in CI.
"""
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent


def inventory():
    reference = json.loads((ROOT / 'reference.lock.json').read_text())
    digest = next(f['sha256'] for f in reference['files'] if f['path'] == 'GameAssembly.dll')
    paths = sorted((ROOT / 'tests/golden').glob('*.tsv')) + sorted((ROOT / 'evidence/oracles').glob('*.tsv'))
    captures = []
    addresses = set()
    evaluations = 0
    for path in paths:
        lines = path.read_text().splitlines()
        if not lines or f'sha256={digest}' not in lines[0]:
            raise ValueError(f'{path}: missing or foreign binary identity')
        headers = [line for line in lines if line.startswith('#')]
        named = [line for line in headers if line.startswith('# RVAs:')]
        if named:
            rvas = re.findall(r'\b\w+=([0-9a-f]+)\b', named[0])
        elif path.name == 'type_flags.tsv':
            rvas = re.findall(r'0x([0-9a-f]+)\b', headers[1])
        elif path.name == 'material_mesh.tsv':
            rvas = headers[1].split('RVAs ', 1)[1].split(';', 1)[0].split(',')
        else:
            raise ValueError(f'{path}: unsupported original-function header')
        rvas = [int(rva, 16) for rva in rvas]
        count = sum(bool(line) and not line.startswith('#') for line in lines)
        if not rvas or count == 0 or len(rvas) != len(set(rvas)):
            raise ValueError(f'{path}: empty evidence or duplicate addresses in one capture')
        addresses.update(rvas)
        evaluations += len(rvas) * count
        captures.append({
            'golden': path.relative_to(ROOT).as_posix(),
            'rvas': [f'0x{rva:x}' for rva in rvas],
            'cases_per_function': count,
            'helper_stubs': ['IL2CPP sqrt wrapper replaced by sqrtss for quaternion normalize']
                            if path.name == 'math_matrix.tsv' else
                            ['five supplied-value ECS accessors; initialized metadata and float2.zero; sqrtss wrapper stub']
                            if path.name == 'movement_shared.tsv' else
                            ['IL2CPP sqrt wrapper replaced by sqrtss']
                            if path.name == 'aim_prediction.tsv' else
                            [] if path.name == 'small_helpers.tsv' else [],
        })
    return {
        'schema_version': 1,
        'metric': 'captured_original_function_addresses',
        'original_sha256': digest,
        'distinct_functions': len(addresses),
        'function_evaluations_including_overlapping_captures': evaluations,
        'behavioral_parity_percent': None,
        'warning': 'Function replays are not full-game scenarios or a game-completion percentage.',
        'captures': captures,
    }


if __name__ == '__main__':
    print(json.dumps(inventory(), indent=2))
