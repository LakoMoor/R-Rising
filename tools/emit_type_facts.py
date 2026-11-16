#!/usr/bin/env python3
"""Extract the per-type facts `Unity.Mathematics` ships, from `dump.cs`.

`evidence/members.tsv` collapses overloads to distinct member names, which is right for counting
coverage but too coarse to drive a generator: `half3` declares `op_Explicit(float3)` while `float3`
declares `op_Implicit(half3)`, and both are just a row named `op_Explicit`/`op_Implicit`. Same for
the static fields — `half3` has only `zero`, while `float3` has `zero, one, eps, min_value,
max_value, default_value`. This keeps the signatures and the field names, so the generator can emit
exactly what the dump lists.

Writes three files:
  <prefix>_conversions.tsv   type, kind (implicit|explicit), from, to
  <prefix>_fields.tsv        type, field
  <prefix>_tostring.tsv      type, placeholder count, the `ToString` format literal

The third comes from `stringliteral.json` rather than `dump.cs`: every `Unity.Mathematics` value
type's `ToString` formats a lazily-materialised literal of the shape `TypeName(args)`, and the
literal table has them all. The placeholder count is recorded so the generator can pick the entry
that matches the type's own component count (`float4x4` has both a 16-argument ToString and a
7-argument helper format).

usage: emit_type_facts.py dump.cs conversions.tsv fields.tsv [stringliteral.json tostring.tsv]
"""
import re
import sys

DUMP = sys.argv[1]
CONV_OUT = sys.argv[2]
FIELD_OUT = sys.argv[3]
STRINGLIT = sys.argv[4] if len(sys.argv) > 4 else None
TS_OUT = sys.argv[5] if len(sys.argv) > 5 else None

TYPE_RE = re.compile(r"^public (?:sealed |static |abstract |partial )*(?:struct|class|enum|interface)\s+([A-Za-z0-9_.]+)")
OP_RE = re.compile(r"public static ([A-Za-z0-9_]+) (op_Implicit|op_Explicit)\(([A-Za-z0-9_]+)[^)]*\)")
FIELD_RE = re.compile(r"^\s*public static readonly (?:readonly )?([A-Za-z0-9_]+)\s+([A-Za-z0-9_]+);")

conversions = set()
fields = set()
type_names = set()
cur = None
in_ns = False
with open(DUMP, encoding="utf-8", errors="replace") as f:
    for line in f:
        if line.startswith("// Namespace: "):
            in_ns = line.strip() == "// Namespace: Unity.Mathematics"
            cur = None
            continue
        if not in_ns:
            continue
        m = TYPE_RE.match(line)
        if m:
            cur = m.group(1).split(".")[-1]
            type_names.add(cur)
            continue
        if cur is None:
            continue
        m = OP_RE.search(line)
        if m:
            ret, kind, arg = m.group(1), m.group(2), m.group(3)
            conversions.add((cur, kind, arg, ret))
            continue
        m = FIELD_RE.match(line)
        if m:
            fields.add((cur, m.group(2)))

with open(CONV_OUT, "w", encoding="utf-8") as f:
    for r in sorted(conversions):
        f.write("\t".join(r) + "\n")
with open(FIELD_OUT, "w", encoding="utf-8") as f:
    for r in sorted(fields):
        f.write("\t".join(r) + "\n")
print(f"wrote {len(conversions)} conversion signatures to {CONV_OUT}")
print(f"wrote {len(fields)} static field rows to {FIELD_OUT}")

if STRINGLIT and TS_OUT:
    import json

    with open(STRINGLIT, encoding="utf-8") as f:
        literals = json.load(f)
    tostring = set()
    for e in literals:
        v = e.get("value", "")
        m = re.match(r"^([A-Za-z0-9_]+)\(", v)
        if not m or m.group(1) not in type_names:
            continue
        if "\t" in v or "\n" in v:
            continue
        tostring.add((m.group(1), len(re.findall(r"\{\d+\}", v)), v))
    with open(TS_OUT, "w", encoding="utf-8") as f:
        for name, n, v in sorted(tostring):
            f.write(f"{name}\t{n}\t{v}\n")
    print(f"wrote {len(tostring)} ToString formats to {TS_OUT}")
