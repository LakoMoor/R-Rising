//! Data-based runtime scenario over all scenes: build each scene `World`, run the port's schedule
//! (`TransformSystem`, parents before children, computed from authoring data) for 1 and 3 fixed
//! frames, and check determinism, change-version skipping and agreement with the stored
//! `LocalToWorld`.
//!
//! This is a scenario on baked scene data. It is NOT execution of the original runtime: no
//! original system code runs, and the stored `LocalToWorld` is only a reference to compare with.
//! Requires the pinned original installation and RRISING_LAYOUT_REPORT; original data is read and
//! summarized, never written out.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{
        TypeLayouts, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, ROTATION_HASH, TRANSLATION_HASH,
    },
    world_adapter::{build_scene_world, PARENT_HASH},
};
use rrising_ecs::{
    transform::{TransformStats, TransformSystem, TransformTypes},
    EntityQuery, Schedule,
};
use std::{path::PathBuf, time::Instant};

const SCALE_HASH: u64 = 0xadc0_56c7_729e_d01e;
const POST_HASH: u64 = 0x0233_3773_b807_e90d;
const STEP_NS: u64 = 16_666_667;

#[derive(Default, Clone, Copy)]
struct Stat {
    n: usize,
    exact: usize,
    exact_modzero: usize,
    within_1e5: usize,
    over_1e3: usize,
    max_rel: f32,
}

impl Stat {
    fn add(&mut self, got: &[f32], want: &[f32]) {
        self.n += 1;
        let (mut rel, mut exact, mut mz) = (0f32, true, true);
        for (g, w) in got.iter().zip(want) {
            exact &= g.to_bits() == w.to_bits();
            mz &= (g + 0.0).to_bits() == (w + 0.0).to_bits();
            let d = (g - w).abs();
            let d = if d.is_nan() { f32::INFINITY } else { d };
            rel = rel.max(d / w.abs().max(1.0));
        }
        self.exact += usize::from(exact);
        self.exact_modzero += usize::from(mz);
        self.within_1e5 += usize::from(rel <= 1e-5);
        self.over_1e3 += usize::from(rel > 1e-3);
        self.max_rel = self.max_rel.max(rel);
    }
    fn merge(&mut self, o: &Stat) {
        self.n += o.n;
        self.exact += o.exact;
        self.exact_modzero += o.exact_modzero;
        self.within_1e5 += o.within_1e5;
        self.over_1e3 += o.over_1e3;
        self.max_rel = self.max_rel.max(o.max_rel);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    stored_ltw: u64,
    state_after_1: u64,
    ltw_after_1: u64,
    state_after_3: u64,
    ltw_after_3: u64,
    versions_after_1: Vec<u32>,
    versions_after_3: Vec<u32>,
}

struct SceneRun {
    fp: Fingerprint,
    stats_f1: TransformStats,
    written_f23: usize,
    stat: Stat,
    untouched_violations: usize,
    t_first: std::time::Duration,
    t_steady: std::time::Duration,
}

fn le_f32(b: &[u8]) -> [f32; 16] {
    let mut r = [0f32; 16];
    for (v, c) in r.iter_mut().zip(b.as_chunks::<4>().0.iter()) {
        *v = f32::from_le_bytes(*c);
    }
    r
}

fn run_scene(layouts: &TypeLayouts, scene: &SceneFile) -> Option<SceneRun> {
    let mut sw = build_scene_world(layouts, scene).expect("world");
    let ty = |h: u64| sw.type_of_hash.get(&h).copied();
    let ltw = ty(LOCAL_TO_WORLD_HASH)?;
    let tt = TransformTypes {
        local_to_world: ltw,
        local_transform: ty(LOCAL_TRANSFORM_HASH),
        translation: ty(TRANSLATION_HASH),
        rotation: ty(ROTATION_HASH),
        scale: ty(SCALE_HASH),
        post_transform: ty(POST_HASH),
        parent: ty(PARENT_HASH),
    };
    let w = &mut sw.world;
    // snapshot of the stored LocalToWorld per entity index, before any system runs
    let q = EntityQuery {
        all: vec![ltw],
        include_disabled: true,
        include_prefab: true,
        ..Default::default()
    };
    let mut stored: Vec<Option<[f32; 16]>> = vec![None; w.store.capacity() as usize + 1];
    let chunks = w.store.query_chunks(&q);
    for &c in &chunks {
        let ents = w.store.chunk_entities(c).expect("entities");
        let b = w.store.chunk_component_bytes(c, ltw).expect("ltw");
        for (e, m) in ents.iter().zip(b.as_chunks::<64>().0) {
            stored[e.index as usize] = Some(le_f32(m));
        }
    }
    let stored_ltw = w.component_hash(ltw);

    let mut sched = Schedule::new(STEP_NS).expect("schedule");
    sched.add_group("Simulation").expect("group");
    sched
        .add_system("Simulation", Box::new(TransformSystem::new(tt)))
        .expect("system");
    let stats_of = |s: &Schedule| {
        s.system::<TransformSystem>("TransformSystem")
            .expect("sys")
            .stats()
    };

    let t = Instant::now();
    sched.run_frame(w).expect("frame 1");
    let t_first = t.elapsed();
    let stats_f1 = stats_of(&sched);
    let (state_after_1, ltw_after_1) = (w.state_hash(), w.component_hash(ltw));
    let versions_after_1 = w.component_versions(ltw);

    let t = Instant::now();
    sched.run_frame(w).expect("frame 2");
    let t_steady = t.elapsed();
    let mut written_f23 = stats_of(&sched).entities_written();
    sched.run_frame(w).expect("frame 3");
    written_f23 += stats_of(&sched).entities_written();
    let fp = Fingerprint {
        stored_ltw,
        state_after_1,
        ltw_after_1,
        state_after_3: w.state_hash(),
        ltw_after_3: w.component_hash(ltw),
        versions_after_1,
        versions_after_3: w.component_versions(ltw),
    };

    // produced (after frame 1; frames 2-3 changed nothing) vs stored; entities the system does not
    // compute (no authoring source) must be byte-identical to the snapshot.
    let mut stat = Stat::default();
    let mut untouched_violations = 0;
    let sources = [tt.local_transform, tt.translation, tt.rotation];
    for &c in &chunks {
        let has_source = sources
            .iter()
            .flatten()
            .any(|&t| w.store.chunk_has_component(c, t));
        let ents = w.store.chunk_entities(c).expect("entities");
        let b = w.store.chunk_component_bytes(c, ltw).expect("ltw");
        for (e, m) in ents.iter().zip(b.as_chunks::<64>().0) {
            let (got, want) = (le_f32(m), stored[e.index as usize].expect("stored"));
            if has_source {
                stat.add(&got, &want);
            } else {
                untouched_violations +=
                    usize::from(got.map(f32::to_bits) != want.map(f32::to_bits));
            }
        }
    }
    Some(SceneRun {
        fp,
        stats_f1,
        written_f23,
        stat,
        untouched_violations,
        t_first,
        t_steady,
    })
}

#[test]
#[ignore = "requires the pinned original installation and RRISING_LAYOUT_REPORT"]
fn all_scenes_run_the_transform_schedule_deterministically() {
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

    let mut total = Stat::default();
    let (mut scenes, mut skipped_scenes) = (0usize, 0usize);
    let (mut roots_written, mut children_written, mut unresolved) = (0usize, 0usize, 0usize);
    let (mut violations, mut steady_writes, mut changed_hash_scenes) = (0usize, 0usize, 0usize);
    let (mut first, mut steady) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let mut worst = (std::time::Duration::ZERO, String::new());
    for path in &paths {
        let scene = SceneFile::open(path).expect("scene");
        let Some(a) = run_scene(&layouts, &scene) else {
            skipped_scenes += 1;
            continue;
        };
        // an independent second build of the same scene must reproduce every fingerprint
        let b = run_scene(&layouts, &scene).expect("second build");
        assert_eq!(
            a.fp,
            b.fp,
            "{}: not deterministic across builds",
            path.display()
        );
        assert_eq!(a.stats_f1, b.stats_f1);
        // steady state: frames 2 and 3 rewrite nothing and bump no LocalToWorld change version
        assert_eq!(
            a.written_f23,
            0,
            "{}: unchanged entities rewritten",
            path.display()
        );
        assert_eq!(a.fp.state_after_1, a.fp.state_after_3, "{}", path.display());
        assert_eq!(a.fp.ltw_after_1, a.fp.ltw_after_3);
        assert_eq!(a.fp.versions_after_1, a.fp.versions_after_3);
        scenes += 1;
        total.merge(&a.stat);
        roots_written += a.stats_f1.root_entities_written;
        children_written += a.stats_f1.children_written;
        unresolved += a.stats_f1.children_unresolved;
        violations += a.untouched_violations;
        steady_writes += a.written_f23;
        changed_hash_scenes += usize::from(a.fp.stored_ltw != a.fp.ltw_after_1);
        first += a.t_first;
        steady += a.t_steady;
        if a.t_first > worst.0 {
            worst = (
                a.t_first,
                path.file_name().unwrap().to_string_lossy().into_owned(),
            );
        }
    }
    println!(
        "scenes run {scenes} (no LocalToWorld: {skipped_scenes}); frame 1 wrote {roots_written} roots + {children_written} children ({unresolved} unresolved); frames 2-3 wrote {steady_writes}; scenes whose LocalToWorld hash changed vs stored: {changed_hash_scenes}"
    );
    println!(
        "produced vs stored over {} entities with an authoring source: bit-identical {} (+-0 equal {}), <=1e-5 {}, worse {}, >1e-3 {}, max_rel {:e}; untouched-entity violations {violations}",
        total.n, total.exact, total.exact_modzero, total.within_1e5, total.n - total.within_1e5, total.over_1e3, total.max_rel
    );
    println!(
        "frame time per scene world (this profile): frame 1 mean {:?} total {first:?} worst {:?} ({}); frame 2 (nothing to do) mean {:?} total {steady:?}",
        first / scenes as u32, worst.0, worst.1, steady / scenes as u32
    );
    assert_eq!(
        violations, 0,
        "entities without an authoring source were rewritten"
    );
    assert_eq!(scenes + skipped_scenes, 1183);
    assert!(total.n > 100_000, "compared {}", total.n);
    assert!(total.exact_modzero * 100 >= total.n * 55);
    assert!(total.within_1e5 * 100 >= total.n * 98);
}
