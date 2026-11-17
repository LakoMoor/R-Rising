#!/usr/bin/env python3
"""Recover scene-matched type identities through constructed generic field signatures.

Read-only. No storage-layout reconstruction and no native pointer execution.
Uses the metadata/PE reader in probe_type_registry.py, but does not modify that
reader's accepted storage shapes. Generic structures are read from explicit
Il2CppGenericClass -> Il2CppGenericInst -> Il2CppType pointers after file mapping.
Behavior specification: shipped TypeHash.HashTypeName@0x8fccc20,
HashType@0x8fcc0a0, HashNamespace@0x8fcc9d0, and CalculateStableTypeHash@0x8fcce70.
The static constructor@0x8fcd380 excludes System.Guid's fields. Custom FieldOffset
attributes are taken from the image's own token range, never guessed from sizes.
"""
import argparse
import collections
import functools
import hashlib
import json
import struct
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from probe_type_registry import Probe, AttributeReader, PRIMITIVES, Unsupported, BASIS, combine, fnv, strhash, scene_hashes


class Identities:
    def __init__(self, probe):
        self.p = probe
        self.m = probe.metadata
        self.image_ranges = {}
        self.assembly_names = {}
        images, size = struct.unpack_from('<II', self.m, 168)
        assemblies, _ = struct.unpack_from('<II', self.m, 176)
        self.attr_data, _ = struct.unpack_from('<II', self.m, 200)
        self.attr_ranges, _ = struct.unpack_from('<II', self.m, 208)
        self.parameters, _ = struct.unpack_from('<II', self.m, 104)
        for image in range(size//40):
            at = images+image*40
            assembly_index = struct.unpack_from('<i',self.m,at+4)[0]
            assembly_name = probe.string(struct.unpack_from('<I',self.m,assemblies+assembly_index*64+16)[0])
            start,count = struct.unpack_from('<iI',self.m,at+8)
            range_start,range_count = struct.unpack_from('<iI',self.m,at+32)
            ranges = dict(struct.unpack_from('<II',self.m,self.attr_ranges+i*8) for i in range(range_start,range_start+range_count))
            for i in range(start,start+count):
                self.image_ranges[i]=ranges
                self.assembly_names[i]=assembly_name
        self.expressions = {}
        self.generic_observations = {}
        self.unsupported = collections.Counter()

    @functools.lru_cache(None)
    def expr(self, descriptor, substitutions=()):
        kind,data,_ = descriptor
        if kind in [0x13,0x1e]:
            if kind == 0x1e:
                raise Unsupported('method generic parameter in component signature')
            number = struct.unpack_from('<H',self.m,self.parameters+data*16+12)[0]
            if number >= len(substitutions):
                raise Unsupported('open generic type parameter')
            return substitutions[number]
        if kind == 0x15:
            definition_ptr = self.p.qword(data)
            defdesc = self.p.native_ptr(definition_ptr)
            definition = self.p.typedef_for(defdesc)
            inst = self.p.qword(data+8)
            method_inst = self.p.qword(data+16)
            if method_inst:
                raise Unsupported('generic class unexpectedly includes method instantiation')
            argc,argv = self.p.qword(inst), self.p.qword(inst+8)
            if not 1 <= argc <= 32:
                raise Unsupported('generic argument count outside supported bound')
            args = tuple(self.expr(self.p.native_ptr(self.p.qword(argv+j*8)),substitutions) for j in range(argc))
            self.generic_observations[data] = dict(generic_class=f'{data:#x}',definition_type_pointer=f'{definition_ptr:#x}',
                definition=self.p.full_name(definition),generic_inst=f'{inst:#x}',argc=argc,argv=f'{argv:#x}')
            return ('type',definition,args)
        if kind in [0x0f,0x1d]:
            return ('pointer' if kind==0x0f else 'array',self.expr(self.p.native_ptr(data),substitutions),1)
        if kind == 0x14:
            element = self.p.native_ptr(self.p.qword(data))
            rank = self.p.binary[self.p.offset(data+8)]
            return ('array',self.expr(element,substitutions),rank)
        i = self.p.typedef_for(descriptor)
        if self.p.type_rows[i]['generic'] >= 0 and not substitutions:
            raise Unsupported('unconstructed generic definition')
        return ('type',i,())

    def contains_entity(self, expression, seen=None):
        seen=set() if seen is None else seen
        if expression in seen:
            return False
        seen.add(expression)
        if expression[0]!='type':
            return False  # pointers/arrays are never patched as inline entities
        if self.p.full_name(expression[1])=='Unity.Entities.Entity':
            return True
        row=self.p.type_rows[expression[1]]
        if not row['bitfield']&1 or row['bitfield']&2 or row['namespace']=='System' and row['name'] in PRIMITIVES.values():
            return False
        return any(self.contains_entity(field,seen) for _,field,_ in self.fields(expression))

    SCALARS = {'System.Boolean':1,'System.Char':2,'System.SByte':1,'System.Byte':1,'System.Int16':2,'System.UInt16':2,
               'System.Int32':4,'System.UInt32':4,'System.Int64':8,'System.UInt64':8,'System.Single':4,'System.Double':8,
               'System.IntPtr':8,'System.UIntPtr':8}

    @functools.lru_cache(None)
    def sequential_layout(self, expression):
        """(size, alignment, entity_offsets) of a value type by natural sequential layout.

        Constructed generics carry no layout in metadata (IL2CPP computes them when the class is
        initialised), so this applies the C# sequential rule. It refuses explicit layouts, and
        results are only trusted after scene validation (offsets, buffer headers, patches).
        Non-generic types are cross-checked against IL2CPP's own baked size by the caller.
        """
        if expression[0]!='type':
            return (8,8,())                       # pointers and managed arrays
        i,args=expression[1:]
        row=self.p.type_rows[i]
        name=self.p.full_name(i)
        if name in self.SCALARS:
            return (self.SCALARS[name],self.SCALARS[name],())
        if name=='Unity.Entities.Entity':
            return (8,4,(0,))
        if not row['bitfield']&1:
            return (8,8,())                       # managed reference
        explicit=row['flags']&0x18==0x10
        offset,align,entities,extent=0,1,[],0
        for _,field,declared in self.fields(expression):
            size,field_align,field_entities=self.sequential_layout(field)
            if explicit:
                if declared is None:
                    raise Unsupported('explicit layout field without a recovered offset')
                offset=declared
            else:
                offset=(offset+field_align-1)//field_align*field_align
            entities.extend(offset+e for e in field_entities)
            offset+=size
            extent=max(extent,offset)
            align=max(align,field_align)
        extent=max(extent,1)
        return ((extent+align-1)//align*align,align,tuple(entities))

    def short_name(self, expression):
        if expression[0]=='type':
            return self.p.type_rows[expression[1]]['name']
        suffix = '*' if expression[0]=='pointer' else '['+','*(expression[2]-1)+']'
        return self.short_name(expression[1])+suffix

    def namespace(self, expression):
        if expression[0]=='type':
            return self.p.type_rows[expression[1]]['namespace']
        return self.namespace(expression[1])

    @functools.lru_cache(None)
    def namespace_hash(self, expression):
        if expression[0]!='type':
            namespace = self.namespace(expression)
            return combine(BASIS,strhash(namespace)) if namespace else BASIS
        row = self.p.type_rows[expression[1]]
        if row['declaring'] >= 0:
            parent = self.p.typedef_for(self.p.native(row['declaring']))
            # A nested type's namespace walk uses the declaring name, not its
            # constructed argument list (the latter belongs to HashTypeName).
            parent_expr=('type',parent,())
            return combine(combine(BASIS,self.namespace_hash(parent_expr)),strhash(self.p.type_rows[parent]['name']))
        return combine(BASIS,strhash(row['namespace'])) if row['namespace'] else BASIS

    @functools.lru_cache(None)
    def name_hash(self, expression):
        value = combine(self.namespace_hash(expression),strhash(self.short_name(expression)))
        if expression[0]=='type':
            for arg in expression[2]:
                value=combine(value,self.name_hash(arg))
        return value

    def full_name(self, expression):
        if expression[0]!='type':
            suffix = '*' if expression[0]=='pointer' else '['+','*(expression[2]-1)+']'
            return self.full_name(expression[1])+suffix
        name=self.p.full_name(expression[1])
        return name+'<'+','.join(self.full_name(arg) for arg in expression[2])+'>' if expression[2] else name

    @functools.lru_cache(None)
    def fields(self, expression, inherited=False):
        if expression[0]!='type':
            return ()
        i,args=expression[1:]
        row=self.p.type_rows[i]
        result=[]
        for j in range(row['field_count']):
            at=self.p.fields+(row['field_start']+j)*12
            name,native,token=struct.unpack_from('<IiI',self.m,at)
            descriptor=self.p.native(native)
            if descriptor[2]&16 or (inherited and descriptor[2]&7 in (0,1)):
                continue
            offset=self.field_offset_attribute(i,token)
            if offset is None and row['flags']&0x18==0x10 and not row['generic']>=0:
                # IL2CPP bakes FieldOffsetAttribute into the field layout table.
                offset=self.p.layout(i)['fields'][j]['value_offset']
            result.append((self.p.string(name),self.expr(descriptor,args),offset))
        return tuple(result)

    def base_expression(self, expression):
        row=self.p.type_rows[expression[1]]
        parent=row['parent']
        if row['bitfield']&1 or parent<0:
            return None
        desc=self.p.native(parent)
        try:
            base=self.expr(desc,expression[2])
        except Unsupported:
            raise Unsupported('generic class base fields not recovered')
        if base[0]!='type' or self.p.full_name(base[1]) in ('System.Object','System.ValueType'):
            return None
        return base

    def hashed_fields(self, expression):
        # Reflection GetFields(Instance|Public|NonPublic) on the derived type:
        # declared fields first, then non-private fields of each base.
        result=list(self.fields(expression))
        base=self.base_expression(expression)
        while base is not None:
            result.extend(f for f in self.fields(base,True))
            base=self.base_expression(base)
        return tuple(result)

    def field_offset_attribute(self, declaring,token):
        ranges=self.image_ranges[declaring]
        if token not in ranges:
            return None
        reader=AttributeReader(self.m,self.attr_data+ranges[token],self.p)
        count=reader.uint()
        methods=[reader.fixed('<i') for _ in range(count)]
        names=[]
        for method in methods:
            typedef=struct.unpack_from('<i',self.m,self.p.methods+method*36+4)[0]
            names.append(self.p.full_name(typedef))
        offset=None
        for name in names:
            argc,fieldc,propc=reader.uint(),reader.uint(),reader.uint()
            args=[reader.value() for _ in range(argc)]
            for _ in range(fieldc+propc):
                reader.value()
                if reader.sint()<0:
                    reader.uint()
            if name=='System.Runtime.InteropServices.FieldOffsetAttribute':
                if len(args)!=1 or not isinstance(args[0],int):
                    raise Unsupported('invalid FieldOffsetAttribute payload')
                offset=args[0]
        return offset

    @functools.lru_cache(None)
    def engine_object(self, i):
        name=self.p.full_name(i)
        if name=='UnityEngine.Object':
            return True
        parent=self.p.type_rows[i]['parent']
        if parent<0:
            return False
        try:
            return self.engine_object(self.p.typedef_for(self.p.native(parent)))
        except Unsupported:
            return False

    def type_hash(self, expression,cache):
        if expression in cache:
            return cache[expression]
        value=self.name_hash(expression)
        cache[expression]=value
        if expression[0]!='type':
            return value
        row=self.p.type_rows[expression[1]]
        name=self.p.full_name(expression[1])
        if self.engine_object(expression[1]):
            value=combine(strhash(self.assembly_names[expression[1]]),value)
            cache[expression]=value
            return value
        # HashType's one-field-ignore set is System.Guid, resolved directly
        # from TypeHash..cctor's native metadata usage at VA 0x18da9c328.
        if name=='System.Guid' or row['bitfield']&2 or name in {
            'System.Boolean','System.Char','System.SByte','System.Byte','System.Int16','System.UInt16',
            'System.Int32','System.UInt32','System.Int64','System.UInt64','System.Single','System.Double',
            'System.IntPtr','System.UIntPtr','System.Void'}:
            return value
        for _,field,offset in self.hashed_fields(expression):
            field_hash=self.type_hash(field,cache)
            if offset is not None:
                value=combine(value,offset & ((1<<64)-1))
            value=combine(value,field_hash)
        cache[expression]=value
        return value

    def stable_hash(self,expression):
        row=self.p.type_rows[expression[1]] if expression[0]=='type' else None
        version=0
        if row:
            found=self.p.attribute_integer(row['index'],'Unity.Entities.TypeManager+TypeVersionAttribute')
            if found is not None:
                version=found
        return combine(self.type_hash(expression,{}),fnv(struct.pack('<i',version)))


    def match_scene(self, observed, baseline=frozenset()):
        probe=self.p
        identities=self
        candidates=set()
        for row in probe.type_rows:
            try:candidates.add(identities.expr(probe.native(row['byval'])))
            except (Unsupported,RecursionError):pass
        for native in range(probe.native_type_count):
            desc=probe.native(native)
            if desc[0]!=21:continue
            try:candidates.add(identities.expr(desc))
            except (Unsupported,RecursionError):pass
        matches={}
        unsupported=collections.Counter()
        for expression in sorted(candidates,key=repr):
            try:
                stable=identities.stable_hash(expression)
                if stable not in observed:
                    continue
                name=identities.full_name(expression)
                if stable in matches and matches[stable]['name']!=name:
                    raise ValueError(f'ambiguous hash {stable:016x}: {matches[stable]["name"]} / {name}')
                direct=[]
                if expression[0]=='type':
                    direct=[{'name':name,'type':identities.full_name(field),'field_offset_attribute':offset}
                            for name,field,offset in identities.fields(expression)]
                self.expressions[stable]=expression
                matches[stable]=dict(name=name,stable_hash=f'{stable:016x}',already_in_baseline=stable in baseline,
                    generic_arguments=[identities.full_name(arg) for arg in expression[2]] if expression[0]=='type' else [],
                    direct_fields=direct)
            except (Unsupported,RecursionError) as error:
                unsupported[str(error)]+=1
        return matches,unsupported


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--install',type=Path,default=Path('..'))
    parser.add_argument('--baseline',type=Path,help='layout report whose matched identities are already known')
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    binary=args.install/'GameAssembly.dll'
    metadata=args.install/'VRising_Data/il2cpp_data/Metadata/global-metadata.dat'
    probe=Probe(binary,metadata)
    identities=Identities(probe)
    observed,scene_count=scene_hashes(args.install/'VRising_Data/StreamingAssets/EntityScenes')
    baseline=set()
    if args.baseline:
        baseline={int(row['stable_hash'],16) for row in json.loads(args.baseline.read_text())['matches']}
    matches,unsupported=identities.match_scene(observed,baseline)
    result=dict(schema_version=1,evidence_kind='offline-derived-identity-and-scene-hash-matched',
        binary_sha256=hashlib.sha256(probe.binary).hexdigest(),metadata_sha256=hashlib.sha256(probe.metadata).hexdigest(),
        scenes=scene_count,scene_hashes=len(observed),candidate_expressions=len(candidates),
        matched_identities=len(matches),new_identities=len(set(matches)-baseline),
        unresolved_scene_hashes=[f'{n:016x}' for n in sorted(observed-set(matches)-baseline)],
        generic_class_observations=list(identities.generic_observations.values()),
        unsupported=dict(unsupported),
        limitations=['Identity only; no generic layout, runtime TypeInfo, or behavioral parity claim',
            'Inherited non-UnityEngine.Object class fields are refused',
            'Open generic parameters are refused; substitutions must come from native constructed signatures'],
        matches=sorted(matches.values(),key=lambda row:row['name']))
    if args.output:args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ['matches','generic_class_observations','unresolved_scene_hashes']},indent=2))
    for row in result['matches']:
        if not row['already_in_baseline'] or 'RenderMesh' in row['name']:
            print(row['stable_hash'],row['name'])


if __name__=='__main__':main()
