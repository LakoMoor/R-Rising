//! Compare pure data computation against the captured original job with explicit
//! supplied-value ECS boundaries. This does not execute the original ECS scheduler.
use rrising_core::{
    movement::{
        normalize_safe_float2, update_steering_input, SharedSteeringSystem, SharedSteeringTypes,
        SteeringParameters,
    },
    pathfinding::PathWaypoints,
};
use rrising_ecs::{
    ArchetypeInput, ChunkInput, Entity, EntityIds, Schedule, SpecialTypes, TypeCategory,
    TypeManager, TypeSpec, World, WorldInput,
};

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}

struct Case {
    parameters: SteeringParameters,
    old: Vec<u8>,
    expected: Vec<u8>,
    normalized: [u32; 2],
}

fn cases() -> impl Iterator<Item = Case> {
    include_str!("../../../evidence/oracles/movement_shared.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 21);
            let hex = |at| u32::from_str_radix(fields[at], 16).unwrap();
            let float = |at| f32::from_bits(hex(at));
            let parameters = SteeringParameters {
                buff_translation: [float(6), float(7), float(8)],
                target_translation: [float(9), float(10), float(11)],
                target_velocity: [float(12), float(13), float(14)],
                world_space_radius: float(15),
                distance_sq: float(16),
                waypoints: PathWaypoints {
                    waypoint: [hex(0) as i32, hex(1) as i32],
                    next_waypoint: [hex(2) as i32, hex(3) as i32],
                    has_waypoint: fields[4] != "0",
                    has_next_waypoint: fields[5] != "0",
                },
            };
            Case {
                parameters,
                old: bytes(fields[17]),
                expected: bytes(fields[20]),
                normalized: [hex(18), hex(19)],
            }
        })
}

#[test]
fn shared_steering_matches_original_job_and_preserves_other_input() {
    let mut count = 0;
    for case in cases() {
        let p = case.parameters;
        let normalized = normalize_safe_float2(
            [p.target_velocity[0], p.target_velocity[2]],
            [p.buff_translation[0], p.buff_translation[2]],
        );
        assert_eq!(
            normalized.map(f32::to_bits),
            case.normalized,
            "normalize row {count}"
        );
        let mut input = case.old.clone();
        update_steering_input(&p, &mut input).unwrap();
        assert_eq!(input, case.expected, "steering row {count}");
        assert_eq!(input[32..], case.old[32..]);
        count += 1;
    }
    assert_eq!(count, 4096);
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn world(case: &Case, excluded: Option<&str>) -> (World, SharedSteeringTypes, Entity) {
    let mut tm = TypeManager::default();
    tm.register(TypeSpec::tag("<null>")).unwrap();
    let mut spec = TypeSpec::component_data("Entity", 8, 8);
    spec.category = TypeCategory::EntityData;
    let entity_type = tm.register(spec).unwrap();
    let types = SharedSteeringTypes {
        buff: tm
            .register(TypeSpec::component_data("Buff", 32, 8))
            .unwrap(),
        move_towards_position_buff: tm
            .register(TypeSpec::component_data("MoveTowardsPositionBuff", 40, 8))
            .unwrap(),
        translation: tm
            .register(TypeSpec::component_data("Translation", 12, 4))
            .unwrap(),
        velocity: tm
            .register(TypeSpec::component_data("Velocity", 12, 4))
            .unwrap(),
        map_collision: tm
            .register(TypeSpec::component_data("MapCollision", 20, 4))
            .unwrap(),
        entity_input: tm
            .register(TypeSpec::component_data("EntityInput", 96, 4))
            .unwrap(),
    };
    let mut special = SpecialTypes::new(entity_type);
    let mut source_types = vec![
        types.buff,
        types.move_towards_position_buff,
        types.translation,
    ];
    if let Some(name) = excluded {
        let tag = tm.register(TypeSpec::tag(name)).unwrap();
        if name == "Prefab" {
            special.prefab = tag;
        } else {
            special.disabled = tag;
        }
        source_types.push(tag);
    }
    let p = &case.parameters;
    let target = Entity::new(77, 3); // Same handle checked by the native oracle's callbacks.
    let mut buff = vec![0; 32];
    buff[8..12].copy_from_slice(&target.index.to_le_bytes());
    buff[12..16].copy_from_slice(&target.version.to_le_bytes());
    let mut move_data = vec![0; 40];
    move_data[..4].copy_from_slice(&p.distance_sq.to_le_bytes());
    for (at, v) in p
        .waypoints
        .waypoint
        .into_iter()
        .chain(p.waypoints.next_waypoint)
        .enumerate()
    {
        move_data[16 + at * 4..20 + at * 4].copy_from_slice(&v.to_le_bytes());
    }
    move_data[32] = u8::from(p.waypoints.has_waypoint);
    move_data[33] = u8::from(p.waypoints.has_next_waypoint);
    let mut collision = vec![0; 20];
    collision[..4].copy_from_slice(&p.world_space_radius.to_le_bytes());
    let input = WorldInput {
        archetypes: vec![
            ArchetypeInput {
                types: source_types,
            },
            ArchetypeInput {
                types: vec![
                    types.translation,
                    types.velocity,
                    types.map_collision,
                    types.entity_input,
                ],
            },
        ],
        chunks: vec![
            ChunkInput {
                shared_components: vec![],
                archetype: 0,
                entities: EntityIds::Preserved(vec![Entity::new(31, 1)]),
                components: vec![
                    (types.buff, buff),
                    (types.move_towards_position_buff, move_data),
                    (types.translation, floats(&p.buff_translation)),
                ],
            },
            ChunkInput {
                shared_components: vec![],
                archetype: 1,
                entities: EntityIds::Preserved(vec![target]),
                components: vec![
                    (types.translation, floats(&p.target_translation)),
                    (types.velocity, floats(&p.target_velocity)),
                    (types.map_collision, collision),
                    (types.entity_input, case.old.clone()),
                ],
            },
        ],
    };
    (
        World::build(tm, special, 128, &input).unwrap(),
        types,
        target,
    )
}

fn schedule(types: SharedSteeringTypes) -> Schedule {
    let mut s = Schedule::new(16_666_667).unwrap();
    s.add_group("SetInput").unwrap();
    s.add_system("SetInput", Box::new(SharedSteeringSystem::new(types)))
        .unwrap();
    s
}

#[test]
fn world_schedule_updates_the_original_target_on_all_native_cases() {
    let mut count = 0;
    for case in cases() {
        let (mut world, types, target) = world(&case, None);
        let mut s = schedule(types);
        s.run_frame(&mut world).unwrap();
        assert_eq!(
            s.system::<SharedSteeringSystem>(SharedSteeringSystem::NAME)
                .unwrap()
                .entities_updated,
            1
        );
        assert_eq!(
            world
                .store
                .component_bytes(target, types.entity_input)
                .unwrap(),
            case.expected,
            "world row {count}"
        );
        // The buff's own position is the destination, not the entity being moved.
        assert_eq!(
            world
                .store
                .component_bytes(Entity::new(31, 1), types.translation)
                .unwrap(),
            floats(&case.parameters.buff_translation)
        );
        count += 1;
    }
    assert_eq!(count, 4096);
}

#[test]
fn query_excludes_prefabs_and_disabled_and_missing_targets_are_errors() {
    let case = cases().next().unwrap();
    for name in ["Prefab", "Disabled"] {
        let (mut w, t, target) = world(&case, Some(name));
        let before = w.state_hash();
        let mut s = schedule(t);
        s.run_frame(&mut w).unwrap();
        assert_eq!(
            s.system::<SharedSteeringSystem>(SharedSteeringSystem::NAME)
                .unwrap()
                .entities_updated,
            0
        );
        assert_eq!(w.state_hash(), before);
        assert_eq!(
            w.store.component_bytes(target, t.entity_input).unwrap(),
            case.old
        );
    }
    let (mut w, t, target) = world(&case, None);
    w.store.destroy_entity(target).unwrap();
    assert!(schedule(t).run_frame(&mut w).is_err());
    let mut short = [0xa5; 95];
    assert!(update_steering_input(&case.parameters, &mut short).is_err());
    assert_eq!(short, [0xa5; 95]);
}
