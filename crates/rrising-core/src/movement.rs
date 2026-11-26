//! Shared `MoveTowardsPosition` steering, not locomotion or collision resolution.
//! Original job RVA 0x669a880; helper RVA 0x63354f0. Controlled native replay
//! supplies ECS accessors and a sqrt wrapper; see evidence/movement_contract.md.
use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, EntityQuery, FrameContext, System, TypeIndex, World};

use crate::pathfinding::{try_get_target_position_from_waypoints, PathWaypoints};

/// Selected fields from the pinned build's 104-byte `ProjectM.Movement` component.
/// The force-position and collision payloads remain opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterMovementFields {
    pub move_input: [f32; 2],
    pub last_nonzero_move_input: [f32; 2],
    pub speed: f32,
    pub ability_cast_speed_multiplier: f32,
    pub has_move_stop_data: bool,
}

impl CharacterMovementFields {
    /// Decode component values without modifying the surrounding force-position data.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() == 104, "Movement requires 104 bytes");
        let f = |at| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked size"));
        Ok(Self {
            move_input: [f(76), f(80)],
            last_nonzero_move_input: [f(84), f(88)],
            speed: f(92),
            ability_cast_speed_multiplier: f(96),
            has_move_stop_data: bytes[100] != 0,
        })
    }
}

#[cfg(test)]
mod character_movement_tests {
    use super::CharacterMovementFields;

    #[test]
    fn reads_fields_and_rejects_wrong_size() {
        let mut bytes = [0u8; 104];
        for (offset, value) in [
            (76, -0.5f32),
            (80, 0.25),
            (84, 1.0),
            (88, -1.0),
            (92, 4.5),
            (96, 0.75),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[100] = 1;
        let fields = CharacterMovementFields::read(&bytes).unwrap();
        assert_eq!(fields.move_input, [-0.5, 0.25]);
        assert_eq!(fields.last_nonzero_move_input, [1.0, -1.0]);
        assert_eq!(fields.speed, 4.5);
        assert_eq!(fields.ability_cast_speed_multiplier, 0.75);
        assert!(fields.has_move_stop_data);
        assert!(CharacterMovementFields::read(&bytes[..103]).is_err());
        assert!(CharacterMovementFields::read(&[0; 105]).is_err());
    }
}

/// Inputs of one shared steering update. World coordinates are projected onto XZ.
#[derive(Clone, Copy, Debug)]
pub struct SteeringParameters {
    pub buff_translation: [f32; 3],
    pub target_translation: [f32; 3],
    pub target_velocity: [f32; 3],
    pub world_space_radius: f32,
    pub distance_sq: f32,
    pub waypoints: PathWaypoints,
}

/// The original float2 helper at RVA 0x63354f0, with a caller-provided fallback.
/// Infinite squared length is accepted; this is not a finite-vector guard.
pub fn normalize_safe_float2(value: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    let dot = value[1] * value[1] + value[0] * value[0];
    if dot > f32::MIN_POSITIVE {
        let inv = 1.0 / dot.sqrt();
        value.map(|v| {
            let scaled = v * inv;
            // SSE's invalid Inf*0 result is negative indefinite. Preserve its bits
            // rather than inheriting an architecture-dependent default NaN sign.
            if scaled.is_nan() {
                f32::from_bits(0xffc0_0000)
            } else {
                scaled
            }
        })
    } else {
        fallback
    }
}

/// Compute the original shared job's first 32 EntityInput bytes, keeping bytes
/// 32..96 untouched. It always updates both aim positions, even when movement stops.
pub fn update_steering_input(params: &SteeringParameters, input: &mut [u8]) -> Result<()> {
    ensure!(input.len() == 96, "EntityInput needs 96 bytes");
    let position = [params.target_translation[0], params.target_translation[2]];
    let velocity = [params.target_velocity[0], params.target_velocity[2]];
    let destination = [params.buff_translation[0], params.buff_translation[2]];
    let mut target = [0.0; 2];
    if !try_get_target_position_from_waypoints(
        &params.waypoints,
        position,
        velocity,
        params.world_space_radius,
        &mut target,
    ) {
        target = destination;
    }
    let dir = normalize_safe_float2([target[0] - position[0], target[1] - position[1]], [0.0; 2]);
    let dx = position[0] - destination[0];
    let dz = position[1] - destination[1];
    let dist2 = dz * dz + dx * dx;
    // COMISS DistanceSq,dist2; SETA: unordered values also keep movement.
    let movement = if params.distance_sq > dist2 {
        [0.0; 2]
    } else {
        dir
    };
    let aim = [dir[0] + position[0], 0.0, dir[1] + position[1]];
    for (out, value) in input[..32]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(movement.into_iter().chain(aim).chain(aim))
    {
        out.copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

/// The original metadata's component identities, supplied by the World type registry.
#[derive(Clone, Copy, Debug)]
pub struct SharedSteeringTypes {
    pub buff: TypeIndex,
    pub move_towards_position_buff: TypeIndex,
    pub translation: TypeIndex,
    pub map_collision: TypeIndex,
    pub velocity: TypeIndex,
    pub entity_input: TypeIndex,
}

impl SharedSteeringTypes {
    /// Query recovered from the original builder at RVA 0x669a3f0.
    pub fn query(&self) -> EntityQuery {
        EntityQuery {
            all: vec![self.buff, self.move_towards_position_buff, self.translation],
            ..Default::default()
        }
    }

    /// Resolve only when all six original component identities are in this World.
    pub fn from_hashes(types: &std::collections::BTreeMap<u64, TypeIndex>) -> Option<Self> {
        Some(Self {
            buff: *types.get(&0x5c06_8978_8602_b4e5)?,
            move_towards_position_buff: *types.get(&0x8093_8861_9ae0_80d7)?,
            translation: *types.get(&0xa42d_e7cc_0763_e5be)?,
            map_collision: *types.get(&0x179e_efa3_019c_a077)?,
            velocity: *types.get(&0x8da5_6f73_0a02_64b0)?,
            entity_input: *types.get(&0xcf49_a498_d747_4572)?,
        })
    }
}

/// World adapter for the captured shared steering body. It reads Buff.Target and
/// writes that target's EntityInput using the real store, preserving other input.
/// Original scheduler, path recalculation and collision behavior are not implied.
pub struct SharedSteeringSystem {
    types: SharedSteeringTypes,
    pub entities_updated: usize,
}

impl SharedSteeringSystem {
    pub const NAME: &'static str = "MoveTowardsPositionSystem_Shared_Update";

    pub fn new(types: SharedSteeringTypes) -> Self {
        Self {
            types,
            entities_updated: 0,
        }
    }
}

fn component(world: &World, entity: Entity, ty: TypeIndex, size: usize) -> Result<&[u8]> {
    let data = world.store.component_bytes(entity, ty)?;
    ensure!(
        data.len() == size,
        "steering component {ty:?} needs {size} bytes, got {}",
        data.len()
    );
    Ok(data)
}

fn vector(bytes: &[u8]) -> [f32; 3] {
    let mut v = [0.0; 3];
    for (out, input) in v.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *out = f32::from_le_bytes(*input);
    }
    v
}

impl System for SharedSteeringSystem {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn update(&mut self, world: &mut World, _ctx: &FrameContext) -> Result<()> {
        self.entities_updated = 0;
        let t = self.types;
        for entity in world.store.query_entities(&t.query())? {
            let buff = component(world, entity, t.buff, 32)?;
            let target = Entity::new(
                i32::from_le_bytes(buff[8..12].try_into().expect("checked Buff size")),
                i32::from_le_bytes(buff[12..16].try_into().expect("checked Buff size")),
            );
            let target = world.resolve(target).with_context(|| {
                format!("steering buff {entity:?} has no live target {target:?}")
            })?;
            let move_data = component(world, entity, t.move_towards_position_buff, 40)?;
            let parameters = SteeringParameters {
                buff_translation: vector(component(world, entity, t.translation, 12)?),
                target_translation: vector(component(world, target, t.translation, 12)?),
                target_velocity: vector(component(world, target, t.velocity, 12)?),
                world_space_radius: f32::from_le_bytes(
                    component(world, target, t.map_collision, 20)?[..4]
                        .try_into()
                        .expect("checked MapCollision size"),
                ),
                distance_sq: f32::from_le_bytes(
                    move_data[..4]
                        .try_into()
                        .expect("checked MoveTowardsPositionBuff size"),
                ),
                waypoints: PathWaypoints::from_bytes(&move_data[16..36])?,
            };
            let mut input: [u8; 96] = component(world, target, t.entity_input, 96)?
                .try_into()
                .expect("checked EntityInput size");
            update_steering_input(&parameters, &mut input)?;
            world
                .store
                .set_component_bytes(target, t.entity_input, &input)?;
            self.entities_updated += 1;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
