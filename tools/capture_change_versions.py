#!/usr/bin/env python3
"""Replay the pinned original's change-version comparison and increment leaves.

Writes only inputs/outputs; temporary original instruction copies are discarded
by oracle_replay.run. Includes wrap-around, the zero sentinel and half-range.
"""
from oracle_replay import ROOT, XORSHIFT, load_image, run

ENTRIES = [('did_change', 0x9072720), ('increment_version', 0x9072730)]
DRIVER = XORSHIFT + r'''
#define MS __attribute__((ms_abi))
extern uint8_t did_change(uint32_t,uint32_t) MS;
extern void increment_version(uint32_t*) MS;
int main(void) {
    const uint32_t edge[] = {0,1,2,0x7fffffff,0x80000000,0x80000001,0xfffffffe,0xffffffff};
    for (int i=0;i<2048;i++) {
        uint32_t chunk, last;
        if (i<64) { chunk=edge[i/8]; last=edge[i%8]; }
        else {
            last=rnd();
            switch(i%5) {
                case 0: chunk=last; break;
                case 1: chunk=last+1u; break;
                case 2: chunk=last-1u; break;
                case 3: chunk=last+0x80000000u; break;
                default: chunk=rnd(); break;
            }
        }
        uint32_t next=chunk;
        increment_version(&next);
        printf("%08x\t%08x\t%u\t%08x\n",chunk,last,(unsigned)did_change(chunk,last),next);
    }
    return 0;
}
'''


def main():
    image, digest = load_image()
    rows = run(image, ENTRIES, DRIVER)
    if len(rows) != 2048:
        raise SystemExit('incomplete original output')
    header = (f'# Original GameAssembly.dll sha256={digest}\n'
              '# x86_64/Rosetta replay, no helper stubs\n'
              '# RVAs: DidChange=9072720 IncrementGlobalSystemVersion=9072730\n'
              '# chunk_version_hex last_system_version_hex changed incremented_chunk_hex\n')
    (ROOT / 'evidence/oracles/change_versions.tsv').write_text(header + '\n'.join(rows) + '\n')
    print(f'change_versions.tsv: {len(rows)} cases per original function')


if __name__ == '__main__':
    main()
