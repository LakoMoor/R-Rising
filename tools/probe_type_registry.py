#!/usr/bin/env python3
"""Offline component identity/layout probe for the pinned V Rising IL2CPP build.

This does not execute TypeManager and does not claim TypeInfo parity. It reads
metadata v31 and the native metadata-registration tables, reproduces the inspected
TypeHash algorithm for supported (nongeneric) types with checked TypeVersion(0) attributes, and reports only candidate hashes actually
present in the supplied entity scenes.

Format reference (not copied implementation):
https://github.com/Perfare/Il2CppDumper/blob/master/Il2CppDumper/Il2Cpp/MetadataClass.cs
https://github.com/Perfare/Il2CppDumper/blob/master/Il2CppDumper/Il2Cpp/Il2CppClass.cs
Behavior: shipped TypeHash RVAs 0x8e4aec0, 0x8fcc080, 0x8fcc0a0,
0x8fcc9d0, 0x8fccc20, 0x8fcce70. These are static observations, not runtime oracles.
"""
import argparse
import functools
import hashlib
import json
import struct
from pathlib import Path

EXPECTED_MATCHES = 1772  # 1,708 value-type shapes + 61 managed IComponentData classes + 3 constructed generics
BASIS = 0xCBF29CE484222325
PRIME = 0x100000001B3
MASK = (1 << 64) - 1
PRIMITIVES = {1: 'Void', 2: 'Boolean', 3: 'Char', 4: 'SByte', 5: 'Byte',
              6: 'Int16', 7: 'UInt16', 8: 'Int32', 9: 'UInt32', 10: 'Int64',
              11: 'UInt64', 12: 'Single', 13: 'Double', 14: 'String',
              22: 'TypedReference', 24: 'IntPtr', 25: 'UIntPtr', 28: 'Object'}


def fnv(data):
    h = BASIS
    for byte in data:
        h = ((h ^ byte) * PRIME) & MASK
    return h


def combine(a, b):
    return ((a ^ b) * PRIME) & MASK


def strhash(s):
    return fnv(s.encode('utf-16le'))


class Unsupported(ValueError):
    pass


class AttributeReader:
    def __init__(self, data, at, probe):
        self.data, self.at, self.probe = data, at, probe

    def byte(self):
        value = self.data[self.at]
        self.at += 1
        return value

    def uint(self):
        first = self.byte()
        if first < 0x80:
            return first
        if first < 0xc0:
            return ((first & 0x3f) << 8) | self.byte()
        if first < 0xe0:
            return ((first & 0x1f) << 24) | (self.byte() << 16) | (self.byte() << 8) | self.byte()
        if first == 0xf0:
            return self.fixed('<I')
        if first == 0xff:
            return 0xffffffff
        if first == 0xfe:
            return 0xfffffffe
        raise Unsupported(f'attribute compressed integer prefix {first:#x}')

    def sint(self):
        value = self.uint()
        if value == 0xffffffff:
            return -2147483648
        return -(value >> 1)-1 if value & 1 else value >> 1

    def fixed(self, fmt):
        value = struct.unpack_from(fmt, self.data, self.at)[0]
        self.at += struct.calcsize(fmt)
        return value

    def kind(self):
        kind = self.byte()
        if kind == 0x55:
            descriptor = self.probe.native(self.sint())
            index = self.probe.typedef_for(descriptor)
            kind = self.probe.native(self.probe.type_rows[index]['element'])[0]
        return kind

    def value(self, kind=None):
        if kind is None:
            kind = self.kind()
        fixed = {2:'<?', 3:'<H', 4:'<b', 5:'<B', 6:'<h', 7:'<H', 10:'<q', 11:'<Q', 12:'<f', 13:'<d'}
        if kind in fixed:
            return self.fixed(fixed[kind])
        if kind == 8:
            return self.sint()
        if kind == 9:
            return self.uint()
        if kind in [14, 0x1d]:
            count = self.sint()
            if count == -1:
                return None
            if not 0 <= count <= len(self.data)-self.at:
                raise Unsupported('attribute length out of bounds')
            if kind == 14:
                start = self.at
                self.at += count
                return self.data[start:self.at].decode('utf8')
            element_kind = self.kind()
            mixed = self.byte()
            return [self.value(None if mixed == 1 else element_kind) for _ in range(count)]
        if kind == 0xff:
            return {'native_type_index': self.sint()}
        raise Unsupported(f'attribute value kind {kind:#x}')


class Probe:
    def __init__(self, binary, metadata):
        self.binary = binary.read_bytes()
        self.metadata = metadata.read_bytes()
        m = self.metadata
        if struct.unpack_from('<II', m) != (0xFAB11BAF, 31):
            raise ValueError('this probe requires metadata v31')
        self.strings, self.string_bytes = struct.unpack_from('<II', m, 24)
        self.methods, _ = struct.unpack_from('<II', m, 48)
        self.interfaces, _ = struct.unpack_from('<II', m, 136)
        self.fields, field_bytes = struct.unpack_from('<II', m, 96)
        self.typedefs, typedef_bytes = struct.unpack_from('<II', m, 160)
        if typedef_bytes % 88 or field_bytes % 12:
            raise ValueError('metadata table size mismatch')
        self.type_count = typedef_bytes // 88
        e = struct.unpack_from('<I', self.binary, 0x3c)[0]
        if self.binary[e:e+4] != b'PE\0\0':
            raise ValueError('PE header required')
        self.image_base = struct.unpack_from('<Q', self.binary, e + 48)[0]
        self.sections = []
        start = e + 24 + struct.unpack_from('<H', self.binary, e+20)[0]
        for i in range(struct.unpack_from('<H', self.binary, e+6)[0]):
            at = start + 40*i
            _, va, raw_size, raw = struct.unpack_from('<IIII', self.binary, at+8)
            self.sections.append((self.image_base+va, raw_size, raw))
        # Find a metadata-registration candidate with adjacent matching
        # fieldOffsetsCount and typeDefinitionsSizesCount. Validate pointer ranges
        # and each TypeDefinition's byval descriptor before accepting it.
        hits = []
        at = 0
        needle = struct.pack('<Q', self.type_count)
        while True:
            at = self.binary.find(needle, at)
            if at < 0:
                break
            if at >= 80 and at+32 <= len(self.binary):
                values = struct.unpack_from('<16Q', self.binary, at-80)
                if values[12] == self.type_count:
                    try:
                        for i in [7, 11, 13]:
                            self.offset(values[i])
                        if values[6] < self.type_count:
                            raise ValueError('native type count too small')
                        hits.append((at-80, values))
                    except ValueError:
                        pass
            at += 1
        if len(hits) != 1:
            raise ValueError(f'expected one metadata registration, got {len(hits)}')
        self.registration_offset, values = hits[0]
        self.native_type_count, self.native_types = values[6:8]
        self.field_offsets, self.type_sizes = values[11], values[13]
        self.type_rows = []
        for i in range(self.type_count):
            at = self.typedefs + i*88
            ni, ns, byval, declaring, parent, element, generic, flags, fields = struct.unpack_from('<IIiiiiiii', m, at)
            counts = struct.unpack_from('<8H', m, at+64)
            bitfield, token = struct.unpack_from('<II', m, at+80)
            row = dict(index=i, name=self.string(ni), namespace=self.string(ns),
                       byval=byval, declaring=declaring, parent=parent,
                       generic=generic, flags=flags, element=element, field_start=fields,
                       interface_start=struct.unpack_from('<i',m,at+52)[0], interface_count=counts[6],
                       field_count=counts[2], bitfield=bitfield, token=token)
            desc = self.native(byval)
            if desc[0] not in PRIMITIVES and desc[0] not in [17, 18]:
                raise ValueError(f'byval descriptor kind mismatch for type {i}: {desc}')
            if desc[0] in [17, 18] and desc[1] != i:
                raise ValueError(f'byval descriptor index mismatch for type {i}')
            self.type_rows.append(row)
        self.primitive_rows = {row['name']: row['index'] for row in self.type_rows
                               if row['namespace'] == 'System' and row['name'] in PRIMITIVES.values()}

        self.attributes = {}
        image_base, image_bytes = struct.unpack_from('<II', m, 168)
        attr_base, _ = struct.unpack_from('<II', m, 200)
        range_base, _ = struct.unpack_from('<II', m, 208)
        for image in range(image_bytes // 40):
            at = image_base + image*40
            type_start, type_count = struct.unpack_from('<iI',m,at+8)
            range_start, range_count = struct.unpack_from('<iI',m,at+32)
            ranges = dict(struct.unpack_from('<II',m,range_base+i*8) for i in range(range_start,range_start+range_count))
            for i in range(type_start,type_start+type_count):
                token = self.type_rows[i]['token']
                names, args, unsupported = [], [], None
                if token in ranges:
                    reader = AttributeReader(m,attr_base+ranges[token],self)
                    count = reader.uint()
                    constructors = [reader.fixed('<i') for _ in range(count)]
                    for ctor in constructors:
                        declaring = struct.unpack_from('<i',m,self.methods+ctor*36+4)[0]
                        names.append(self.full_name(declaring))
                    try:
                        for _ in constructors:
                            arg_count, field_count, property_count = reader.uint(), reader.uint(), reader.uint()
                            values = [reader.value() for _ in range(arg_count)]
                            for _ in range(field_count+property_count):
                                reader.value()
                                if reader.sint() < 0:
                                    reader.uint()
                            args.append(values)
                    except (Unsupported, ValueError, IndexError, struct.error) as error:
                        unsupported = str(error)
                self.attributes[i] = dict(names=names, arguments=args, unsupported=unsupported)

    def attribute_integer(self, index, name):
        data = self.attributes[index]
        if name not in data['names']:
            return None
        at = data['names'].index(name)
        if at >= len(data['arguments']) or len(data['arguments'][at]) != 1 or not isinstance(data['arguments'][at][0],int):
            raise Unsupported(f'{name}: attribute constructor payload not decoded')
        return data['arguments'][at][0]

    def interface_names(self, i, visited=None):
        visited = set() if visited is None else visited
        if i in visited:
            return set()
        visited.add(i)
        row = self.type_rows[i]
        result = set()
        for j in range(row['interface_count']):
            native_index = struct.unpack_from('<i',self.metadata,self.interfaces+(row['interface_start']+j)*4)[0]
            try:
                typedef = self.typedef_for(self.native(native_index))
                result.add(self.full_name(typedef))
                result.update(self.interface_names(typedef,visited))
            except Unsupported:
                # Generic interfaces cannot hide an inherited non-generic marker in
                # admitted component types; keep their presence explicit.
                result.add(f'unsupported-native-interface:{native_index}')
        return result

    def has_generic_interface(self, i, definition_name, visited=None):
        """True if any (inherited) interface is a constructed instance of `definition_name`.

        Mirrors `GetInterfaces().Any(...)` in BuildComponentType (lambda at RVA 0x9065a80,
        result becomes TypeIndex bit 23, `IsIEquatable`); only the generic definition is compared.
        """
        visited = set() if visited is None else visited
        if i in visited:
            return False
        visited.add(i)
        row = self.type_rows[i]
        for j in range(row['interface_count']):
            native_index = struct.unpack_from('<i',self.metadata,self.interfaces+(row['interface_start']+j)*4)[0]
            kind, data, _ = self.native(native_index)
            if kind == 0x15:
                if self.full_name(self.typedef_for(self.native_ptr(self.qword(data)))) == definition_name:
                    return True
                continue
            try:
                typedef = self.typedef_for(self.native(native_index))
            except Unsupported:
                continue
            if self.has_generic_interface(typedef, definition_name, visited):
                return True
        return False

    @functools.lru_cache(None)
    def is_zero_size(self, i):
        row = self.type_rows[i]
        if not row['bitfield'] & 1 or row['namespace']=='System' and row['name'] in PRIMITIVES.values():
            return False
        return all(f['typedef_index'] is not None and self.is_zero_size(f['typedef_index']) for f in self.layout(i)['fields'] if not f['static'])

    def entity_offsets(self, i, base=0):
        if self.full_name(i) == 'Unity.Entities.Entity':
            return [base]
        row = self.type_rows[i]
        if not row['bitfield'] & 1 or row['namespace']=='System' and row['name'] in PRIMITIVES.values():
            return []
        offsets=[]
        for field in self.layout(i)['fields']:
            if not field['static'] and field['typedef_index'] is not None:
                offsets.extend(self.entity_offsets(field['typedef_index'],base+field['value_offset']))
            elif not field['static'] and self.native(field['native_type_index'])[0] == 21:
                # Constructed generic field: its storage is covered by the owner's size; only
                # refuse if the instantiation could hide an Entity needing patching.
                expression = self.identities.expr(self.native(field['native_type_index']))
                if self.identities.contains_entity(expression):
                    raise Unsupported('generic field contains Entity offsets that are not recovered')
        return offsets

    def ecs_layout(self, i, stable):
        row = self.type_rows[i]
        name = self.full_name(i)
        interfaces = self.interface_names(i)
        def has(suffix):
            return 'Unity.Entities.'+suffix in interfaces
        attrs = self.attributes[i]['names']
        forced = self.attribute_integer(i,'Unity.Entities.TypeManager+ForcedMemoryOrderingAttribute')
        version = self.attribute_integer(i,'Unity.Entities.TypeManager+TypeVersionAttribute')
        if version not in [None,0]:
            raise Unsupported('nonzero type version needs re-hashing')
        memory = 0 if name == 'Unity.Entities.Entity' else forced if forced is not None else stable
        category = 'entity' if name == 'Unity.Entities.Entity' else 'buffer' if has('IBufferElementData') else 'shared' if has('ISharedComponentData') else 'component' if has('IComponentData') else None
        size = self.layout(i)['managed_value_size']
        zero = self.is_zero_size(i)
        capacity = None
        max_capacity = self.attribute_integer(i,'Unity.Entities.MaximumChunkCapacityAttribute')
        maximum_chunk_capacity = 128 if max_capacity is None else min(128,max_capacity)
        size_in_chunk = None
        if category in ['component','entity']:
            size_in_chunk = 0 if zero else size
        if category == 'shared':
            size_in_chunk = 0
        if category == 'buffer':
            capacity = self.attribute_integer(i,'Unity.Entities.InternalBufferCapacityAttribute')
            if capacity is None:
                if not size:
                    raise Unsupported('buffer has zero element size')
                capacity = 128//size
            size_in_chunk = 16+size*capacity
        alignment = size if 0 < size < 16 and size & (size-1) == 0 else 16
        if name == 'Unity.Entities.Entity':
            alignment = 8
        return dict(memory_ordering=f'{memory:016x}', memory_ordering_source='Entity special registration' if name == 'Unity.Entities.Entity' else 'ForcedMemoryOrderingAttribute' if forced is not None else 'CalculateMemoryOrdering fallback to stable hash; no override attribute present',
                    type_version=0 if version is None else version,
                    interfaces=sorted(interfaces), attribute_types=attrs,
                    attribute_payload_issue=self.attributes[i]['unsupported'],
                    category=category, managed=any(not f['static'] and self.native(f['native_type_index'])[0] in [14,18,28,29] for f in self.layout(i)['fields']), zero_sized=size_in_chunk == 0, zero_size_struct=zero,
                    enableable=has('IEnableableComponent'),
                    # Entity is registered through the special path (memory ordering 0, always slot 0), not BuildComponentType.
                    iequatable=name != 'Unity.Entities.Entity' and self.has_generic_interface(i,'System.IEquatable`1'),
                    cleanup=has('ICleanupComponentData') or has('ICleanupBufferElementData') or has('ICleanupSharedComponentData'),
                    size_in_chunk=size_in_chunk, element_size=size if category == 'buffer' else size_in_chunk,
                    buffer_capacity=capacity, alignment_in_bytes=0 if category == 'shared' else alignment,
                    maximum_chunk_capacity=maximum_chunk_capacity,
                    entity_offsets=self.entity_offsets(i))

    def ecs_layout_generic(self, expression, stable):
        """Storage shape of a constructed generic value-type component/buffer element.

        Size, alignment and Entity offsets come from `Identities.sequential_layout` (the C#
        sequential/explicit rule over the constructed field types); metadata bakes no layout for
        constructed generics. Anything not cross-checkable by the scene acceptance is not admitted.
        """
        i = expression[1]
        name = self.identities.full_name(expression)
        interfaces = self.interface_names(i)
        attrs = self.attributes[i]['names']
        forced = self.attribute_integer(i,'Unity.Entities.TypeManager+ForcedMemoryOrderingAttribute')
        version = self.attribute_integer(i,'Unity.Entities.TypeManager+TypeVersionAttribute')
        max_capacity = self.attribute_integer(i,'Unity.Entities.MaximumChunkCapacityAttribute')
        if version not in [None,0] or forced is not None or self.attributes[i]['unsupported']:
            raise Unsupported('generic type carries an ordering/version override')
        category = 'buffer' if 'Unity.Entities.IBufferElementData' in interfaces else 'component' if 'Unity.Entities.IComponentData' in interfaces else None
        if category is None or not self.type_rows[i]['bitfield'] & 1:
            raise Unsupported('generic type is not an unmanaged component or buffer element')
        size, alignment_of, entities = self.identities.sequential_layout(expression)
        entities = list(entities)
        if size <= 0:
            raise Unsupported('empty generic component')
        capacity = None
        if category == 'buffer':
            capacity = self.attribute_integer(i,'Unity.Entities.InternalBufferCapacityAttribute')
            if capacity is None:
                capacity = 128//size
            size_in_chunk = 16+size*capacity
        else:
            size_in_chunk = size
        alignment = size if 0 < size < 16 and size & (size-1) == 0 else 16
        return dict(memory_ordering=f'{stable:016x}', memory_ordering_source='CalculateMemoryOrdering fallback to stable hash; no override attribute present',
                    type_version=0, interfaces=sorted(interfaces), attribute_types=attrs, attribute_payload_issue=self.attributes[i]['unsupported'],
                    category=category, managed=False, zero_sized=False, zero_size_struct=False,
                    enableable='Unity.Entities.IEnableableComponent' in interfaces, iequatable=self.has_generic_interface(i,'System.IEquatable`1'),
                    cleanup=any(x in interfaces for x in ['Unity.Entities.ICleanupComponentData','Unity.Entities.ICleanupBufferElementData','Unity.Entities.ICleanupSharedComponentData']),
                    size_in_chunk=size_in_chunk, element_size=size if category == 'buffer' else size_in_chunk,
                    buffer_capacity=capacity, alignment_in_bytes=alignment,
                    maximum_chunk_capacity=128 if max_capacity is None else min(128,max_capacity),
                    entity_offsets=entities), name, size

    def ecs_layout_managed_class(self, i, stable):
        """Storage shape of a managed `IComponentData` class (BuildComponentType managed branch).

        RVA 0x905afa7..0x905b0ff: sizeInChunk = 4 (a managed-store index; the object itself
        lives outside the chunk), elementSize = max(4, 0) = 4, bufferCapacity = -1, alignment 0.
        The zero-size flag (bit 30) is only set when sizeInChunk == 0, so it is never set here.
        Entity references inside the object are found by managed reflection at runtime and are
        not recoverable offline, so `entity_offsets` is empty and never patched.
        """
        attrs = self.attributes[i]['names']
        forced = self.attribute_integer(i,'Unity.Entities.TypeManager+ForcedMemoryOrderingAttribute')
        version = self.attribute_integer(i,'Unity.Entities.TypeManager+TypeVersionAttribute')
        max_capacity = self.attribute_integer(i,'Unity.Entities.MaximumChunkCapacityAttribute')
        if version not in [None,0] or forced is not None or max_capacity is not None or self.attributes[i]['unsupported']:
            raise Unsupported('managed class carries an ordering/capacity override')
        interfaces = self.interface_names(i)
        return dict(memory_ordering=f'{stable:016x}', memory_ordering_source='CalculateMemoryOrdering fallback to stable hash; no override attribute present',
                    type_version=0, interfaces=sorted(interfaces), attribute_types=attrs, attribute_payload_issue=self.attributes[i]['unsupported'],
                    category='component', managed=True, zero_sized=False, zero_size_struct=False,
                    enableable='Unity.Entities.IEnableableComponent' in interfaces, iequatable=self.has_generic_interface(i,'System.IEquatable`1'),
                    cleanup='Unity.Entities.ICleanupComponentData' in interfaces,
                    size_in_chunk=4, element_size=4, buffer_capacity=None, alignment_in_bytes=0,
                    maximum_chunk_capacity=128, entity_offsets=[])

    def offset(self, address):
        for va, n, raw in self.sections:
            if va <= address < va+n:
                return raw+address-va
        raise ValueError(f'VA {address:#x} has no file mapping')

    def qword(self, va):
        return struct.unpack_from('<Q', self.binary, self.offset(va))[0]

    def string(self, index):
        if index >= self.string_bytes:
            raise ValueError('string outside table')
        start = self.strings+index
        end = self.metadata.index(0, start, self.strings+self.string_bytes)
        return self.metadata[start:end].decode('utf-8')

    @functools.lru_cache(None)
    def native(self, index):
        if not 0 <= index < self.native_type_count:
            raise ValueError('native type index outside table')
        return self.native_ptr(self.qword(self.native_types+index*8))

    def native_ptr(self, ptr):
        data, bits = struct.unpack_from('<QI', self.binary, self.offset(ptr))
        return ((bits >> 16) & 255, data, bits & 65535)

    def typedef_for(self, desc):
        kind, data, _ = desc
        if kind in PRIMITIVES:
            return self.primitive_rows[PRIMITIVES[kind]]
        if kind in [17, 18] and data < self.type_count:
            return data
        raise Unsupported(f'native type kind {kind:#x} is not resolved by this probe')

    def descriptor_name(self, desc):
        if desc[0] in [15,29]:
            return self.descriptor_name(self.native_ptr(desc[1])) + ('*' if desc[0] == 15 else '[]')
        return self.type_rows[self.typedef_for(desc)]['name']

    def descriptor_namespace(self, desc):
        if desc[0] in [15,29]:
            return self.descriptor_namespace(self.native_ptr(desc[1]))
        return self.type_rows[self.typedef_for(desc)]['namespace']

    def terminal_descriptor_hash(self, desc):
        # HashType @0x8fcc0a0 skips fields of pointers/arrays; their Type.Name
        # includes the suffix and Type.Namespace is the element namespace.
        if desc[0] not in [15,29]:
            raise Unsupported('nonterminal descriptor requires field hash')
        namespace = self.descriptor_namespace(desc)
        ns = combine(BASIS,strhash(namespace)) if namespace else BASIS
        return combine(ns,strhash(self.descriptor_name(desc)))

    @functools.lru_cache(None)
    def namespace_hash(self, i):
        row = self.type_rows[i]
        if row['declaring'] >= 0:
            parent = self.typedef_for(self.native(row['declaring']))
            return combine(combine(BASIS, self.namespace_hash(parent)), strhash(self.type_rows[parent]['name']))
        return combine(BASIS, strhash(row['namespace'])) if row['namespace'] else BASIS

    @functools.lru_cache(None)
    def name_hash(self, i):
        row = self.type_rows[i]
        if row['generic'] != -1:
            raise Unsupported('generic definition or instantiation')
        return combine(self.namespace_hash(i), strhash(row['name']))

    def type_hash(self, i, cache):
        if i in cache:
            return cache[i]
        row = self.type_rows[i]
        value = self.name_hash(i)
        cache[i] = value  # The original caches the name hash before field recursion.
        # HashType skips primitive/enum/array/pointer/generic-parameter fields.
        if row['namespace'] == 'System' and row['name'] in PRIMITIVES.values() or row['bitfield'] & 2:
            return value
        # IL2CPP bakes FieldOffsetAttribute into its field-offset table: explicit-layout
        # types hash that offset (validated by 277 additional scene-hash matches).
        explicit = row['flags'] & 0x18 == 0x10
        for field in self.layout(i)['fields']:
            if not field['static']:
                if explicit:
                    value = combine(value, field['value_offset'] & MASK)
                field_hash = self.type_hash(field['typedef_index'], cache) if field['typedef_index'] is not None else self.terminal_descriptor_hash(self.native(field['native_type_index']))
                value = combine(value, field_hash)
        cache[i] = value
        return value

    @functools.lru_cache(None)
    def layout(self, i):
        row = self.type_rows[i]
        size_ptr = self.qword(self.type_sizes+i*8)
        sizes = struct.unpack_from('<IIII', self.binary, self.offset(size_ptr))
        offsets_ptr = self.qword(self.field_offsets+i*8)
        fields = []
        for n in range(row['field_count']):
            at = self.fields + (row['field_start']+n)*12
            name, native_type, _ = struct.unpack_from('<IiI', self.metadata, at)
            desc = self.native(native_type)
            try:
                typedef = self.typedef_for(desc)
            except Unsupported:
                if not desc[2] & 16 and desc[0] not in [15,29,21]:
                    raise
                typedef = None
            boxed_offset = struct.unpack_from('<i', self.binary, self.offset(offsets_ptr+n*4))[0]
            static = bool(desc[2] & 16)
            fields.append(dict(name=self.string(name), native_type_index=native_type,
                               typedef_index=typedef, static=static,
                               boxed_offset=boxed_offset,
                               value_offset=None if static else boxed_offset-(16 if row['bitfield'] & 1 else 0)))
        return dict(boxed_instance_size=sizes[0], managed_value_size=sizes[0]-16 if row['bitfield'] & 1 else None, native_size=sizes[1],
                    static_fields_size=sizes[2], thread_static_fields_size=sizes[3], fields=fields)

    def full_name(self, i):
        row = self.type_rows[i]
        if row['declaring'] >= 0:
            return self.full_name(self.typedef_for(self.native(row['declaring'])))+'+'+row['name']
        return row['namespace']+'.'+row['name'] if row['namespace'] else row['name']


def scene_hashes(directory):
    tag = bytes((n >> 4) | ((n & 15) << 4) for n in bytes.fromhex('F5364E1CCB62466A9883F6F9554D4F0C'))
    hashes, files = set(), 0
    for path in sorted(directory.glob('*.entities')):
        with path.open('rb') as stream:
            head = stream.read(152)
            if len(head) != 152 or head[:8] != b'DOTSBIN!' or struct.unpack_from('<i', head, 8)[0] != 77:
                raise ValueError(f'{path}: unsupported scene container')
            nodes, node_bytes = struct.unpack_from('<qi', head, 104)
            stream.seek(nodes)
            headers = stream.read(node_bytes)
            # This shipped build has one folder root and six 80-byte raw nodes.
            if len(headers) != 552:
                raise ValueError(f'{path}: expected the pinned build\'s six-stream node tree')
            for at in range(72, len(headers), 80):
                if headers[at+8:at+24] == tag:
                    data = struct.unpack_from('<q', headers, at+56)[0]
                    stream.seek(data)
                    count = struct.unpack('<i', stream.read(4))[0]
                    if not 0 <= count <= 16384:
                        raise ValueError(f'{path}: invalid type count')
                    hashes.update(struct.unpack('<'+'Q'*count, stream.read(count*8)))
                    files += 1
    return hashes, files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--install', type=Path, default=Path('..'))
    parser.add_argument('--server', action='store_true', help='read a dedicated-server install')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check', action='store_true', help='check pinned-build identity and scene-derived layout anchors')
    args = parser.parse_args()
    binary = args.install/'GameAssembly.dll'
    data = args.install/('VRisingServer_Data' if args.server else 'VRising_Data')
    metadata = data/'il2cpp_data/Metadata/global-metadata.dat'
    probe = Probe(binary, metadata)
    observed, files = scene_hashes(data/'StreamingAssets/EntityScenes')
    matched, unsupported = [], 0
    # Constructed-generic field signatures (native kind 0x15) and explicit layouts
    # are hashed by the generic identity probe; it reproduces the same algorithm.
    from probe_generic_identities import Identities
    identities = probe.identities = Identities(probe)
    for row in probe.type_rows:
        if not row['bitfield'] & 1:
            # Managed IComponentData classes: slot shape from BuildComponentType's managed branch.
            try:
                if 'Unity.Entities.IComponentData' not in probe.interface_names(row['index']) or row['generic'] >= 0:
                    continue
                stable = identities.stable_hash(identities.expr(probe.native(row['byval'])))
                if stable in observed:
                    matched.append(dict(name=probe.full_name(row['index']), stable_hash=f'{stable:016x}', metadata_type_index=row['index'],
                                        managed_value_size=0, native_size=None, ecs=probe.ecs_layout_managed_class(row['index'],stable)))
            except (Unsupported, RecursionError, struct.error):
                unsupported += 1
            continue
        try:
            stable = identities.stable_hash(identities.expr(probe.native(row['byval'])))
            if stable not in observed:
                continue
            matched.append(dict(name=probe.full_name(row['index']), stable_hash=f'{stable:016x}',
                                metadata_type_index=row['index'], **probe.layout(row['index']), ecs=probe.ecs_layout(row['index'],stable)))
        except (Unsupported, RecursionError):
            unsupported += 1
    # Scene hashes that belong to managed classes or constructed generics have no ECS
    # storage shape here; they are recorded as identities only (never read by layouts).
    known = {int(row['stable_hash'], 16) for row in matched}
    identity_matches, _ = identities.match_scene(observed, known)
    # Constructed generic components (e.g. Rukhanka blob records, structs holding ReferenceToken<Entity>):
    # shape from the sequential/explicit layout rule; scene acceptance cross-checks it.
    for stable, row in identity_matches.items():
        if stable in known:
            continue
        expression = identities.expressions[stable]
        if expression[0] != 'type' or not probe.type_rows[expression[1]]['bitfield'] & 1:
            continue
        try:
            ecs, name, size = probe.ecs_layout_generic(expression, stable)
        except (Unsupported, RecursionError, struct.error):
            continue
        matched.append(dict(name=row['name'], stable_hash=row['stable_hash'], metadata_type_index=expression[1], managed_value_size=size, native_size=None, ecs=ecs))
        known.add(stable)
    identity_only = [dict(name=row['name'], stable_hash=row['stable_hash'], generic_arguments=row['generic_arguments'])
                     for row in identity_matches.values() if int(row['stable_hash'], 16) not in known]
    identity_only.sort(key=lambda row: row['name'])
    unresolved = len(observed) - len(known) - len(identity_only)
    result = dict(schema_version=1, evidence_kind='offline-derived-and-scene-hash-matched',
                  binary_sha256=hashlib.sha256(probe.binary).hexdigest(),
                  metadata_sha256=hashlib.sha256(probe.metadata).hexdigest(),
                  registration_file_offset=f'{probe.registration_offset:#x}',
                  native_type_table=f'{probe.native_types:#x}', native_types=probe.native_type_count,
                  metadata_types=probe.type_count, scene_files=files, distinct_scene_hashes=len(observed),
                  matched_types=len(matched), unsupported_type_candidates=unsupported,
                  limitations=['Not a TypeInfo dump or runtime oracle',
                               'Hashes are matched against scene data and TypeVersion(0) is checked from type attributes',
                               'Does not resolve generics, pointer/array field types or explicit layout attributes',
                               'These are offline-derived layout inputs, not a runtime TypeInfo dump; write groups, reference-counting and serialization flags are not reconstructed'],
                  identity_only_count=len(identity_only), identity_only=identity_only, unresolved_scene_hashes=unresolved,
                  matches=sorted(matched,key=lambda item:item['name']))
    if args.check and args.server:
        if result['binary_sha256'] != '41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7' or result['metadata_sha256'] != '80ec821655b86d5a891c0268e3b10891de810535a099cbed8a45bfc647141bb5':
            raise ValueError('pinned server build identity changed')
        if files != 372 or len(observed) != 1390 or len(known) != 1390 or identity_only or unresolved:
            raise ValueError('pinned server scene layout coverage changed')
    if args.check and not args.server:
        if result['binary_sha256'] != 'a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56' or result['metadata_sha256'] != 'b8f9f76ffb72a0c77079082aac8eb78da839094e9d090e8461b49230e8831271':
            raise ValueError('pinned-build identity changed; anchors are not transferable')
        if len(matched) != EXPECTED_MATCHES or len({row['stable_hash'] for row in matched}) != EXPECTED_MATCHES:
            raise ValueError('pinned-build type-hash coverage changed')
        if result['identity_only_count'] != 0 or result['unresolved_scene_hashes'] != 0:
            raise ValueError('scene hash identity coverage changed')
        anchors = {
            'Unity.Entities.Entity': ('6f02124d960c5b29',8),
            'Unity.Transforms.Translation': ('a42de7cc0763e5be',12),
            'Unity.Transforms.Rotation': ('b1ac034499f49e3e',16),
            'Unity.Transforms.LocalToWorld': ('97490261b3c2de08',64),
            'Unity.Transforms.LocalTransform': ('5c5d60f217e41458',32),
        }
        by_name = {row['name']:row for row in matched}
        for name,(stable,size) in anchors.items():
            record = by_name[name]
            if (record['stable_hash'],record['ecs']['size_in_chunk']) != (stable,size):
                raise ValueError('layout anchor changed: '+name)
        for name, stable in [('Unity.Rendering.ManagedRenderMeshArray','f423fa42b77d7330'),('ProjectM.Terrain.PerChunkFluffDataComponent','a26e40a1d5cc937c')]:
            if by_name[name]['stable_hash'] != stable or by_name[name]['ecs']['category'] != 'shared' or not by_name[name]['ecs']['managed']:
                raise ValueError('managed shared-array identity changed: '+name)
        # Shapes recovered for the formerly identity-only types (see docs/COMPATIBILITY.md).
        managed = by_name['ProjectM.NetworkSnapshotIdentifier']['ecs']
        if (managed['managed'], managed['size_in_chunk'], managed['element_size'], managed['alignment_in_bytes']) != (True, 4, 4, 0):
            raise ValueError('managed IComponentData class slot shape changed')
        if sum(1 for row in matched if row['ecs']['managed'] and row['ecs']['category'] == 'component') != 61:
            raise ValueError('managed IComponentData class count changed')
        mount = by_name['ProjectM.MountModelStreaming']
        if (mount['ecs']['size_in_chunk'], mount['ecs']['entity_offsets']) != (28, [16]):
            raise ValueError('MountModelStreaming generic Entity-offset case changed')
        for name in ['Rukhanka.NewBlobAssetDatabaseRecord`1<Rukhanka.AnimationClipBlob>', 'Rukhanka.NewBlobAssetDatabaseRecord`1<Rukhanka.AvatarMaskBlob>']:
            record = by_name[name]['ecs']
            if (record['category'], record['element_size'], record['buffer_capacity'], record['size_in_chunk']) != ('buffer', 24, 5, 136):
                raise ValueError('constructed generic buffer shape changed: '+name)
        if not by_name['ProjectM.ProfessorCoilPairElement']['ecs']['iequatable'] or by_name['Unity.Entities.Entity']['ecs']['iequatable']:
            raise ValueError('IEquatable flag derivation changed')
        if by_name['ProjectM.VampireTag']['ecs']['size_in_chunk'] != 0:
            raise ValueError('empty tag unexpectedly consumes chunk payload')
        if by_name['ProjectM.TileModelRegistrationState']['managed_value_size'] != 1 or by_name['ProjectM.TileModelRegistrationState']['native_size'] != 4:
            raise ValueError('managed/marshalled bool-layout distinction lost')
        print('Pinned-build checks passed: '+str(EXPECTED_MATCHES)+' unique storage-shape matches plus '+str(result['identity_only_count'])+' identity-only (all 1772 scene hashes have a shape); Entity/transform anchors; zero-size tag; Boolean layout distinction.')
    if args.output:
        args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k!='matches'},indent=2))
    for item in result['matches']:
        if item['name'] in ['Unity.Transforms.Translation','Unity.Transforms.Rotation','Unity.Transforms.LocalToWorld','Unity.Transforms.LocalTransform','Unity.Entities.Entity']:
            print(json.dumps(item,indent=2))


if __name__ == '__main__':
    # Re-enter through the importable module so shared exception classes are identical
    # to the ones probe_generic_identities imports.
    import sys
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import probe_type_registry
    probe_type_registry.main()
