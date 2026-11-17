//! Original-data behavioral comparison: compute `LocalToWorld` from the authoring transforms with
//! `rrising_ecs::transform` and compare with the `LocalToWorld` the original's TransformSystem stored
//! in each scene file. Requires the pinned original installation and RRISING_LAYOUT_REPORT; not a
//! portable CI oracle. Original data is read and summarized, never written out.
//!
//! Source rule (measured, see `docs/COMPATIBILITY.md`): `Translation`/`Rotation` drive the matrix
//! when present (their `LocalTransform` twin is stale in ~0.3% of entities), with the `Scale` component, else
//! `LocalTransform.Scale`, as uniform scale; otherwise `LocalTransform` alone.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{
        TypeLayouts, ENTITY_HASH, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, ROTATION_HASH,
        TRANSLATION_HASH,
    },
};
use rrising_ecs::transform::{
    child_to_world, local_transform_matrix, post_transform, trs_matrix, LocalTransform, Mat,
};
use std::{collections::BTreeMap, path::PathBuf};

const SCALE_HASH: u64 = 0xadc0_56c7_729e_d01e;
const PARENT_HASH: u64 = 0x6a6b_52ec_c95a_65e0;
const POST_HASH: u64 = 0x0233_3773_b807_e90d;
const STATIC_HASH: u64 = 0x495a_dd5c_e819_8ca0;
const STATIC_CHILD_HASH: u64 = 0xc86a_4979_705d_472e;

fn f32s<const N: usize>(b: &[u8]) -> [f32; N] {
    let mut r = [0f32; N];
    for (i, v) in r.iter_mut().enumerate() {
        *v = f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    }
    r
}

fn id(b: &[u8]) -> (i32, i32) {
    (
        i32::from_le_bytes(b[0..4].try_into().unwrap()),
        i32::from_le_bytes(b[4..8].try_into().unwrap()),
    )
}

#[derive(Clone, Default)]
struct Rec {
    key: String,
    lt: Option<LocalTransform>,
    t: Option<[f32; 3]>,
    r: Option<[f32; 4]>,
    s: Option<f32>,
    post: Option<Mat>,
    parent: Option<(i32, i32)>,
    stored: Option<Mat>,
}

#[derive(Default, Clone)]
struct Stat {
    n: usize,
    exact: usize,
    exact_modzero: usize,
    within_1e5: usize,
    over_1e3: usize,
    max_abs: f32,
    max_rel: f32,
}

impl Stat {
    fn add(&mut self, got: &Mat, want: &Mat) {
        self.n += 1;
        let (mut abs, mut rel, mut exact, mut exact_mz) = (0f32, 0f32, true, true);
        for (g, w) in got.iter().zip(want) {
            exact &= g.to_bits() == w.to_bits();
            exact_mz &= (g + 0.0).to_bits() == (w + 0.0).to_bits();
            let d = (g - w).abs();
            let d = if d.is_nan() { f32::INFINITY } else { d };
            abs = abs.max(d);
            // Relative to the element magnitude, floored at 1 so near-zero lanes do not dominate.
            rel = rel.max(d / w.abs().max(1.0));
        }
        self.exact += usize::from(exact);
        self.exact_modzero += usize::from(exact_mz);
        self.within_1e5 += usize::from(rel <= 1e-5);
        self.over_1e3 += usize::from(rel > 1e-3);
        self.max_abs = self.max_abs.max(abs);
        self.max_rel = self.max_rel.max(rel);
    }
    fn merge(&mut self, o: &Stat) {
        self.n += o.n;
        self.exact += o.exact;
        self.exact_modzero += o.exact_modzero;
        self.within_1e5 += o.within_1e5;
        self.over_1e3 += o.over_1e3;
        self.max_abs = self.max_abs.max(o.max_abs);
        self.max_rel = self.max_rel.max(o.max_rel);
    }
}

/// The entity's local matrix, or `None` when it has no authoring transform.
fn local(r: &Rec) -> Option<Mat> {
    let m = if r.t.is_some() || r.r.is_some() {
        let scale = r.s.or(r.lt.map(|l| l.scale)).unwrap_or(1.0);
        trs_matrix(
            r.t.unwrap_or([0.0; 3]),
            r.r.unwrap_or([0.0, 0.0, 0.0, 1.0]),
            scale,
        )
    } else if let Some(lt) = &r.lt {
        local_transform_matrix(lt)
    } else {
        return None;
    };
    Some(match &r.post {
        Some(p) => post_transform(&m, p),
        None => m,
    })
}

fn chain_world(
    i: usize,
    recs: &[Rec],
    by_id: &BTreeMap<(i32, i32), usize>,
    memo: &mut [Option<Option<Mat>>],
    depth: usize,
) -> Option<Mat> {
    if let Some(m) = memo[i] {
        return m;
    }
    let r = &recs[i];
    let res = (|| {
        let l = local(r)?;
        match r.parent {
            None => Some(l),
            Some(p) => {
                let pi = *by_id.get(&p)?;
                if depth > 64 {
                    return None;
                }
                let pw = chain_world(pi, recs, by_id, memo, depth + 1).or(recs[pi].stored)?;
                Some(child_to_world(&pw, &l))
            }
        }
    })();
    memo[i] = Some(res);
    res
}

#[test]
#[ignore = "requires the pinned original installation and RRISING_LAYOUT_REPORT"]
fn computed_local_to_world_matches_stored() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("set RRISING_LAYOUT_REPORT");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("set VRISING_DATA"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).expect("report")).expect("layouts");
    let mut paths: Vec<_> = std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))
        .expect("scenes")
        .map(|p| p.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "entities"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 1183);

    let (mut entities, mut with_stored) = (0usize, 0usize);
    let mut no_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut parent_outside = 0usize;
    // `step` uses the parent's stored LocalToWorld (isolates the child's own step); `chain`
    // recomputes parents from their own authoring data so rounding compounds up the hierarchy.
    let mut step: BTreeMap<String, Stat> = BTreeMap::new();
    let mut chain: BTreeMap<String, Stat> = BTreeMap::new();
    let mut lt_stale = (0usize, 0usize); // (entities with LT+T+R, of which position or rotation differ)

    for path in &paths {
        let scene = SceneFile::open(path).expect("scene");
        let resolved = layouts.resolve_scene(&scene).expect("resolve");
        let mut recs: Vec<Rec> = Vec::new();
        let mut by_id: BTreeMap<(i32, i32), usize> = BTreeMap::new();
        for (ci, chunk) in resolved.chunks.iter().enumerate() {
            let Some(arch) = resolved.archetypes[chunk.header.archetype_index as usize].as_ref()
            else {
                continue;
            };
            for ei in 0..chunk.header.count as usize {
                let get = |h: u64| {
                    resolved
                        .component_bytes(ci, ei, h)
                        .expect("component bytes")
                };
                let mut r = Rec {
                    lt: get(LOCAL_TRANSFORM_HASH).map(|b| {
                        let v = f32s::<8>(b);
                        LocalTransform {
                            position: [v[0], v[1], v[2]],
                            scale: v[3],
                            rotation: [v[4], v[5], v[6], v[7]],
                        }
                    }),
                    t: get(TRANSLATION_HASH).map(f32s::<3>),
                    r: get(ROTATION_HASH).map(f32s::<4>),
                    s: get(SCALE_HASH).map(|b| f32s::<1>(b)[0]),
                    post: get(POST_HASH).map(f32s::<16>),
                    parent: get(PARENT_HASH).map(id),
                    stored: get(LOCAL_TO_WORLD_HASH).map(f32s::<16>),
                    ..Default::default()
                };
                let scale = r.s.or(r.lt.map(|l| l.scale)).unwrap_or(1.0);
                let mut k: Vec<&str> = Vec::new();
                for (present, name) in [
                    (r.lt.is_some(), "LocalTransform"),
                    (r.t.is_some(), "Translation"),
                    (r.r.is_some(), "Rotation"),
                    (r.s.is_some(), "Scale"),
                    (r.post.is_some(), "PostTransformMatrix"),
                    (r.parent.is_some(), "Parent"),
                    (arch.hashes.contains(&STATIC_HASH), "Static"),
                    (arch.hashes.contains(&STATIC_CHILD_HASH), "StaticChild"),
                    (scale != 1.0, "scale!=1"),
                ] {
                    if present {
                        k.push(name);
                    }
                }
                r.key = k.join("+");
                if let (Some(lt), Some(t), Some(q)) = (r.lt, r.t, r.r) {
                    lt_stale.0 += 1;
                    lt_stale.1 += usize::from(lt.position != t || lt.rotation != q);
                }
                if let Some(b) = get(ENTITY_HASH) {
                    by_id.insert(id(b), recs.len());
                }
                recs.push(r);
            }
        }
        entities += recs.len();
        let mut memo = vec![None; recs.len()];
        for i in 0..recs.len() {
            let r = &recs[i];
            let Some(stored) = r.stored else { continue };
            with_stored += 1;
            let Some(l) = local(r) else {
                *no_source.entry(r.key.clone()).or_default() += 1;
                continue;
            };
            let step_world = match r.parent {
                None => Some(l),
                Some(p) => match by_id.get(&p).and_then(|&pi| recs[pi].stored) {
                    Some(ps) => Some(child_to_world(&ps, &l)),
                    None => {
                        parent_outside += 1;
                        None
                    }
                },
            };
            if let Some(w) = step_world {
                step.entry(r.key.clone()).or_default().add(&w, &stored);
            }
            if let Some(w) = chain_world(i, &recs, &by_id, &mut memo, 0) {
                chain.entry(r.key.clone()).or_default().add(&w, &stored);
            }
        }
    }

    let total = |m: &BTreeMap<String, Stat>| {
        let mut t = Stat::default();
        m.values().for_each(|s| t.merge(s));
        t
    };
    let (ts, tc) = (total(&step), total(&chain));
    println!("entities={entities} with_stored_ltw={with_stored} parent_outside_or_unstored={parent_outside}");
    println!(
        "LT+Translation+Rotation entities {} whose LT position/rotation differ from T/R: {}",
        lt_stale.0, lt_stale.1
    );
    println!("stored LocalToWorld with no authoring source: {no_source:?}");
    for (name, m, t) in [
        ("step (stored parent)", &step, &ts),
        ("chain (recomputed parents)", &chain, &tc),
    ] {
        println!("== {name}: compared {} exact {} exact_modzero {} within1e-5 {} >1e-3 {} max_abs {:e} max_rel {:e}",
            t.n, t.exact, t.exact_modzero, t.within_1e5, t.over_1e3, t.max_abs, t.max_rel);
        for (k, s) in m {
            println!(
                "  {k:70} n={:7} exact={:7} within1e-5={:7} >1e-3={:5} max_abs={:e} max_rel={:e}",
                s.n, s.exact, s.within_1e5, s.over_1e3, s.max_abs, s.max_rel
            );
        }
    }
    assert_eq!(entities, 1_385_671);
    // Regression floors on the pinned install (measured values are in docs/COMPATIBILITY.md).
    assert_eq!(ts.n, 112_177);
    assert!(ts.n > 100_000, "compared {}", ts.n);
    assert!(ts.exact_modzero * 100 >= ts.n * 60);
    assert!(ts.within_1e5 * 100 >= ts.n * 98);
}
