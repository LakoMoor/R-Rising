//! Bounded Movement write in AbilityCast_SetupStartValues_Shared.
//! Other writes, singleton lifecycle and scheduling are not implemented.
use anyhow::{ensure, Context, Result};
use rrising_ecs::{EntityQuery, TypeIndex, World};

use crate::movement::setup_ability_movement;

/// Identities for the observed five-component query and optional client gates.
pub struct AbilityMovementTypes {
    pub movement: TypeIndex,
    pub target_direction: TypeIndex,
    pub input: TypeIndex,
    pub translation: TypeIndex,
    pub rotation: TypeIndex,
    pub local_controlled: Option<TypeIndex>,
    pub network_interpolated: Option<TypeIndex>,
}

impl AbilityMovementTypes {
    pub fn from_world(world: &World) -> Result<Self> {
        let required = |name: &str| {
            world
                .types
                .type_index_of(name)
                .with_context(|| format!("missing {name}"))
        };
        Ok(Self {
            movement: required("ProjectM.Movement")?,
            target_direction: required("ProjectM.TargetDirection")?,
            input: required("ProjectM.EntityInput")?,
            translation: required("Unity.Transforms.Translation")?,
            rotation: required("Unity.Transforms.Rotation")?,
            local_controlled: world
                .types
                .type_index_of("ProjectM.Network.LocalControlled"),
            network_interpolated: world
                .types
                .type_index_of("ProjectM.Network.NetworkInterpolated_Shared"),
        })
    }

    pub fn query(&self) -> EntityQuery {
        EntityQuery {
            all: vec![
                self.movement,
                self.target_direction,
                self.input,
                self.translation,
                self.rotation,
            ],
            ..Default::default()
        }
    }
}

/// Execute only the recovered Movement body for matched entities. Caller supplies
/// world kind and ensures the original ModificationsRegistry singleton exists.
/// Default query exclusion is the port's partial ECS implementation.
pub fn apply_ability_movement_body(
    world: &mut World,
    types: &AbilityMovementTypes,
    is_client: bool,
) -> Result<usize> {
    let mut count = 0;
    for entity in world.store.query_entities(&types.query())? {
        if is_client {
            let local = match types.local_controlled {
                Some(ty) => world.store.try_component_bytes(entity, ty)?.is_some(),
                None => false,
            };
            if !local {
                if let Some(ty) = types.network_interpolated {
                    if let Some(bytes) = world.store.try_component_bytes(entity, ty)? {
                        ensure!(bytes.len() > 0x44, "NetworkInterpolated_Shared too short");
                        if bytes[0x44] == 0 {
                            continue;
                        }
                    }
                }
            }
        }
        let input = world.store.component_bytes(entity, types.input)?.to_vec();
        let mut movement = world
            .store
            .component_bytes(entity, types.movement)?
            .to_vec();
        setup_ability_movement(&mut movement, &input)?;
        world
            .store
            .set_component_bytes(entity, types.movement, &movement)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, WorldInput};

    #[test]
    fn client_gate_and_server_branch() {
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("reserved")).unwrap();
        let entity_type = types
            .register(TypeSpec::component_data("Entity", 8, 8))
            .unwrap();
        let movement = types
            .register(TypeSpec::component_data("ProjectM.Movement", 104, 4))
            .unwrap();
        let target = types
            .register(TypeSpec::component_data("ProjectM.TargetDirection", 48, 4))
            .unwrap();
        let input = types
            .register(TypeSpec::component_data("ProjectM.EntityInput", 96, 4))
            .unwrap();
        let translation = types
            .register(TypeSpec::component_data(
                "Unity.Transforms.Translation",
                12,
                4,
            ))
            .unwrap();
        let rotation = types
            .register(TypeSpec::component_data("Unity.Transforms.Rotation", 16, 4))
            .unwrap();
        let network = types
            .register(TypeSpec::component_data(
                "ProjectM.Network.NetworkInterpolated_Shared",
                72,
                4,
            ))
            .unwrap();
        let local = types
            .register(TypeSpec::tag("ProjectM.Network.LocalControlled"))
            .unwrap();
        let mut world = World::build(
            types,
            SpecialTypes::new(entity_type),
            8,
            &WorldInput::default(),
        )
        .unwrap();
        let arch = world
            .store
            .get_or_create_archetype(
                &world.types,
                &[movement, target, input, translation, rotation, network],
            )
            .unwrap();
        let entity = world.store.create_entity(arch).unwrap();
        let mut raw = [0u8; 96];
        raw[..4].copy_from_slice(&0.5f32.to_le_bytes());
        world
            .store
            .set_component_bytes(entity, input, &raw)
            .unwrap();
        let config = AbilityMovementTypes::from_world(&world).unwrap();
        assert_eq!(
            apply_ability_movement_body(&mut world, &config, true).unwrap(),
            0
        );
        assert_eq!(
            apply_ability_movement_body(&mut world, &config, false).unwrap(),
            1
        );
        assert_eq!(
            &world.store.component_bytes(entity, movement).unwrap()[76..80],
            &raw[..4]
        );
        let mut net = [0u8; 72];
        net[0x44] = 1;
        world
            .store
            .set_component_bytes(entity, network, &net)
            .unwrap();
        assert_eq!(
            apply_ability_movement_body(&mut world, &config, true).unwrap(),
            1
        );
        let local_arch = world
            .store
            .get_or_create_archetype(
                &world.types,
                &[
                    movement,
                    target,
                    input,
                    translation,
                    rotation,
                    network,
                    local,
                ],
            )
            .unwrap();
        let local_entity = world.store.create_entity(local_arch).unwrap();
        world
            .store
            .set_component_bytes(local_entity, input, &raw)
            .unwrap();
        assert_eq!(
            apply_ability_movement_body(&mut world, &config, true).unwrap(),
            2
        );
    }
}
