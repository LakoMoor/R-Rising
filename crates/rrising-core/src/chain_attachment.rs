//! Essential non-static attachment, measured independently of prefab creation and placement.
use crate::{auto_chain::AutoChainClock, transition_selection::PrefabCandidate};
use anyhow::{ensure, Result};
use rrising_ecs::{Entity, TypeIndex, World};

#[derive(Debug, PartialEq)]
pub struct Attachment {
    pub clock: Option<[u8; 8]>,
    pub child: [u8; 12],
    pub networked_child: Option<[u8; 12]>,
    pub active: [u8; 12],
    pub changed: [u8; 8],
}
fn entity_bytes(entity: Entity) -> [u8; 8] {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&entity.index.to_le_bytes());
    bytes[4..].copy_from_slice(&entity.version.to_le_bytes());
    bytes
}
/// Original attachment writes an auto clock only when the optional config is present.
/// In its absence, an existing parent clock is left untouched.
pub fn plan(
    chain: Entity,
    new_instance: Entity,
    candidate: &PrefabCandidate,
    prefab_networked: bool,
    now: f64,
) -> Attachment {
    let mut child = [0; 12];
    child[..8].copy_from_slice(&entity_bytes(chain));
    child[8..].copy_from_slice(&candidate.element_index.to_le_bytes());
    let mut active = [0; 12];
    active[..4].copy_from_slice(&candidate.element_index.to_le_bytes());
    active[4..].copy_from_slice(&entity_bytes(new_instance));
    let mut network = [0; 12];
    network[..8].copy_from_slice(&entity_bytes(chain));
    Attachment {
        clock: candidate.auto_transition.map(|config| {
            AutoChainClock::create(true, config.duration, now, candidate.forwarded_seconds)
                .next_attempt
                .to_le_bytes()
        }),
        child,
        networked_child: prefab_networked.then_some(network),
        active,
        changed: entity_bytes(chain),
    }
}

pub struct AttachmentTypes {
    pub child: TypeIndex,
    pub disabled: TypeIndex,
    pub active: TypeIndex,
    pub clock: TypeIndex,
    pub networked_child: TypeIndex,
    pub static_compatible: TypeIndex,
    pub changed_event: TypeIndex,
}
/// Attach a caller-created, already-shaped child. This is immediate ECS application;
/// original ECB playback order relative to other systems has not been recovered.
/// Does not clone a prefab, apply TRS, delete the previous child or consume requests.
pub fn apply_existing(
    world: &mut World,
    chain: Entity,
    new_instance: Entity,
    candidate: &PrefabCandidate,
    types: &AttachmentTypes,
    now: f64,
) -> Result<Entity> {
    ensure!(chain != new_instance, "child aliases chain entity");
    ensure!(
        world.store.exists(candidate.prefab)?,
        "prefab entity is not live"
    );
    ensure!(
        world
            .store
            .try_component_bytes(candidate.prefab, types.static_compatible)?
            .is_none(),
        "static-transform attachment is not implemented"
    );
    let networked = world
        .store
        .try_component_bytes(candidate.prefab, types.networked_child)?
        .is_some();
    let values = plan(chain, new_instance, candidate, networked, now);
    ensure!(
        world
            .store
            .try_component_bytes(new_instance, types.disabled)?
            .is_some_and(|value| value.is_empty()),
        "Disabled tag must be prepared on the child"
    );
    ensure!(
        world
            .store
            .component_bytes(new_instance, types.child)?
            .len()
            == 12,
        "child slot must be prepared"
    );
    ensure!(
        world.store.component_bytes(chain, types.active)?.len() == 12,
        "active size"
    );
    if values.clock.is_some() {
        ensure!(
            world.store.component_bytes(chain, types.clock)?.len() == 8,
            "clock slot must be prepared"
        );
    }
    if networked {
        ensure!(
            world
                .store
                .component_bytes(new_instance, types.networked_child)?
                .len()
                == 12,
            "networked slot must be prepared"
        );
    }
    ensure!(
        world
            .types
            .type_info(types.changed_event)
            .is_some_and(|info| info.size_in_chunk == 8),
        "changed-event size"
    );
    let event_archetype = world
        .store
        .get_or_create_archetype(&world.types, &[types.changed_event])?;
    let event = world.store.create_entity(event_archetype)?;
    world
        .store
        .set_component_bytes(event, types.changed_event, &values.changed)?;
    if let Some(clock) = values.clock {
        world
            .store
            .set_component_bytes(chain, types.clock, &clock)?;
    }
    world
        .store
        .set_component_bytes(new_instance, types.child, &values.child)?;
    if let Some(network) = values.networked_child {
        world
            .store
            .set_component_bytes(new_instance, types.networked_child, &network)?;
    }
    world
        .store
        .set_component_bytes(chain, types.active, &values.active)?;
    Ok(event)
}
