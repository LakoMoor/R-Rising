//! Generates the `Unity.Mathematics` vector surface that the original gets from its own source
//! generator.
//!
//! The type list and the operator shapes are not guessed: they are read from
//! `evidence/members.tsv` (which `tools/port_coverage.py` derives from the shipped `dump.cs`) and
//! from the dump's own signatures. For every vector type the dump lists, this emits the components,
//! the static fields, the constructors, the full swizzle set (`get_xx` … `get_zzzz`,
//! `set_xy` … `set_xyzw`), the indexer, `Equals`/`GetHashCode`/`ToString` and the operators —
//! including the scalar-left and scalar-right forms the dump declares, and the `int` right side of
//! the integer shifts.
//!
//! The output is written twice: to `OUT_DIR` (what the crate compiles) and to `generated/` next to
//! the crate, so a reviewer can read the exact surface the port exposes and the coverage tool can
//! measure it.
//!
//! `half2`/`half3`/`half4` are generated from the same ledger as the rest: the scalar `half` is in
//! `src/binary16.rs`, transcribed from its own decompiled bodies. Conversions are gated on
//! `evidence/math_conversions.tsv` because `members.tsv` cannot tell `half3(float3)` (the dump has
//! it) from `half3(int3)` (it does not).

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

/// One vector family: the Rust scalar type plus which operator groups the dump gives it.
struct Family {
    name: &'static str,
    scalar: &'static str,
    zero: &'static str,
    one: &'static str,
    /// `eps` exists only on the floating families.
    eps: Option<&'static str>,
    min: &'static str,
    max: &'static str,
    signed: bool,
    /// Which families the dump gives bitwise operators and shifts to. Nothing branches on it now:
    /// the generator reads that from the ledger, so the field is kept as documentation.
    #[allow(dead_code)]
    integral: bool,
    boolean: bool,
    floating: bool,
    /// `half` is the odd one out: the dump gives it no arithmetic operators at all, its conversions
    /// go through `math.f32tof16`/`math.f16tof32` rather than a cast, and the `math` statics
    /// (`dot`/`cross`/`length`) have no binary16 overloads.
    binary16: bool,
}

const FAMILIES: &[Family] = &[
    Family {
        name: "bool",
        scalar: "bool",
        zero: "false",
        one: "true",
        eps: None,
        min: "false",
        max: "true",
        signed: false,
        integral: false,
        boolean: true,
        floating: false,
        binary16: false,
    },
    Family {
        name: "int",
        scalar: "i32",
        zero: "0",
        one: "1",
        eps: None,
        min: "i32::MIN",
        max: "i32::MAX",
        signed: true,
        integral: true,
        boolean: false,
        floating: false,
        binary16: false,
    },
    Family {
        name: "uint",
        scalar: "u32",
        zero: "0",
        one: "1",
        eps: None,
        min: "u32::MIN",
        max: "u32::MAX",
        signed: false,
        integral: true,
        boolean: false,
        floating: false,
        binary16: false,
    },
    Family {
        name: "float",
        scalar: "f32",
        zero: "0.0",
        one: "1.0",
        eps: Some("f32::EPSILON"),
        min: "f32::MIN",
        max: "f32::MAX",
        signed: true,
        integral: false,
        boolean: false,
        floating: true,
        binary16: false,
    },
    Family {
        name: "double",
        scalar: "f64",
        zero: "0.0",
        one: "1.0",
        eps: Some("f64::EPSILON"),
        min: "f64::MIN",
        max: "f64::MAX",
        signed: true,
        integral: false,
        boolean: false,
        floating: true,
        binary16: false,
    },
    Family {
        name: "half",
        scalar: "half",
        zero: "half::zero",
        one: "half::one",
        eps: Some("half::eps"),
        min: "half::min_value_as_half",
        max: "half::max_value_as_half",
        signed: true,
        integral: false,
        boolean: false,
        floating: true,
        binary16: true,
    },
];

/// Component letters per width, matching the dump's field names.
const LETTERS: [&str; 4] = ["x", "y", "z", "w"];

/// Every string of length `len` over `letters`, lexicographic — the order the original's generator
/// emits them in, which is also the order `dump.cs` lists them.
fn swizzles(letters: &[&str], len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = vec![0usize; len];
    loop {
        out.push(cur.iter().map(|&i| letters[i]).collect());
        // Odometer: carry into the last position that can still advance.
        let mut pos = len;
        loop {
            if pos == 0 {
                return out;
            }
            pos -= 1;
            if cur[pos] + 1 < letters.len() {
                cur[pos] += 1;
                cur[pos + 1..].fill(0);
                break;
            }
        }
    }
}

/// Length-`len` arrangements of *distinct* letters, which is what a swizzle setter can be:
/// `set_xxy` would assign `x` twice, and the dump indeed has no `set_xxy`.
fn permutations(letters: &[&str], len: usize) -> Vec<String> {
    fn walk(
        letters: &[&str],
        used: &mut Vec<bool>,
        cur: &mut String,
        len: usize,
        out: &mut Vec<String>,
    ) {
        if cur.len() == len {
            out.push(cur.clone());
            return;
        }
        for i in 0..letters.len() {
            if !used[i] {
                used[i] = true;
                cur.push_str(letters[i]);
                walk(letters, used, cur, len, out);
                cur.truncate(cur.len() - 1);
                used[i] = false;
            }
        }
    }
    let mut out = Vec::new();
    let mut used = vec![false; letters.len()];
    let mut cur = String::new();
    walk(letters, &mut used, &mut cur, len, &mut out);
    out
}

fn scalar_of(family: &Family, len: usize) -> String {
    format!("{}{}", family.name, len)
}

/// The Rust expression converting component `c` of `v` (a `src` vector) to the `dst` scalar type.
/// Rust forbids `int as bool`, and forbids `bool as f32`, so those two directions need the extra
/// step the original's cast performs anyway.
fn cast_expr(src: &Family, dst: &Family, c: &str) -> String {
    if src.binary16 || dst.binary16 {
        return format!("v.{c}.into()");
    }
    if dst.boolean {
        if src.boolean {
            format!("v.{c}")
        } else {
            format!("v.{c} != {}", src.zero)
        }
    } else if src.boolean && dst.floating {
        format!("v.{c} as i32 as {}", dst.scalar)
    } else {
        format!("v.{c} as {}", dst.scalar)
    }
}

/// The Rust expression converting a `src` scalar named `v` to the `dst` scalar type.
fn cast_scalar(src: &Family, dst: &Family) -> String {
    if src.name == dst.name {
        return "v".to_string();
    }
    // `half` converts through `math.f32tof16`/`math.f16tof32`, not a Rust cast; the `From` impls
    // in `src/half.rs` are those two functions, so `.into()` is the same code the body runs.
    if src.binary16 || dst.binary16 {
        return "v.into()".to_string();
    }
    if dst.boolean {
        if src.boolean {
            "v".to_string()
        } else {
            format!("v != {}", src.zero)
        }
    } else if src.boolean && dst.floating {
        format!("v as i32 as {}", dst.scalar)
    } else {
        format!("v as {}", dst.scalar)
    }
}

/// The dump's conversion operators, as `(from, to) -> "op_Implicit" | "op_Explicit"`.
///
/// With no ledger — a clean checkout before `tools/emit_type_facts.py` has run — every pair is
/// allowed, which is what this generator did before the ledger existed.
struct ConversionLedger {
    pairs: BTreeMap<(String, String), String>,
}

impl ConversionLedger {
    fn has(&self, from: &str, to: &str) -> bool {
        self.pairs.is_empty() || self.pairs.contains_key(&(from.to_string(), to.to_string()))
    }

    /// Which operator the dump spells this conversion as, for the doc comment.
    fn note(&self, from: &str, to: &str) -> &'static str {
        match self.pairs.get(&(from.to_string(), to.to_string())) {
            Some(k) if k == "op_Implicit" => "op_Implicit",
            Some(_) => "op_Explicit",
            None => "op_Implicit",
        }
    }
}

/// The shipped `ToString` format strings, as `(type, placeholder count) -> format`, from
/// `evidence/math_tostring.tsv`.
///
/// The count is part of the key because the literal table holds more than one format per type name:
/// `float4x4` has its own 16-argument ToString *and* a 7-argument helper format, and the generator
/// wants the one whose argument count is the type's element count.
///
/// With no ledger — a clean checkout before `tools/emit_type_facts.py` has run — the generator
/// builds the same shape itself, so the build still works.
struct FormatLedger {
    by: BTreeMap<(String, usize), String>,
}

impl FormatLedger {
    /// The format for `ty` with `n` components. Falls back to `synth` when the ledger is absent
    /// (clean checkout); panics when the ledger exists but has no row for this shape, because that
    /// means the generator and the shipped metadata disagree.
    fn get_or(&self, ty: &str, n: usize, synth: &str) -> String {
        match self.by.get(&(ty.to_string(), n)) {
            Some(f) => f.clone(),
            None if self.by.is_empty() => synth.to_string(),
            None => panic!(
                "no ToString format for {ty} with {n} components in evidence/math_tostring.tsv"
            ),
        }
    }
}

/// The `Display` impl that reproduces a shipped format string. `{k}` placeholders become Rust's
/// `{}` in order, and the literal text between them is kept exactly — including the `f` the float
/// families append to each number and the two spaces between matrix rows.
fn display_impl(ty: &str, fmt: &str, args: &[String]) -> String {
    let mut text = String::new();
    let mut rest = fmt;
    let mut used = 0usize;
    while let Some(i) = rest.find('{') {
        text.push_str(&rest[..i]);
        let end = rest[i..].find('}').unwrap_or_else(|| {
            panic!("unterminated placeholder in the {ty} ToString format {fmt:?}")
        });
        let idx: usize = rest[i + 1..i + end]
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric placeholder in {fmt:?}"));
        if idx != used {
            panic!("the {ty} ToString format does not number its placeholders in order: {fmt:?}");
        }
        used += 1;
        text.push_str("{}");
        rest = &rest[i + end + 1..];
    }
    text.push_str(rest);
    if used != args.len() {
        panic!(
            "the {ty} ToString format takes {used} arguments, the type has {} components",
            args.len()
        );
    }
    let lit = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "\nimpl std::fmt::Display for {ty} {{\n    /// `ToString()` — the shipped format string for this\n    /// type, kept verbatim (`evidence/math_tostring.tsv`): the name, the arguments in\n    /// declaration order, and the separators the literal uses.\n    #[inline]\n    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        write!(w, \"{lit}\",\n            {},\n        )\n    }}\n}}\n",
        args.join(",\n            ")
    )
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let members_path = Path::new(&manifest).join("../../evidence/members.tsv");
    println!("cargo:rerun-if-changed={}", members_path.display());

    let mut listed: BTreeMap<String, usize> = BTreeMap::new();
    let mut mats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut ledger: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    // The conversion operators, as `(from, to) -> Implicit|Explicit`. `members.tsv` collapses
    // overloads to distinct names, so it cannot tell the conversions `half3` declares (`float3`,
    // `double3` and the scalar splats) from the ones it does not (`int3`, `uint3`, `bool3`); the
    // signatures live in a second ledger so the generator emits exactly the dump's set.
    let conv_path = Path::new(&manifest).join("../../evidence/math_conversions.tsv");
    println!("cargo:rerun-if-changed={}", conv_path.display());
    let mut pairs: BTreeMap<(String, String), String> = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(&conv_path) {
        for line in text.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() == 4 {
                pairs.insert(
                    (cols[2].to_string(), cols[3].to_string()),
                    cols[1].to_string(),
                );
            }
        }
    }
    let conv = ConversionLedger { pairs };
    // The `ToString` format literals, keyed by (type, argument count).
    let ts_path = Path::new(&manifest).join("../../evidence/math_tostring.tsv");
    println!("cargo:rerun-if-changed={}", ts_path.display());
    let mut by: BTreeMap<(String, usize), String> = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(&ts_path) {
        for line in text.lines() {
            let cols: Vec<&str> = line.splitn(3, '\t').collect();
            if cols.len() == 3 {
                if let Ok(n) = cols[1].parse::<usize>() {
                    by.insert((cols[0].to_string(), n), cols[2].to_string());
                }
            }
        }
    }
    let fmts = FormatLedger { by };
    if let Ok(text) = fs::read_to_string(&members_path) {
        'lines: for line in text.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() != 3 || cols[0] != "Unity.Mathematics.dll" {
                continue;
            }
            for f in FAMILIES {
                let rest = match cols[1].strip_prefix(f.name) {
                    Some(rest) => rest,
                    None => continue,
                };
                if let Ok(w) = rest.parse::<usize>() {
                    if (2..=4).contains(&w) {
                        listed.insert(cols[1].to_string(), w);
                        ledger
                            .entry(cols[1].to_string())
                            .or_default()
                            .insert(cols[2].to_string());
                    }
                    continue 'lines;
                }
                // `{rows}x{cols}`: the matrix families the dump lists next to the vectors.
                if let Some((r, c)) = rest.split_once('x') {
                    if let (Ok(r), Ok(c)) = (r.parse::<usize>(), c.parse::<usize>()) {
                        if (2..=4).contains(&r) && (2..=4).contains(&c) {
                            mats.insert(cols[1].to_string(), (r, c));
                            ledger
                                .entry(cols[1].to_string())
                                .or_default()
                                .insert(cols[2].to_string());
                        }
                        continue 'lines;
                    }
                }
            }
        }
    }
    // The ledger is regenerated after this build script on a clean checkout, so fall back to the
    // family/width grid the dump is known to contain rather than emitting nothing.
    if listed.is_empty() {
        for f in FAMILIES {
            for w in 2..=4 {
                listed.insert(format!("{}{}", f.name, w), w);
            }
        }
    }
    if mats.is_empty() {
        for f in FAMILIES {
            for r in 2..=4 {
                for c in 2..=4 {
                    mats.insert(format!("{}{}x{}", f.name, r, c), (r, c));
                }
            }
        }
    }

    let mut out: Vec<u8> = Vec::new();
    out.write_all(b"// Generated by `crates/rrising-math/build.rs` from `evidence/members.tsv`,\n// `evidence/math_conversions.tsv`, `evidence/math_fields.tsv` and `evidence/math_tostring.tsv`.\n// Do not edit by hand; edit the generator.\n\n").unwrap();

    let mut emitted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (ty, width) in &listed {
        let family = FAMILIES
            .iter()
            .find(|f| ty.starts_with(f.name))
            .expect("listed family");
        emitted.insert(
            ty.clone(),
            gen_type(
                &mut out,
                family,
                *width,
                &listed,
                ledger.get(ty),
                &conv,
                &fmts,
            ),
        );
    }
    for (ty, (rows, cols)) in &mats {
        let family = FAMILIES
            .iter()
            .find(|f| ty.starts_with(f.name))
            .expect("listed family");
        emitted.insert(
            ty.clone(),
            gen_matrix(
                &mut out,
                family,
                *rows,
                *cols,
                &mats,
                ledger.get(ty),
                (&conv, &fmts),
            ),
        );
    }

    // Cross-check against the dump: the property surface must be exactly what `dump.cs` declares,
    // so a generator bug cannot silently drop `zyx` or invent a `set_xx` the original lacks.
    for (ty, members) in &ledger {
        let want: BTreeSet<String> = members
            .iter()
            .filter(|m| m.starts_with("get_") || m.starts_with("set_"))
            .cloned()
            .collect();
        let got = emitted
            .get(ty)
            .unwrap_or_else(|| panic!("{ty} was not generated"));
        let missing: Vec<&String> = want.difference(got).collect();
        let extra: Vec<&String> = got.difference(&want).collect();
        if !missing.is_empty() || !extra.is_empty() {
            panic!("swizzle surface for {ty} does not match the dump: missing {missing:?}, extra {extra:?}");
        }
    }

    // Conversions: every same-width vector pair, and every scalar to every vector (the dump's
    // `op_Implicit(float v)` / `op_Explicit(bool v)` splat conversions).
    for w in 2..=4 {
        for a in FAMILIES {
            for b in FAMILIES {
                if a.name == b.name {
                    continue;
                }
                let (src, dst) = (scalar_of(a, w), scalar_of(b, w));
                if !listed.contains_key(&src) || !listed.contains_key(&dst) || !conv.has(&src, &dst)
                {
                    continue;
                }
                let body = LETTERS[..w]
                    .iter()
                    .map(|c| format!("        {}: {},\n", c, cast_expr(a, b, c)))
                    .collect::<String>();
                let note = conv.note(&src, &dst);
                let _ = writeln!(out, "\nimpl From<{src}> for {dst} {{\n    /// `{src} -> {dst}`; the dump declares `{note}`.\n    #[inline]\n    fn from(v: {src}) -> {dst} {{\n        {dst} {{\n{body}        }}\n    }}\n}}");
            }
            // Scalar -> vector: the dump's `op_Implicit(float v)` / `op_Explicit(bool v)` splats.
            for b in FAMILIES {
                let dst = scalar_of(b, w);
                if !listed.contains_key(&dst) || !conv.has(a.name, &dst) {
                    continue;
                }
                let body = LETTERS[..w]
                    .iter()
                    .map(|c| format!("        {c}: {},\n", cast_scalar(a, b)))
                    .collect::<String>();
                let note = conv.note(a.name, &dst);
                let _ = writeln!(out, "\nimpl From<{}> for {dst} {{\n    /// `{} -> {dst}`; the dump declares `{note}`, splatting the scalar.\n    #[inline]\n    fn from(v: {}) -> {dst} {{\n        {dst} {{\n{body}        }}\n    }}\n}}", a.scalar, a.scalar, a.scalar);
            }
        }
    }

    // Matrix conversions: same shape across families, and scalar to matrix — the dump declares
    // `op_Implicit(float v)` / `op_Explicit(bool v)` on every matrix type too.
    let shapes: BTreeSet<(usize, usize)> = mats.values().copied().collect();
    for &(rows, cols) in &shapes {
        for a in FAMILIES {
            for b in FAMILIES {
                let src = format!("{}{}x{}", a.name, rows, cols);
                let dst = format!("{}{}x{}", b.name, rows, cols);
                if a.name == b.name
                    || !mats.contains_key(&src)
                    || !mats.contains_key(&dst)
                    || !conv.has(&src, &dst)
                {
                    continue;
                }
                let body = (0..cols)
                    .map(|j| format!("        c{j}: v.c{j}.into(),\n"))
                    .collect::<String>();
                let note = conv.note(&src, &dst);
                let _ = writeln!(out, "\nimpl From<{src}> for {dst} {{\n    /// `{src} -> {dst}`; the dump declares `{note}`.\n    #[inline]\n    fn from(v: {src}) -> {dst} {{\n        {dst} {{\n{body}        }}\n    }}\n}}");
            }
            for b in FAMILIES {
                let mty = format!("{}{}x{}", b.name, rows, cols);
                let mcol = format!("{}{}", b.name, rows);
                if !mats.contains_key(&mty) || !conv.has(a.name, &mty) {
                    continue;
                }
                let expr = cast_scalar(a, b);
                let body = (0..cols)
                    .map(|j| format!("        c{j}: {mcol}::splat({expr}),\n"))
                    .collect::<String>();
                let note = conv.note(a.name, &mty);
                let _ = writeln!(out, "\nimpl From<{}> for {mty} {{\n    /// `{} -> {mty}`; the dump declares `{note}`, splatting the scalar into every column.\n    #[inline]\n    fn from(v: {}) -> {mty} {{\n        {mty} {{\n{body}        }}\n    }}\n}}", a.scalar, a.scalar, a.scalar);
            }
        }
    }
    // Cross-shape matrix conversions. The dump declares exactly one: `float3x3 op_Explicit(float4x4)`
    // @ 0x92bac10, whose body copies the top-left 3x3 out of the 4x4's column-major elements.
    for ((from, to), kind) in &conv.pairs {
        let (f_r, f_c) = match mats.get(from.as_str()) {
            Some(s) => *s,
            None => continue,
        };
        let (t_r, t_c) = match mats.get(to.as_str()) {
            Some(s) => *s,
            None => continue,
        };
        if (f_r, f_c) == (t_r, t_c) {
            continue;
        }
        assert!(
            t_r <= f_r && t_c <= f_c,
            "matrix conversion {from} -> {to} is not a truncation"
        );
        let family = FAMILIES
            .iter()
            .find(|f| from.starts_with(f.name))
            .expect("matrix family");
        let t_col = format!("{}{}", family.name, t_r);
        let cols = (0..t_c)
            .map(|j| {
                let comps = LETTERS[..t_r]
                    .iter()
                    .map(|c| format!("v.c{j}.{c}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("        c{j}: {t_col}::new({comps}),\n")
            })
            .collect::<String>();
        let note = if kind == "op_Implicit" {
            "op_Implicit"
        } else {
            "op_Explicit"
        };
        let _ = writeln!(out, "\nimpl From<{from}> for {to} {{\n    /// `{from} -> {to}`; the dump declares `{note}`, and the body copies the top-left {t_r}x{t_c} of the source's column-major elements.\n    #[inline]\n    fn from(v: {from}) -> {to} {{\n        {to} {{\n{cols}        }}\n    }}\n}}");
    }
    // Cross-check the conversions the same way the swizzles are checked: every `impl From<S> for D`
    // emitted must be a conversion the dump declares, and every conversion the dump declares between
    // two types this generator emits must have been emitted. The four scalar `half` conversions are
    // hand-written in `src/binary16.rs`, so they are excluded here rather than duplicated.
    if !conv.pairs.is_empty() {
        let text = String::from_utf8(out.clone()).expect("generated utf-8");
        let mut got: BTreeSet<(String, String)> = BTreeSet::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("impl From<") {
                if let Some((s, d)) = rest.split_once("> for ") {
                    got.insert((
                        s.trim().to_string(),
                        d.trim_end_matches(" {").trim().to_string(),
                    ));
                }
            }
        }
        let rust_name = |n: &str| -> String {
            FAMILIES
                .iter()
                .find(|f| f.name == n)
                .map(|f| f.scalar.to_string())
                .unwrap_or_else(|| n.to_string())
        };
        let types: BTreeSet<String> = listed
            .keys()
            .chain(mats.keys())
            .cloned()
            .chain(FAMILIES.iter().map(|f| f.scalar.to_string()))
            .collect();
        let hand_written: BTreeSet<(String, String)> = [
            ("f32", "half"),
            ("f64", "half"),
            ("half", "f32"),
            ("half", "f64"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let want: BTreeSet<(String, String)> = conv
            .pairs
            .keys()
            .map(|(from, to)| (rust_name(from), rust_name(to)))
            .filter(|(from, to)| types.contains(from) && types.contains(to))
            .filter(|p| !hand_written.contains(p))
            .collect();
        let missing: Vec<&(String, String)> = want.difference(&got).collect();
        let extra: Vec<&(String, String)> = got.difference(&want).collect();
        if !missing.is_empty() || !extra.is_empty() {
            panic!(
                "conversion surface does not match the dump: missing {missing:?}, extra {extra:?}"
            );
        }
    }
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let gen_dir = Path::new(&manifest).join("generated");
    let _ = fs::create_dir_all(&gen_dir);
    for p in [
        Path::new(&out_dir).join("vectors.rs"),
        gen_dir.join("vectors.rs"),
    ] {
        fs::write(p, &out).expect("write generated vectors");
    }
}

fn gen_type(
    out: &mut Vec<u8>,
    f: &Family,
    w: usize,
    listed: &BTreeMap<String, usize>,
    members: Option<&BTreeSet<String>>,
    conv: &ConversionLedger,
    fmts: &FormatLedger,
) -> BTreeSet<String> {
    let ty = format!("{}{}", f.name, w);
    let letters = &LETTERS[..w];
    let fields = letters
        .iter()
        .map(|c| format!("pub {c}: {},", f.scalar))
        .collect::<Vec<_>>()
        .join(" ");
    let mut src = String::new();
    let mut names = BTreeSet::new();
    // Every operator below is gated on the dump's own member list for this type, so the port
    // cannot invent an operator the original lacks or silently drop one it has. With no ledger
    // (a clean checkout before `--emit-members` has run) the family's full set is emitted.
    let has = |m: &str| members.map(|s| s.contains(m)).unwrap_or(true);

    // `f32`/`f64` implement neither `Eq` nor `Hash` in Rust, which matches the original: its
    // `Equals` is component-wise `==` (NaN != NaN) and its `GetHashCode` hashes the raw bits.
    let derives = if f.floating {
        "Clone, Copy, PartialEq, Debug, Default"
    } else {
        "Clone, Copy, PartialEq, Eq, Hash, Debug, Default"
    };
    src += &format!("\n/// `Unity.Mathematics.{ty}` — components in the dump's order, at its offsets.\n#[repr(C)]\n#[derive({derives})]\npub struct {ty} {{ {fields} }}\n");
    if f.floating {
        let hash_body = letters
            .iter()
            .map(|c| format!("        self.{c}.to_bits().hash(state);\n"))
            .collect::<String>();
        src += &format!("\nimpl std::hash::Hash for {ty} {{\n    /// `GetHashCode()`: the bit patterns, which is what the original hashes.\n    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{\n{hash_body}    }}\n}}\n");
    }

    // Static fields the dump lists as `public static readonly`.
    let field = |n: &str, v: &str| {
        format!(
            "    /// `{ty}.{n}`.\n    pub const {n}: {ty} = {ty} {{ {} }};\n",
            letters
                .iter()
                .map(|c| format!("{c}: {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    src += &format!("impl {ty} {{\n");
    src += &field("zero", f.zero);
    src += &field("one", f.one);
    if let Some(e) = f.eps {
        src += &field("eps", e);
    }
    src += &field("min_value", f.min);
    src += &field("max_value", f.max);
    src += &field("default_value", f.zero);
    if f.boolean {
        src += &field("truevalue", "true");
        src += &field("falsevalue", "false");
    }

    // Constructors. C# overloads cannot exist in Rust, so the all-components form keeps `new`,
    // the scalar form is `splat` and the copy form is `copy`; the dump groups them under the
    // single `.ctor` member.
    let all_scalars = letters
        .iter()
        .map(|c| format!("{c}: {}", f.scalar))
        .collect::<Vec<_>>()
        .join(", ");
    let all_init = letters
        .iter()
        .map(|c| format!("{c}: {c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let splat_init = letters
        .iter()
        .map(|c| format!("{c}: v"))
        .collect::<Vec<_>>()
        .join(", ");
    src += &format!("    /// `{ty}({all_scalars})`.\n    #[inline]\n    pub const fn new({all_scalars}) -> {ty} {{ {ty} {{ {all_init} }} }}\n");
    src += &format!("    /// `{ty}({} v)` — every component set to `v`.\n    #[inline]\n    pub const fn splat(v: {}) -> {ty} {{ {ty} {{ {splat_init} }} }}\n", f.scalar, f.scalar);
    src += &format!(
        "    /// `{ty}({ty} v)`.\n    #[inline]\n    pub const fn copy(v: {ty}) -> {ty} {{ v }}\n"
    );
    for other in FAMILIES {
        let other_name = other.name;
        if other_name == f.name {
            continue;
        }
        let src_ty = scalar_of(other, w);
        if !listed.contains_key(&src_ty) || !conv.has(&src_ty, &ty) {
            continue;
        }
        let body = letters
            .iter()
            .map(|c| format!("        {}: {},\n", c, cast_expr(other, f, c)))
            .collect::<String>();
        let note = conv.note(&src_ty, &ty);
        src += &format!("    /// `{ty}({src_ty} v)` — the dump's `{note}` conversion, spelled as a method because Rust has no operator overloading.\n    #[inline]\n    pub fn from_{other_name}(v: {src_ty}) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n");
    }

    // Swizzle getters: every length 2..4 over this type's own letters — exactly the dump's set.
    for len in 2..=4 {
        for s in swizzles(letters, len) {
            let ret = scalar_of(f, len);
            let body = s
                .chars()
                .enumerate()
                .map(|(i, c)| format!("{}: self.{c}", LETTERS[i]))
                .collect::<Vec<_>>()
                .join(", ");
            src += &format!("    /// `get_{s}`.\n    #[inline]\n    pub const fn {s}(&self) -> {ret} {{ {ret} {{ {body} }} }}\n");
            names.insert(format!("get_{s}"));
        }
    }
    // Swizzle setters: only distinct letters, which is why the dump has `set_xy` but no `set_xx`.
    for len in 2..=w {
        for s in permutations(letters, len) {
            let arg_ty = scalar_of(f, len);
            let body = s
                .chars()
                .enumerate()
                .map(|(i, c)| format!("self.{c} = v.{};", LETTERS[i]))
                .collect::<Vec<_>>()
                .join(" ");
            src += &format!("    /// `set_{s}`.\n    #[inline]\n    pub fn set_{s}(&mut self, v: {arg_ty}) {{ {body} }}\n");
            names.insert(format!("set_{s}"));
        }
    }

    // `get_Item` / `set_Item`. Out of range panics, the Rust counterpart of the
    // `IndexOutOfRangeException` the original throws.
    let idx_body = letters
        .iter()
        .enumerate()
        .map(|(i, c)| format!("            {i} => self.{c},\n"))
        .collect::<String>();
    src += &format!("    /// `get_Item(int)`.\n    #[inline]\n    pub fn item(&self, index: u32) -> {} {{\n        match index {{\n{idx_body}            _ => panic!(\"{ty} index out of range\"),\n        }}\n    }}\n", f.scalar);
    names.insert("get_Item".to_string());
    let set_idx_body = letters
        .iter()
        .enumerate()
        .map(|(i, c)| format!("            {i} => self.{c} = value,\n"))
        .collect::<String>();
    src += &format!("    /// `set_Item(int, {})`.\n    #[inline]\n    pub fn set_item(&mut self, index: u32, value: {}) {{\n        match index {{\n{set_idx_body}            _ => panic!(\"{ty} index out of range\"),\n        }}\n    }}\n", f.scalar, f.scalar);
    names.insert("set_Item".to_string());
    src += &format!("}}\n\nimpl std::ops::Index<u32> for {ty} {{\n    type Output = {};\n    /// `this[int]`.\n    #[inline]\n    fn index(&self, i: u32) -> &Self::Output {{\n        match i {{\n", f.scalar);
    for (i, c) in letters.iter().enumerate() {
        src += &format!("            {i} => &self.{c},\n");
    }
    src +=
        &format!("            _ => panic!(\"{ty} index out of range\"),\n        }}\n    }}\n}}\n");

    // `ToString` becomes `Display`, Rust's formatting trait. The format string is the shipped
    // literal for this type; the synthesis below is what the library's own generator emits, and it
    // is only used when the ledger is absent.
    let suffix = if f.name == "float" { "f" } else { "" };
    let synth = {
        let parts: Vec<String> = (0..w).map(|i| format!("{{{i}}}{suffix}")).collect();
        format!("{ty}({})", parts.join(", "))
    };
    let fmt = fmts.get_or(&ty, w, &synth);
    let args: Vec<String> = letters.iter().map(|c| format!("self.{c}")).collect();
    src += &display_impl(&ty, &fmt, &args);

    // Component-wise binary operators, in all three forms the dump declares: (vector, vector),
    // (vector, scalar) and (scalar, vector).
    let bin = |name: &str, lhs_vec: bool, rhs: &str, op: &str, doc: &str| {
        let (lhs_ty, rhs_ty) = if lhs_vec {
            (ty.as_str(), rhs)
        } else {
            (rhs, ty.as_str())
        };
        let body = if lhs_vec {
            letters
                .iter()
                .map(|c| {
                    let r = if rhs == ty {
                        format!("rhs.{c}")
                    } else {
                        "rhs".to_string()
                    };
                    format!("        {c}: self.{c} {op} {r},\n")
                })
                .collect::<String>()
        } else {
            letters
                .iter()
                .map(|c| format!("        {c}: self {op} rhs.{c},\n"))
                .collect::<String>()
        };
        format!("\nimpl std::ops::{name}<{rhs_ty}> for {lhs_ty} {{\n    type Output = {ty};\n    /// `{doc}`.\n    #[inline]\n    fn {}(self, rhs: {rhs_ty}) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n", name.to_lowercase())
    };

    for (name, op, doc) in [
        ("Add", "+", "op_Addition"),
        ("Sub", "-", "op_Subtraction"),
        ("Mul", "*", "op_Multiply"),
        ("Div", "/", "op_Division"),
        ("Rem", "%", "op_Modulus"),
        ("BitAnd", "&", "op_BitwiseAnd"),
        ("BitOr", "|", "op_BitwiseOr"),
        ("BitXor", "^", "op_ExclusiveOr"),
    ] {
        if !has(doc) {
            continue;
        }
        src += &bin(name, true, &ty, op, doc);
        src += &bin(name, true, f.scalar, op, &format!("{doc} (vector, scalar)"));
        src += &bin(
            name,
            false,
            f.scalar,
            op,
            &format!("{doc} (scalar, vector)"),
        );
    }
    if has("op_LeftShift") {
        // The dump declares `op_LeftShift(int4 x, int n)`: the shift count is a scalar `int`.
        src += &bin("Shl", true, "i32", "<<", "op_LeftShift");
    }
    if has("op_RightShift") {
        src += &bin("Shr", true, "i32", ">>", "op_RightShift");
    }
    if has("op_UnaryNegation") {
        // C# negates an unsigned vector by wrapping; Rust has no `-u32`, so the port says so.
        let body = letters
            .iter()
            .map(|c| {
                if f.signed {
                    format!("        {c}: -self.{c},\n")
                } else {
                    format!("        {c}: self.{c}.wrapping_neg(),\n")
                }
            })
            .collect::<String>();
        src += &format!("\nimpl std::ops::Neg for {ty} {{\n    type Output = {ty};\n    /// `op_UnaryNegation`.\n    #[inline]\n    fn neg(self) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n");
    }
    if has("op_OnesComplement") || has("op_LogicalNot") {
        let body = letters
            .iter()
            .map(|c| format!("        {c}: !self.{c},\n"))
            .collect::<String>();
        let doc = if f.boolean {
            "op_LogicalNot"
        } else {
            "op_OnesComplement"
        };
        src += &format!("\nimpl std::ops::Not for {ty} {{\n    type Output = {ty};\n    /// `{doc}`.\n    #[inline]\n    fn not(self) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n");
    }

    // `op_Equality` and the comparisons return a component-wise `boolN` in the original, so they
    // are *not* `PartialEq`/`PartialOrd`; the port spells them as methods and says so in the name.
    // `PartialEq` (Rust's `==`) stands in for `Equals`, which returns a single `bool`.
    // One generic method covers the dump's three overloads, because a scalar operand is just the
    // splatted vector (`From<scalar> for {ty}` above).
    let bool_ty = format!("bool{w}");
    if listed.contains_key(&bool_ty) {
        src += &format!("impl {ty} {{\n");
        for (method, op, doc) in [
            ("is_equal", "==", "op_Equality"),
            ("is_not_equal", "!=", "op_Inequality"),
            ("less_than", "<", "op_LessThan"),
            ("less_than_or_equal", "<=", "op_LessThanOrEqual"),
            ("greater_than", ">", "op_GreaterThan"),
            ("greater_than_or_equal", ">=", "op_GreaterThanOrEqual"),
        ] {
            if !has(doc) {
                continue;
            }
            let body = letters
                .iter()
                .map(|c| format!("        {c}: self.{c} {op} r.{c},\n"))
                .collect::<String>();
            src += &format!("    /// `{doc}` — component-wise, so the result is `{bool_ty}`, not `bool`; that is why this is a method and not `PartialOrd`.\n    #[inline]\n    pub fn {method}<R: Into<{ty}>>(self, r: R) -> {bool_ty} {{\n        let r: {ty} = r.into();\n        {bool_ty} {{\n{body}        }}\n    }}\n");
        }
        src += "}\n";
    }

    // `math.dot` / `math.cross` / `math.length` are statics on the original's `math` class; the
    // dump gives the vector types no instance versions. They live here as instance methods so the
    // generic `math::dot` &c. can dispatch on them — recorded in docs/PORT.md.
    if !f.boolean && !f.binary16 {
        // The original writes the dot as one left-to-right sum, so the first product is never added
        // to a zero accumulator: `0.0 + -0.0` is `+0.0`, which would flip the sign of a negative
        // first product.
        let dot_body = letters
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if i == 0 {
                    format!("self.{c} * y.{c}")
                } else {
                    format!(" + self.{c} * y.{c}")
                }
            })
            .collect::<String>();
        src += &format!("\nimpl {ty} {{\n    /// `math.dot({ty}, {ty})`, as an instance method so `math::dot` can dispatch generically.\n    #[inline]\n    pub fn dot(self, y: {ty}) -> {} {{\n        {dot_body}\n    }}\n", f.scalar);
        if w == 3 {
            src += &format!("    /// `math.cross({ty}, {ty})`, as an instance method for generic dispatch.\n    #[inline]\n    pub fn cross(self, y: {ty}) -> {ty} {{\n        {ty} {{\n            x: self.y * y.z - self.z * y.y,\n            y: self.z * y.x - self.x * y.z,\n            z: self.x * y.y - self.y * y.x,\n        }}\n    }}\n");
        }
        if w == 2 {
            src += &format!("    /// `math.cross({ty}, {ty})` — the 2D form returns the scalar z.\n    #[inline]\n    pub fn cross(self, y: {ty}) -> {} {{\n        self.x * y.y - self.y * y.x\n    }}\n", f.scalar);
        }
        if f.floating {
            src += &format!("    /// `math.lengthsq({ty})`.\n    #[inline]\n    pub fn lengthsq(self) -> {} {{\n        self.dot(self)\n    }}\n    /// `math.length({ty})`.\n    #[inline]\n    pub fn length(self) -> {} {{\n        self.lengthsq().sqrt()\n    }}\n    /// `math.distance({ty}, {ty})`.\n    #[inline]\n    pub fn distance(a: {ty}, b: {ty}) -> {} {{\n        (b - a).length()\n    }}\n}}\n", f.scalar, f.scalar, f.scalar);
        } else {
            src += "}\n";
        }
    }

    out.write_all(src.as_bytes()).unwrap();
    names
}

/// Generate one matrix type: `cols` columns of `{family}{rows}` vectors, at the dump's offsets.
///
/// `op_Multiply` on a matrix is component-wise in the original — the decompiled
/// `float3x2::op_Multiply(float3x2, float3x2)` at RVA 0x9333a30 multiplies the six components
/// pairwise, never a row-by-column sum; the real matrix product is `math.mul`.
fn gen_matrix(
    out: &mut Vec<u8>,
    f: &Family,
    rows: usize,
    cols: usize,
    mats: &BTreeMap<String, (usize, usize)>,
    members: Option<&BTreeSet<String>>,
    ledgers: (&ConversionLedger, &FormatLedger),
) -> BTreeSet<String> {
    let (conv, fmts) = ledgers;
    let ty = format!("{}{}x{}", f.name, rows, cols);
    let col_ty = format!("{}{}", f.name, rows);
    let bool_ty = format!("bool{}x{}", rows, cols);
    let mut src = String::new();
    let mut names = BTreeSet::new();
    let has = |m: &str| members.map(|s| s.contains(m)).unwrap_or(true);

    let fields = (0..cols)
        .map(|j| format!("pub c{j}: {col_ty},"))
        .collect::<Vec<_>>()
        .join(" ");
    let derives = if f.floating {
        "Clone, Copy, PartialEq, Debug, Default"
    } else {
        "Clone, Copy, PartialEq, Eq, Hash, Debug, Default"
    };
    src += &format!("\n/// `Unity.Mathematics.{ty}` — {cols} columns of `{col_ty}`, in the dump's order.\n#[repr(C)]\n#[derive({derives})]\npub struct {ty} {{ {fields} }}\n");
    if f.floating {
        let hash_body = (0..cols)
            .map(|j| format!("        self.c{j}.hash(state);\n"))
            .collect::<String>();
        src += &format!("\nimpl std::hash::Hash for {ty} {{\n    /// `GetHashCode()`: the columns' bit patterns.\n    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{\n{hash_body}    }}\n}}\n");
    }

    let zero_cols = (0..cols)
        .map(|j| format!("c{j}: {col_ty}::zero"))
        .collect::<Vec<_>>()
        .join(", ");
    let new_args = (0..cols)
        .map(|j| format!("c{j}: {col_ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let new_init = (0..cols)
        .map(|j| format!("c{j}: c{j}"))
        .collect::<Vec<_>>()
        .join(", ");
    let splat_cols = (0..cols)
        .map(|j| format!("c{j}: {col_ty}::splat(v)"))
        .collect::<Vec<_>>()
        .join(", ");
    src += &format!(
        "impl {ty} {{\n    /// `{ty}.zero`.\n    pub const zero: {ty} = {ty} {{ {zero_cols} }};\n"
    );
    src += &format!("    /// `{ty}({new_args})`.\n    #[inline]\n    pub const fn new({new_args}) -> {ty} {{ {ty} {{ {new_init} }} }}\n");
    src += &format!("    /// `{ty}({} v)` — every component set to `v`.\n    #[inline]\n    pub const fn splat(v: {}) -> {ty} {{ {ty} {{ {splat_cols} }} }}\n", f.scalar, f.scalar);
    for other in FAMILIES {
        let other_name = other.name;
        if other_name == f.name {
            continue;
        }
        let src_ty = format!("{}{}x{}", other.name, rows, cols);
        if !mats.contains_key(&src_ty) || !conv.has(&src_ty, &ty) {
            continue;
        }
        let body = (0..cols)
            .map(|j| format!("        c{j}: v.c{j}.into(),\n"))
            .collect::<String>();
        let note = conv.note(&src_ty, &ty);
        src += &format!("    /// `{ty}({src_ty} v)` — the dump's `{note}` conversion.\n    #[inline]\n    pub fn from_{other_name}(v: {src_ty}) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n");
    }

    // `get_Item(int)` returns a column; the dump declares no setter.
    let idx_body = (0..cols)
        .map(|j| format!("            {j} => self.c{j},\n"))
        .collect::<String>();
    src += &format!("    /// `get_Item(int)` — the column at `index`.\n    #[inline]\n    pub fn item(&self, index: u32) -> {col_ty} {{\n        match index {{\n{idx_body}            _ => panic!(\"{ty} index out of range\"),\n        }}\n    }}\n");
    names.insert("get_Item".to_string());
    src += "}\n";

    src += &format!("\nimpl std::ops::Index<u32> for {ty} {{\n    type Output = {col_ty};\n    /// `this[int]` — the column at `index`.\n    #[inline]\n    fn index(&self, i: u32) -> &Self::Output {{\n        match i {{\n");
    for j in 0..cols {
        src += &format!("            {j} => &self.c{j},\n");
    }
    src +=
        &format!("            _ => panic!(\"{ty} index out of range\"),\n        }}\n    }}\n}}\n");

    // `ToString` prints the matrix **row by row**: `float3x4` (three rows of four) groups after
    // every fourth element, `float4x3` after every third. The fields are columns, so the argument
    // order walks rows outside, columns inside.
    let suffix = if f.name == "float" { "f" } else { "" };
    let synth = {
        let groups: Vec<String> = (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| format!("{{{}}}{suffix}", r * cols + c))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect();
        format!("{ty}({})", groups.join(",  "))
    };
    let fmt = fmts.get_or(&ty, rows * cols, &synth);
    let args: Vec<String> = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| format!("self.c{c}.{}", LETTERS[r])))
        .collect();
    src += &display_impl(&ty, &fmt, &args);

    // Column-wise operators, in the three forms the dump declares.
    let bin = |name: &str, lhs_mat: bool, rhs: &str, op: &str, doc: &str| {
        let (lhs_ty, rhs_ty) = if lhs_mat {
            (ty.as_str(), rhs)
        } else {
            (rhs, ty.as_str())
        };
        let body = (0..cols)
            .map(|j| {
                if !lhs_mat {
                    // `self` is the scalar, `rhs` the matrix.
                    format!("        c{j}: self {op} rhs.c{j},\n")
                } else if rhs == ty {
                    format!("        c{j}: self.c{j} {op} rhs.c{j},\n")
                } else {
                    format!("        c{j}: self.c{j} {op} rhs,\n")
                }
            })
            .collect::<String>();
        format!("\nimpl std::ops::{name}<{rhs_ty}> for {lhs_ty} {{\n    type Output = {ty};\n    /// `{doc}`.\n    #[inline]\n    fn {}(self, rhs: {rhs_ty}) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n", name.to_lowercase())
    };

    for (name, op, doc) in [
        ("Add", "+", "op_Addition"),
        ("Sub", "-", "op_Subtraction"),
        ("Mul", "*", "op_Multiply"),
        ("Div", "/", "op_Division"),
        ("Rem", "%", "op_Modulus"),
        ("BitAnd", "&", "op_BitwiseAnd"),
        ("BitOr", "|", "op_BitwiseOr"),
        ("BitXor", "^", "op_ExclusiveOr"),
    ] {
        if !has(doc) {
            continue;
        }
        src += &bin(
            name,
            true,
            &ty,
            op,
            &format!("{doc} (component-wise, as the decompiled body shows)"),
        );
        src += &bin(name, true, f.scalar, op, &format!("{doc} (matrix, scalar)"));
        src += &bin(
            name,
            false,
            f.scalar,
            op,
            &format!("{doc} (scalar, matrix)"),
        );
    }
    if has("op_LeftShift") {
        src += &bin("Shl", true, "i32", "<<", "op_LeftShift");
    }
    if has("op_RightShift") {
        src += &bin("Shr", true, "i32", ">>", "op_RightShift");
    }
    if has("op_UnaryNegation") {
        let body = (0..cols)
            .map(|j| format!("        c{j}: -self.c{j},\n"))
            .collect::<String>();
        src += &format!("\nimpl std::ops::Neg for {ty} {{\n    type Output = {ty};\n    /// `op_UnaryNegation`.\n    #[inline]\n    fn neg(self) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n");
    }
    if has("op_OnesComplement") || has("op_LogicalNot") {
        let body = (0..cols)
            .map(|j| format!("        c{j}: !self.c{j},\n"))
            .collect::<String>();
        let doc = if f.boolean {
            "op_LogicalNot"
        } else {
            "op_OnesComplement"
        };
        src += &format!("\nimpl std::ops::Not for {ty} {{\n    type Output = {ty};\n    /// `{doc}`.\n    #[inline]\n    fn not(self) -> {ty} {{\n        {ty} {{\n{body}        }}\n    }}\n}}\n");
    }

    // Component-wise comparisons return the matching `boolMxN`, so they are methods, not traits.
    if mats.contains_key(&bool_ty) {
        src += &format!("impl {ty} {{\n");
        for (method, doc) in [
            ("is_equal", "op_Equality"),
            ("is_not_equal", "op_Inequality"),
            ("less_than", "op_LessThan"),
            ("less_than_or_equal", "op_LessThanOrEqual"),
            ("greater_than", "op_GreaterThan"),
            ("greater_than_or_equal", "op_GreaterThanOrEqual"),
        ] {
            if !has(doc) {
                continue;
            }
            // The columns are vectors, and their own comparisons are methods returning `boolN`, so
            // the matrix version is the same method applied column by column.
            let body = (0..cols)
                .map(|j| format!("        c{j}: self.c{j}.{method}(r.c{j}),\n"))
                .collect::<String>();
            src += &format!("    /// `{doc}` — column-wise, so the result is `{bool_ty}`, not `bool`.\n    #[inline]\n    pub fn {method}<R: Into<{ty}>>(self, r: R) -> {bool_ty} {{\n        let r: {ty} = r.into();\n        {bool_ty} {{\n{body}        }}\n    }}\n");
        }
        src += "}\n";
    }

    out.write_all(src.as_bytes()).unwrap();
    names
}
