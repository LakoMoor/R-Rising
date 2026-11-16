#!/usr/bin/env python3
"""Measure how much of the shipped managed code R-Rising has ported.

Source of truth: the Il2CppDumper dump of the shipped `GameAssembly.dll`. The member list is kept
in `evidence/members.tsv` — one line per distinct (assembly, type, member) triple, produced from
`dump.cs` by `--emit-members` — so the measurement is reproducible from the repository alone.

The counting rule, and why it is this rule:

* The unit is the distinct (type, member) pair, not the overload. One Rust function that covers
  six C# overloads counts once; otherwise a single hand-written `clamp` would be credited with
  six methods it does not even distinguish.
* C# operators map to Rust traits: `op_Addition` counts when the crate declares `impl Add for
  <that type>`. Without that, a port that implements arithmetic as `+` reads as unimplemented.
  `op_Implicit`/`op_Explicit` map to `From`, because Rust has one infallible conversion trait where
  C# has two spellings of the same thing. The component-wise comparisons (`op_LessThan` and friends,
  which return `boolN`, not `bool`) have no trait to map to; the port spells them as methods and the
  table below accepts those names.
* `ctor` counts when the crate declares `pub fn new` for that type. `get_Foo`/`set_Foo` and the
  generated swizzles (`get_xyzw`) count when the crate declares an item named `foo`/`xyzw`.
  `.cctor` counts when the type declares `pub const`/`pub static` items, which is what a Rust
  static constructor exists to initialise in these value types.
* `op_Increment`, `op_Decrement` and `op_UnaryPlus` have no Rust counterpart for these value
  types, so they are excluded from the denominator instead of counted as missing.
* A name counts when it appears in the crate's compiled code, with comments and string literals
  stripped first. That is wider than "declared here" on purpose — macro-generated items have no
  literal names in the source — and it can over-count a name shared by two types in one crate.
* A member counts only for a type the crate actually mentions. Without this rule a Rust `clone`
  or `entries` anywhere in a crate would credit every C# type that has a method of that name,
  which is how an unstarted port reads as 0.2 % done.
* An assembly with no crate assigned to it yet counts in the totals as unported. That is what the
  overall number means: it is the share of the shipped binary the port can already reproduce.

Usage:
  tools/port_coverage.py --emit-members <inventory.jsonl>   # (re)build evidence/members.tsv
  tools/port_coverage.py                                     # measure the crates against it
  tools/port_coverage.py --top Unity.Mathematics             # what is still missing, by type
"""

import argparse
import collections
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MEMBERS = os.path.join(ROOT, "evidence", "members.tsv")

# Which crate is responsible for which original assembly.
CRATE_FOR_ASSEMBLY = [
    ("Unity.Mathematics", "rrising-math"),
    ("Unity.Entities", "rrising-ecs"),
    ("ProjectM", "rrising-core"),
    ("Stunlock", "rrising-core"),
]

# C# member name -> the Rust trait that stands in for it.
TRAITS = {
    "op_Addition": "Add",
    "op_Subtraction": "Sub",
    "op_Multiply": "Mul",
    "op_Division": "Div",
    "op_Modulus": "Rem",
    "op_AddAssignment": "AddAssign",
    "op_SubtractAssignment": "SubAssign",
    "op_MultiplyAssignment": "MulAssign",
    "op_DivideAssignment": "DivAssign",
    "op_ModuloAssignment": "RemAssign",
    "op_UnaryNegation": "Neg",
    "op_Equality": "PartialEq",
    "op_Inequality": "PartialEq",
    "op_LessThan": "PartialOrd",
    "op_LessThanOrEqual": "PartialOrd",
    "op_GreaterThan": "PartialOrd",
    "op_GreaterThanOrEqual": "PartialOrd",
    "op_BitwiseAnd": "BitAnd",
    "op_BitwiseOr": "BitOr",
    "op_ExclusiveOr": "BitXor",
    "op_OnesComplement": "Not",
    "op_LogicalNot": "Not",
    "op_LeftShift": "Shl",
    "op_RightShift": "Shr",
    "op_Implicit": "From",
    "op_Explicit": "From",
    "equals": "PartialEq",
    "Equals": "PartialEq",
    "getHashCode": "Hash",
    "GetHashCode": "Hash",
    "toString": "Display",
    "ToString": "Display",
    "CompareTo": "PartialOrd",
}

TRAIT_VALUES = set(TRAITS.values())

# Members with no Rust counterpart for these value types: excluded from the denominator.
NO_COUNTERPART = {"op_UnaryPlus", "op_Increment", "op_Decrement"}

# Operators with no Rust trait for these types (component-wise comparisons return `boolN`, not
# `bool`): the port spells them as methods, and these are the method names that count.
OPERATOR_METHODS = {
    "op_LessThan": "less_than",
    "op_LessThanOrEqual": "less_than_or_equal",
    "op_GreaterThan": "greater_than",
    "op_GreaterThanOrEqual": "greater_than_or_equal",
}

TRAIT_NAMES = "|".join(sorted(set(TRAITS.values())))


def snake(name):
    """`PerspectiveFov` -> `perspective_fov`, `xyzw` -> `xyzw`, `op_Addition` -> `op_addition`."""
    name = re.sub(r"[^A-Za-z0-9_]", "", name)
    out = []
    for i, ch in enumerate(name):
        if (
            ch.isupper()
            and i
            and (not name[i - 1].isupper() or (i + 1 < len(name) and name[i + 1].islower()))
        ):
            out.append("_")
        out.append(ch.lower())
    return "".join(out)


def crate_source(crate):
    path = os.path.join(ROOT, "crates", crate, "src")
    text = []
    for dirpath, _, files in os.walk(path):
        for fn in sorted(files):
            if fn.endswith(".rs"):
                with open(os.path.join(dirpath, fn), encoding="utf-8", errors="replace") as f:
                    text.append(f.read())
    # Generated code is part of the crate too.
    for dirpath, _, files in os.walk(os.path.join(ROOT, "crates", crate)):
        if os.path.basename(dirpath) == "generated":
            for fn in sorted(files):
                if fn.endswith(".rs"):
                    with open(os.path.join(dirpath, fn), encoding="utf-8", errors="replace") as f:
                        text.append(f.read())
    return "\n".join(text)


def strip_noise(src):
    """Drop comments and string literals: a name in a comment proves nothing, a name in code does."""
    src = re.sub(r"/\*.*?\*/", " ", src, flags=re.S)
    src = re.sub(r"//[^\n]*", " ", src)
    src = re.sub(r'"(?:[^"\\\\]|\\\\.)*"', '""', src)
    return src


def crate_items(crate):
    """The names a crate's code uses, plus the trait impls and `new`s it declares, per type.

    A name counts when it appears in compiled code (comments and strings stripped). That is wider
    than "declared here" on purpose: much of this port is generated by macros, whose item names
    are not literal text in the source. The cost is that a name shared with another type in the
    same crate counts too — a bias that is recorded rather than hidden.
    """
    src = strip_noise(crate_source(crate))
    names = {snake(t) for t in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", src)}
    traits = set()
    # `impl Trait for Type`, `impl std::ops::Add<Vec> for Vec` and `impl<T> From<T> for Vec` all
    # count: Rust spells the C# operator as a trait impl on the type, with the operand as a parameter.
    for m in re.finditer(
        r"impl(?:\s*<[^>]*>)?\s+(?:[A-Za-z0-9_:]*::)?(%s)(?:<[^<>]*>)?\s+for\s+([A-Za-z0-9_]+)" % TRAIT_NAMES,
        src,
    ):
        traits.add((snake(m.group(2)), m.group(1)))
    # `#[derive(PartialEq, Hash, ...)]` declares those traits just as surely as an explicit impl.
    # Other attributes may sit between the derive and the struct (`#[repr(C)]` does, in quat.rs), so
    # the pattern skips them rather than requiring the struct on the next line.
    for m in re.finditer(
        r"#\[derive\(([^)]*)\)\](?:\s*#[^\n]*)*\s*(?:pub\s+)?struct\s+([A-Za-z0-9_]+)", src
    ):
        for name in m.group(1).replace(" ", "").split(","):
            if name in TRAIT_VALUES:
                traits.add((snake(m.group(2)), name))
    has_new = set()
    has_statics = set()
    for m in re.finditer(r"impl(?:\s*<[^>]*>)?\s+([A-Za-z0-9_]+)\s*\{", src):
        body = src[m.end() : m.end() + 6000]
        if "fn new(" in body:
            has_new.add(snake(m.group(1)))
        if "pub const " in body or "pub static " in body:
            has_statics.add(snake(m.group(1)))
    return names, traits, has_new, has_statics


def load_members():
    if not os.path.exists(MEMBERS):
        sys.exit(
            f"{MEMBERS} is missing; run tools/port_coverage.py --emit-members <inventory.jsonl> first"
        )
    rows = []
    with open(MEMBERS, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) == 3:
                rows.append((parts[0], parts[1], parts[2]))
    return rows

def crate_for(assembly):
    for prefix, crate in CRATE_FOR_ASSEMBLY:
        if assembly.startswith(prefix):
            return crate
    return None


def measure(rows, verbose_types=None):
    cache = {}
    totals = collections.Counter()
    ported = collections.Counter()
    skipped = collections.Counter()
    missing = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    unattempted = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    for assembly, type_name, member in rows:
        group = group_of(assembly)
        if member in NO_COUNTERPART:
            skipped[group] += 1
            continue
        totals[group] += 1
        crate = crate_for(assembly)
        ok = False
        if crate:
            if crate not in cache:
                cache[crate] = crate_items(crate)
            names, traits, has_new, has_statics = cache[crate]
            # A member only counts if the crate knows the type that declares it: without this,
            # a Rust `clone` or `entries` in an unrelated crate would credit every C# type that
            # happens to have a method of the same name.
            if snake(type_name) not in names:
                if verbose_types is not None and group.startswith(verbose_types):
                    unattempted[group][snake(type_name)][member] += 1
                continue
            if member in TRAITS:
                trait = TRAITS[member]
                ok = (snake(type_name), trait) in traits
                if not ok and member in OPERATOR_METHODS:
                    # Component-wise comparisons return boolN in the original, so there is no
                    # trait to implement; the port's method name is the counterpart.
                    ok = OPERATOR_METHODS[member] in names
            elif member in ("cctor", ".cctor"):
                # A Rust static constructor is a `const`/`static` item, not code that runs.
                ok = snake(type_name) in has_statics
            elif member in ("ctor", ".ctor"):
                ok = snake(type_name) in has_new
            else:
                key = snake(re.sub(r"^(get_|set_)", "", member))
                ok = key in names
        if ok:
            ported[group] += 1
        elif crate is not None and verbose_types is not None and group.startswith(verbose_types):
            missing[group][snake(type_name)][member] += 1
    return totals, ported, skipped, missing, unattempted


def group_of(assembly):
    if assembly.startswith("ProjectM") or assembly.startswith("Stunlock"):
        return "game-authored (ProjectM*, Stunlock*)"
    if assembly.startswith("Unity.Mathematics"):
        return "Unity.Mathematics* (ported by rrising-math)"
    if assembly.startswith("Unity.Entities"):
        return "Unity.Entities* (ported by rrising-ecs)"
    if assembly.startswith("UnityEngine") or assembly.startswith("Unity."):
        return "engine assemblies (no crate assigned yet)"
    if assembly.startswith(("System", "mscorlib", "netstandard", "Mono")):
        return "BCL (no crate assigned yet)"
    return "middleware / other (no crate assigned yet)"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--emit-members", metavar="INVENTORY_JSONL")
    ap.add_argument("--json", action="store_true", help="emit machine-readable name-match metrics, not behavioral parity")
    ap.add_argument("--top", metavar="GROUP_PREFIX", help="list the types with the most gaps")
    args = ap.parse_args()

    if args.emit_members:
        seen = set()
        out = []
        with open(args.emit_members, encoding="utf-8") as f:
            for line in f:
                o = json.loads(line)
                type_name = o["decl"].split(":")[0].strip().split(".")[-1]
                for m in o["methods"]:
                    member = m["sig"].split("(")[0].strip()
                    key = (o["image"], type_name, member)
                    if key in seen:
                        continue
                    seen.add(key)
                    out.append(key)
        out.sort()
        os.makedirs(os.path.dirname(MEMBERS), exist_ok=True)
        with open(MEMBERS, "w", encoding="utf-8") as f:
            for a, t, m in out:
                f.write(f"{a}\t{t}\t{m}\n")
        print(f"wrote {len(out)} distinct (assembly, type, member) rows to {MEMBERS}")
        return

    rows = load_members()
    totals, ported, skipped, missing, unattempted = measure(rows, args.top)
    if args.json:
        print(json.dumps({
            "schema_version": 1,
            "metric": "heuristic_member_name_match",
            "behavioral_parity_percent": None,
            "warning": "Name matches can over-count; these numbers do not measure playable game completion or performance.",
            "matched": sum(ported.values()),
            "total": sum(totals.values()),
            "percent": round(100 * sum(ported.values()) / max(1, sum(totals.values())), 4),
            "groups": [{"name": group, "matched": ported[group], "total": totals[group],
                        "percent": round(100 * ported[group] / max(1, totals[group]), 4)}
                       for group in sorted(totals)]
        }, indent=2))
        return
    print(f"distinct (assembly, type, member) rows: {len(rows)}")
    print(f"excluded, no Rust counterpart: {sum(skipped.values())}")
    print()
    print(f"{'group':52} {'ported':>7} {'total':>7} {'share':>7}")
    grand_t = grand_p = 0
    for group in sorted(totals, key=lambda g: (-ported[g] / max(1, totals[g]), g)):
        t, p = totals[group], ported[group]
        grand_t += t
        grand_p += p
        print(f"{group:52} {p:7} {t:7} {100.0 * p / max(1, t):6.1f} %")
    print(f"{'ALL':52} {grand_p:7} {grand_t:7} {100.0 * grand_p / max(1, grand_t):6.1f} %")

    if args.top:
        groups = [g for g in totals if g.lower().startswith(args.top.lower())]
        print(f"\ntypes with the most gaps in {args.top!r}:")
        for g in groups:
            ranked = sorted(missing[g].items(), key=lambda kv: (-len(kv[1]), kv[0]))[:20]
            for t, c in ranked:
                print(f"   {t:22} {len(c):5}  {', '.join(sorted(c)[:10])}")
            if not ranked:
                print(f"   (none in {g} — every member the dump lists there is accounted for)")
            not_attempted = sorted(unattempted[g].items(), key=lambda kv: (-sum(kv[1].values()), kv[0]))[:20]
            if not_attempted:
                print(f"\ntypes in {g!r} the crate does not declare yet (in the total, not in the gap list):")
                for t, c in not_attempted:
                    print(f"   {t:22} {sum(c.values()):5}  {', '.join(sorted(c)[:10])}")


if __name__ == "__main__":
    main()
