//! Shared experimental player motion driver. This is not the original scheduler.
use crate::ordinary_movement_ecs::OrdinaryMovementTypes;
use anyhow::{ensure, Context, Result};
use rrising_ecs::{
    Entity, FrameContext, Schedule, SpecialTypes, System, TypeIndex, TypeManager, TypeSpec, World,
    WorldInput,
};

pub const STEP_NS: u64 = 16_666_667;

/// Visual approximation driven by horizontal displacement, not the game's Animator.
pub struct MotionAnimation {
    previous: [f32; 3],
    phase_seconds: f32,
    moving: bool,
}

impl MotionAnimation {
    pub fn new(previous: [f32; 3]) -> Self {
        Self {
            previous,
            phase_seconds: 0.0,
            moving: false,
        }
    }
    pub fn advance(&mut self, position: [f32; 3], elapsed: f32, duration: f32) {
        self.moving = position[0] != self.previous[0] || position[2] != self.previous[2];
        if self.moving {
            log::debug!(target: "rrising::input", "player moved {:?} -> {position:?}", self.previous);
        }
        self.previous = position;
        if self.moving {
            self.phase_seconds = (self.phase_seconds + elapsed).rem_euclid(duration);
        }
    }
    pub fn frame(&self, duration: f32, count: usize) -> Option<usize> {
        self.moving
            .then(|| ((self.phase_seconds / duration * count as f32) as usize).min(count - 1))
    }
}

/// Copy of the specific component bytes consumed by the diagnostic driver.
/// Values captured from the owner's game remain local in a scene transfer.
#[derive(Clone, Debug)]
pub struct PlayerComponents {
    pub input: Vec<u8>,
    pub movement: Vec<u8>,
    pub velocity: Vec<u8>,
    pub move_velocity: Vec<u8>,
    pub translation: Vec<u8>,
    pub spell_controlled: Option<Vec<u8>>,
}

impl PlayerComponents {
    pub fn capture(world: &World, entity: Entity) -> Result<Self> {
        let get = |name: &str| -> Result<Vec<u8>> {
            let ty = world
                .types
                .type_index_of(name)
                .with_context(|| format!("missing {name}"))?;
            Ok(world.store.component_bytes(entity, ty)?.to_vec())
        };
        let spell_controlled = world
            .types
            .type_index_of("ProjectM.IsSpellControlled")
            .map(|ty| world.store.try_component_bytes(entity, ty))
            .transpose()?
            .flatten()
            .map(<[u8]>::to_vec);
        Ok(Self {
            input: get("ProjectM.EntityInput")?,
            movement: get("ProjectM.Movement")?,
            velocity: get("ProjectM.Velocity")?,
            move_velocity: get("ProjectM.MoveVelocity")?,
            translation: get("Unity.Transforms.Translation")?,
            spell_controlled,
        })
    }
}

/// The exact system used in both native and browser diagnostics. It deliberately
/// combines isolated recovered functions in a host-defined order and excludes collision.
pub struct ExperimentalPlayerMotion {
    pub entity: Entity,
    pub input: TypeIndex,
    pub types: OrdinaryMovementTypes,
}

impl System for ExperimentalPlayerMotion {
    fn name(&self) -> &str {
        "ExperimentalPlayerMotion"
    }
    fn update(&mut self, world: &mut World, ctx: &FrameContext) -> Result<()> {
        let input = world
            .store
            .component_bytes(self.entity, self.input)?
            .to_vec();
        let mut movement = world
            .store
            .component_bytes(self.entity, self.types.movement)?
            .to_vec();
        crate::movement::setup_ability_movement(&mut movement, &input)?;
        world
            .store
            .set_component_bytes(self.entity, self.types.movement, &movement)?;
        let mut command = [0u8; crate::command_movement::INPUT_COMMAND_SIZE];
        crate::command_movement::write_command_input_from_world(
            world,
            self.entity,
            &mut command,
            self.input,
        )?;
        command[144..148].copy_from_slice(&(ctx.delta_ns as f32 * 1e-9).to_le_bytes());
        crate::ordinary_movement_ecs::step_ordinary_command_bytes(
            world,
            self.entity,
            self.types,
            &command,
            true,
        )
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A small ECS projection holding only the components used by the shared driver.
pub struct DiagnosticPlayer {
    world: World,
    entity: Entity,
    input: TypeIndex,
    translation: TypeIndex,
    schedule: Schedule,
    last_direction: [f32; 3],
}

impl DiagnosticPlayer {
    pub fn from_components(data: &PlayerComponents) -> Result<Self> {
        for (name, bytes, size) in [
            ("EntityInput", &data.input, 96),
            ("Movement", &data.movement, 104),
            ("Velocity", &data.velocity, 12),
            ("MoveVelocity", &data.move_velocity, 12),
            ("Translation", &data.translation, 12),
        ] {
            ensure!(bytes.len() == size, "{name} has invalid length");
        }
        if let Some(bytes) = &data.spell_controlled {
            ensure!(bytes.len() == 2, "IsSpellControlled has invalid length");
        }
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("<reserved>"))?;
        let entity_type = types.register(TypeSpec::component_data("Entity", 8, 8))?;
        let input = types.register(TypeSpec::component_data("ProjectM.EntityInput", 96, 16))?;
        let movement = types.register(TypeSpec::component_data("ProjectM.Movement", 104, 16))?;
        let velocity = types.register(TypeSpec::component_data("ProjectM.Velocity", 12, 16))?;
        let move_velocity =
            types.register(TypeSpec::component_data("ProjectM.MoveVelocity", 12, 16))?;
        let translation = types.register(TypeSpec::component_data(
            "Unity.Transforms.Translation",
            12,
            16,
        ))?;
        let spell = if data.spell_controlled.is_some() {
            Some(types.register(TypeSpec::component_data("ProjectM.IsSpellControlled", 2, 2))?)
        } else {
            None
        };
        let mut world = World::build(
            types,
            SpecialTypes::new(entity_type),
            8,
            &WorldInput::default(),
        )?;
        let mut component_types = vec![input, movement, velocity, move_velocity, translation];
        if let Some(spell) = spell {
            component_types.push(spell);
        }
        let archetype = world
            .store
            .get_or_create_archetype(&world.types, &component_types)?;
        let entity = world.store.create_entity(archetype)?;
        for (ty, bytes) in [
            (input, &data.input),
            (movement, &data.movement),
            (velocity, &data.velocity),
            (move_velocity, &data.move_velocity),
            (translation, &data.translation),
        ] {
            world.store.set_component_bytes(entity, ty, bytes)?;
        }
        if let (Some(ty), Some(bytes)) = (spell, &data.spell_controlled) {
            world.store.set_component_bytes(entity, ty, bytes)?;
        }
        let mut schedule = Schedule::new(STEP_NS)?;
        schedule.add_group("ExperimentalPlayer")?;
        schedule.add_system(
            "ExperimentalPlayer",
            Box::new(ExperimentalPlayerMotion {
                entity,
                input,
                types: OrdinaryMovementTypes {
                    movement,
                    velocity,
                    move_velocity,
                    translation,
                },
            }),
        )?;
        Ok(Self {
            world,
            entity,
            input,
            translation,
            schedule,
            last_direction: [0.; 3],
        })
    }

    /// Feed camera-relative axes and advance fixed steps with the native diagnostic's cap.
    pub fn advance(&mut self, axis: [f32; 2], yaw: f32, elapsed_ns: u64) -> Result<u32> {
        ensure!(
            axis.iter().all(|v| v.is_finite() && v.abs() <= 1.0) && yaw.is_finite(),
            "invalid input axes"
        );
        let mut state = crate::player_movement::MovementInput {
            axis,
            mode: 0,
            slow: false,
            last_direction: self.last_direction,
        };
        let movement = crate::player_movement::get_movement(
            &mut state,
            [0., (yaw * 0.5).sin(), 0., (yaw * 0.5).cos()],
        );
        self.last_direction = state.last_direction;
        crate::player_input::write_player_movement(
            &mut self.world,
            self.entity,
            self.input,
            [movement[0], movement[2]],
        )?;
        self.schedule.advance(&mut self.world, elapsed_ns, 4)
    }

    pub fn position(&self) -> Result<[f32; 3]> {
        let bytes = self
            .world
            .store
            .component_bytes(self.entity, self.translation)?;
        ensure!(bytes.len() == 12, "invalid translation");
        Ok(std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("checked size"))
        }))
    }
}
