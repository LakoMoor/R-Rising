//! Schedule ordering, fixed-step driver and the transform system on a synthetic world (not
//! original data; the original-data scenario lives in `rrising-content/tests/scene_scenario.rs`).
use std::any::Any;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use rrising_ecs::transform::{TransformSystem, TransformTypes};
use rrising_ecs::{
    ArchetypeInput, ChunkInput, Entity, EntityIds, FrameContext, Schedule, SpecialTypes, System,
    TypeCategory, TypeIndex, TypeManager, TypeSpec, World, WorldInput,
};

struct Log(&'static str, Vec<&'static str>, Arc<Mutex<Vec<String>>>);

impl System for Log {
    fn name(&self) -> &str {
        self.0
    }
    fn update_after(&self) -> Vec<String> {
        self.1.iter().map(|s| (*s).to_owned()).collect()
    }
    fn update(&mut self, _: &mut World, ctx: &FrameContext) -> Result<()> {
        self.2
            .lock()
            .unwrap()
            .push(format!("{}@{}", self.0, ctx.frame));
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn empty_world() -> World {
    let mut tm = TypeManager::default();
    tm.register(TypeSpec::tag("<reserved null>")).unwrap();
    let mut es = TypeSpec::component_data("Entity", 8, 8);
    es.category = TypeCategory::EntityData;
    let entity = tm.register(es).unwrap();
    World::build(tm, SpecialTypes::new(entity), 4, &WorldInput::default()).unwrap()
}

#[test]
fn order_is_stable_topological_and_groups_run_in_creation_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut s = Schedule::new(10).unwrap();
    s.add_group("Sim").unwrap();
    s.add_group("Late").unwrap();
    let mk = |n, after: Vec<&'static str>| Box::new(Log(n, after, log.clone()));
    s.add_system("Sim", mk("c", vec!["a"])).unwrap();
    s.add_system("Sim", mk("a", vec![])).unwrap();
    s.add_system("Sim", mk("b", vec![])).unwrap();
    s.add_system("Late", mk("z", vec![])).unwrap();
    // registration order c, a, b; c needs a. Lowest registration index among ready systems
    // goes first: a (c not ready), then c (index 0 < b), then b
    assert_eq!(
        s.execution_order().unwrap(),
        ["Sim/a", "Sim/c", "Sim/b", "Late/z"]
    );
    let mut w = empty_world();
    s.run_frame(&mut w).unwrap();
    s.run_frame(&mut w).unwrap();
    assert_eq!(
        log.lock().unwrap().join(","),
        "a@0,c@0,b@0,z@0,a@1,c@1,b@1,z@1"
    );
    assert!(s.add_system("Sim", mk("a", vec![])).is_err());
    s.add_system("Sim", mk("d", vec!["nope"])).unwrap();
    assert!(s.execution_order().is_err());
}

#[test]
fn cycles_are_rejected() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut s = Schedule::new(10).unwrap();
    s.add_group("G").unwrap();
    s.add_system("G", Box::new(Log("a", vec!["b"], log.clone())))
        .unwrap();
    s.add_system("G", Box::new(Log("b", vec!["a"], log)))
        .unwrap();
    assert!(s.execution_order().is_err());
}

#[test]
fn fixed_step_accumulates_and_caps() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut s = Schedule::new(10).unwrap();
    s.add_group("G").unwrap();
    s.add_system("G", Box::new(Log("a", vec![], log.clone())))
        .unwrap();
    let mut w = empty_world();
    assert_eq!(s.advance(&mut w, 25, 8).unwrap(), 2); // 5 ns left over
    assert_eq!(s.advance(&mut w, 4, 8).unwrap(), 0);
    assert_eq!(s.advance(&mut w, 1, 8).unwrap(), 1); // 5 + 4 + 1 = 10
    assert_eq!(s.advance(&mut w, 1000, 3).unwrap(), 3); // capped, remainder dropped
    assert_eq!(s.advance(&mut w, 0, 3).unwrap(), 0);
    assert_eq!(s.frame(), 6);
    assert!(Schedule::new(0).is_err());
}

struct Tm {
    types: TypeManager,
    special: SpecialTypes,
    ltw: TypeIndex,
    t: TypeIndex,
    r: TypeIndex,
    parent: TypeIndex,
}

fn tm() -> Tm {
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("<reserved null>")).unwrap();
    let mut es = TypeSpec::component_data("Entity", 8, 8);
    es.category = TypeCategory::EntityData;
    let entity = types.register(es).unwrap();
    let ltw = types
        .register(TypeSpec::component_data("LocalToWorld", 64, 16))
        .unwrap();
    let t = types
        .register(TypeSpec::component_data("Translation", 12, 4))
        .unwrap();
    let r = types
        .register(TypeSpec::component_data("Rotation", 16, 16))
        .unwrap();
    let parent = types
        .register(TypeSpec::component_data("Parent", 8, 4))
        .unwrap();
    Tm {
        types,
        special: SpecialTypes::new(entity),
        ltw,
        t,
        r,
        parent,
    }
}

fn f(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

const ID_R: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// root (id 12) in archetype A; grandchild (10 -> 11) listed before child (11 -> 12) in B.
fn scene() -> (World, TransformTypes) {
    let m = tm();
    let (ltw, t, r, parent) = (m.ltw, m.t, m.r, m.parent);
    let input = WorldInput {
        archetypes: vec![
            ArchetypeInput {
                types: vec![ltw, t, r],
            },
            ArchetypeInput {
                types: vec![ltw, t, r, parent],
            },
        ],
        chunks: vec![
            ChunkInput {
                archetype: 0,
                entities: EntityIds::Preserved(vec![Entity::new(12, 1)]),
                components: vec![(t, f(&[1.0, 2.0, 3.0])), (r, f(&ID_R))],
            },
            ChunkInput {
                archetype: 1,
                entities: EntityIds::Preserved(vec![Entity::new(10, 1), Entity::new(11, 1)]),
                components: vec![
                    (t, f(&[0.0, 5.0, 0.0, 10.0, 0.0, 0.0])),
                    (r, f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])),
                    (
                        parent,
                        [11i32, 1, 12, 1]
                            .iter()
                            .flat_map(|x| x.to_le_bytes())
                            .collect(),
                    ),
                ],
            },
        ],
    };
    let w = World::build(m.types, m.special, 32, &input).unwrap();
    let tt = TransformTypes {
        local_to_world: ltw,
        local_transform: None,
        translation: Some(t),
        rotation: Some(r),
        scale: None,
        post_transform: None,
        parent: Some(parent),
    };
    (w, tt)
}

fn ltw_translation(w: &World, tt: &TransformTypes, e: Entity) -> [f32; 3] {
    let b = w.store.component_bytes(e, tt.local_to_world).unwrap();
    let v: Vec<f32> = b
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    [v[12], v[13], v[14]]
}

#[test]
fn transform_system_orders_parents_first_and_skips_unchanged() {
    let (mut w, tt) = scene();
    let mut s = Schedule::new(16_666_667).unwrap();
    s.add_group("Transform").unwrap();
    s.add_system("Transform", Box::new(TransformSystem::new(tt)))
        .unwrap();
    s.run_frame(&mut w).unwrap();
    let st = s
        .system::<TransformSystem>("TransformSystem")
        .unwrap()
        .stats();
    assert_eq!((st.root_entities_written, st.children_written), (1, 2));
    assert_eq!(
        ltw_translation(&w, &tt, Entity::new(12, 1)),
        [1.0, 2.0, 3.0]
    );
    assert_eq!(
        ltw_translation(&w, &tt, Entity::new(11, 1)),
        [11.0, 2.0, 3.0]
    );
    assert_eq!(
        ltw_translation(&w, &tt, Entity::new(10, 1)),
        [11.0, 7.0, 3.0]
    );
    let h = w.state_hash();
    let versions = w.component_versions(tt.local_to_world);

    s.run_frame(&mut w).unwrap();
    let st = s
        .system::<TransformSystem>("TransformSystem")
        .unwrap()
        .stats();
    assert_eq!(st.entities_written(), 0);
    assert_eq!((st.root_chunks_skipped, st.children_skipped), (1, 2));
    assert_eq!(w.state_hash(), h);
    assert_eq!(w.component_versions(tt.local_to_world), versions);

    // an outside write to the root's Translation propagates to both descendants
    w.store
        .set_component_bytes(
            Entity::new(12, 1),
            tt.translation.unwrap(),
            &f(&[4.0, 0.0, 0.0]),
        )
        .unwrap();
    s.run_frame(&mut w).unwrap();
    let st = s
        .system::<TransformSystem>("TransformSystem")
        .unwrap()
        .stats();
    assert_eq!((st.root_entities_written, st.children_written), (1, 2));
    assert_eq!(
        ltw_translation(&w, &tt, Entity::new(10, 1)),
        [14.0, 5.0, 0.0]
    );
    s.run_frame(&mut w).unwrap();
    assert_eq!(
        s.system::<TransformSystem>("TransformSystem")
            .unwrap()
            .stats()
            .entities_written(),
        0
    );
}

#[test]
fn transform_filter_observes_low_versions_after_counter_wrap() {
    let (mut w, tt) = scene();
    assert_eq!(w.store.global_system_version(), 1);
    let mut sys = TransformSystem::new(tt);
    // Current input versions are 1, newer than a system's pre-wrap last version.
    // An unsigned `1 > u32::MAX` incorrectly skips the entire transform hierarchy.
    let ctx = FrameContext {
        frame: 0,
        delta_ns: 1,
        elapsed_ns: 0,
        system_version: 1,
        last_system_version: u32::MAX,
    };
    sys.update(&mut w, &ctx).unwrap();
    assert_eq!(sys.stats().entities_written(), 3);
    assert_eq!(
        ltw_translation(&w, &tt, Entity::new(10, 1)),
        [11.0, 7.0, 3.0]
    );
    let hash = w.state_hash();
    let ctx = FrameContext {
        last_system_version: 1,
        ..ctx
    };
    sys.update(&mut w, &ctx).unwrap();
    assert_eq!(sys.stats().entities_written(), 0);
    assert_eq!(w.state_hash(), hash);
}

#[test]
fn transform_system_leaves_unresolved_parents_untouched() {
    let (mut w, tt) = scene();
    // child 11 now names a parent that does not exist
    w.store
        .set_component_bytes(
            Entity::new(11, 1),
            tt.parent.unwrap(),
            &[99i32, 1]
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
        .unwrap();
    let mut sys = TransformSystem::new(tt);
    let ctx = FrameContext {
        frame: 0,
        delta_ns: 1,
        elapsed_ns: 0,
        system_version: 1,
        last_system_version: 0,
    };
    sys.update(&mut w, &ctx).unwrap();
    let st = sys.stats();
    assert_eq!(st.children_unresolved, 1); // 11 dangles; 10's parent 11 is listed
    assert_eq!(ltw_translation(&w, &tt, Entity::new(11, 1)), [0.0; 3]);
    // 10 inherits 11's unwritten (all-zero) matrix, so its product is zero
    assert_eq!(ltw_translation(&w, &tt, Entity::new(10, 1)), [0.0; 3]);
}
