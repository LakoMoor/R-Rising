#!/usr/bin/env python3
"""Replay original Unity.Entities pure leaves: TypeIndex flag getters, Hash128 compare/hash,
Entity.CompareTo, ChunkIndex.op_LessThan and Archetype getters (see oracle_replay.py)."""
from oracle_replay import ROOT, load_image, run, XORSHIFT

TYPE = [('cleanup', 0x8fcd9b0), ('cleanup_shared', 0x8fcd9c0), ('cleanup_buffer', 0x8fcd9d0),
        ('is_component', 0x8fcd9e0), ('shared', 0x8fcd9f0), ('iequatable', 0x8fcda00),
        ('managed_component', 0x8fcda10), ('managed_shared', 0x8fcda20), ('managed_type', 0x8fcda30),
        ('zero_sized', 0x8fcda40), ('chunk_component', 0x8fcda50), ('enableable', 0x8fcda60),
        ('refcounted', 0x8fcda70), ('temp_baking', 0x8fcdaa0), ('buffer', 0x5b6a0f0),
        ('native_container', 0x5b6f4b0), ('baking_only', 0x5b6f4d0), ('has_refs', 0x8fcda80),
        ('serializable', 0x8fcda90)]
TYPE_VALUE = [('type_flags', 0x8fcdab0), ('type_index', 0x25b2500)]
HASH = [('h_lt', 0x8fcb7f0), ('h_gt', 0x8fcb810), ('h_cmp', 0x8fcb830), ('h_hash', 0x8fcb880),
        ('h_ne', 0x86436f0)]
ARCH_BOOL = [('a_cleanup_complete', 0x911c330), ('a_cleanup_needed', 0x911c340), ('a_disabled', 0x911c350),
             ('a_prefab', 0x911c360), ('a_spawn_tag', 0x911c370), ('a_destroy_tag', 0x911c380),
             ('a_chunk_header', 0x911c390), ('a_blob', 0x911c3a0), ('a_managed_refs', 0x911c3b0),
             ('a_companion', 0x911c3c0), ('a_weak', 0x911c3d0), ('a_sysinst', 0x911c3e0),
             ('a_has_chunk_comp', 0x911c4e0)]
ARCH_INT = [('a_num_native', 0x911c3f0), ('a_num_buffer', 0x911c400), ('a_num_managed', 0x911c420),
            ('a_num_tag', 0x911c440), ('a_num_shared', 0x911c460), ('a_num_chunk', 0x911c480),
            ('a_nonzero', 0x911c490), ('a_native_end', 0x911c4a0), ('a_buffer_end', 0x911c4b0),
            ('a_tag_end', 0x911c4c0), ('a_shared_end', 0x911c4d0)]
MISC = [('entity_cmp', 0x8fc5a80), ('chunk_lt', 0x8fcdac0), ('chunk_gt', 0x8fcdad0), ('cta_ge', 0x8fcdae0)]

def decl():
    s = ''
    for n, _ in TYPE + ARCH_BOOL + [('h_lt', 0), ('h_gt', 0), ('h_ne', 0)]:
        s += f'extern uint8_t {n}(const void*,const void*) MS;\n'
    for n, _ in TYPE_VALUE + ARCH_INT + [('h_cmp', 0), ('h_hash', 0)]:
        s += f'extern int32_t {n}(const void*,const void*) MS;\n'
    s += 'extern int32_t entity_cmp(const void*,uint64_t) MS;\nextern uint8_t chunk_lt(int32_t,int32_t) MS;\n'
    s += 'extern uint8_t chunk_gt(int32_t,int32_t) MS;\nextern uint8_t cta_ge(int32_t,int32_t) MS;\n'
    return s

def drivers():
    t = XORSHIFT + '#define MS __attribute__((ms_abi))\n' + decl()
    tcalls = ''.join(f'printf("\\t%u",(unsigned){n}(&v,0));' for n, _ in TYPE) + \
        ''.join(f'printf("\\t%d",{n}(&v,0));' for n, _ in TYPE_VALUE)
    type_main = t + r'''int main(void){for(int i=0;i<2048;i++){uint32_t v; if(i<32)v=1u<<i; else if(i<64)v=~(1u<<(i-32)); else if(i==64)v=0; else if(i==65)v=0xffffffff; else {v=rnd(); if(i&1)v&=rnd();}
 printf("%08x",v);''' + tcalls + r'''printf("\n");}return 0;}'''
    hash_main = t + r'''int main(void){for(int i=0;i<2048;i++){uint32_t a[4],b[4];for(int k=0;k<4;k++){a[k]=rnd();b[k]=a[k];}
 if(i%4==1){int k=rnd()&3;b[k]=rnd();} else if(i%4==2){int k=rnd()&3;b[k]+=(i&8)?1:-1;} else if(i%4==3){b[3]=a[3];b[2]=a[2];b[1]=rnd();}
 if(i<16){for(int k=0;k<4;k++){a[k]=(i&1)?0:0xffffffff;b[k]=(i&2)?0:0xffffffff;}}
 printf("%08x%08x%08x%08x\t%08x%08x%08x%08x\t%u\t%u\t%d\t%d\t%d\t%u\n",a[0],a[1],a[2],a[3],b[0],b[1],b[2],b[3],h_lt(a,b),h_gt(a,b),h_cmp(a,b),h_hash(a,0),h_hash(b,0),h_ne(a,b));}return 0;}'''
    arch = ''.join(f'printf("\\t%u",(unsigned){n}(buf,0));' for n, _ in ARCH_BOOL) + \
        ''.join(f'printf("\\t%d",{n}(buf,0));' for n, _ in ARCH_INT)
    arch_main = t + r'''int main(void){for(int i=0;i<2048;i++){uint8_t buf[512];memset(buf,0xa5,sizeof buf);
 int16_t s[5];for(int k=0;k<5;k++){s[k]=(int16_t)rnd(); if(i&1)s[k]&=0xff; if(i<32)s[k]=(i&1)?-1:(i>>1);}
 uint16_t flags=(uint16_t)(i<16384?rnd():0); if(i<16)flags=1u<<i; if(i>=16&&i<32)flags=~(1u<<(i-16));
 int32_t tc=(int32_t)(rnd()&0xff); if(i%5==0)tc=s[4];
 memcpy(buf+280,&s[0],2);memcpy(buf+282,&s[1],2);memcpy(buf+284,&s[2],2);memcpy(buf+286,&s[3],2);memcpy(buf+288,&s[4],2);
 memcpy(buf+196,&tc,4);memcpy(buf+290,&flags,2);
 printf("%04x\t%d\t%d\t%d\t%d\t%d\t%d",flags,s[0],s[1],s[2],s[3],s[4],tc);''' + arch + r'''printf("\n");}return 0;}'''
    misc_main = t + r'''int main(void){for(int i=0;i<2048;i++){int32_t a=rnd(),b=rnd(); if(i%3==0)b=a; if(i%3==1)b=a+((i&4)?1:-1); if(i<8){int32_t s[]={0,1,-1,0x7fffffff,(int32_t)0x80000000};a=s[i%5];b=s[(i+2)%5];}
 uint32_t ver=rnd(); uint64_t e=((uint64_t)ver<<32)|(uint32_t)b; int32_t ea=a;
 printf("%08x\t%08x\t%d\t%u\t%u\t%u\n",(uint32_t)a,(uint32_t)b,entity_cmp(&ea,e),chunk_lt(a,b),chunk_gt(a,b),cta_ge(a,b));}return 0;}'''
    return type_main, hash_main, arch_main, misc_main

def main():
    image, digest = load_image()
    tm, hm, am, mm = drivers()
    outs = [
        ('typeindex_flags.tsv', tm, TYPE + TYPE_VALUE, 'raw_hex then ' + ' '.join(n for n, _ in TYPE + TYPE_VALUE)),
        ('hash128.tsv', hm, HASH, 'a_hex(v0..v3) b_hex lt gt compare hash(a) hash(b) inequality'),
        ('archetype_getters.tsv', am, ARCH_BOOL + ARCH_INT, 'flags_hex first_buffer first_managed first_tag first_shared first_chunk types_count then ' + ' '.join(n for n, _ in ARCH_BOOL + ARCH_INT)),
        ('entity_compare.tsv', mm, MISC, 'a b entity_compare(a,Entity(b,v)) chunk_lt chunk_gt component_type_in_archetype_ge'),
    ]
    total = 0
    for name, src, entries, cols in outs:
        rows = run(image, entries, src)
        if len(rows) != 2048:
            raise SystemExit(f'{name}: incomplete output')
        header = (f'# Original GameAssembly.dll sha256={digest}\n# x86_64/Rosetta replay\n# RVAs: '
                  + ','.join(f'{n}={r:x}' for n, r in entries) + f'\n# {cols}\n')
        (ROOT / 'evidence/oracles' / name).write_text(header + '\n'.join(rows) + '\n')
        total += len(rows)
        print(f'{name}: {len(rows)} cases, {len(entries)} functions')

if __name__ == '__main__':
    main()
