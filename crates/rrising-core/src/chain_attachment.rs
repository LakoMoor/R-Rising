//! Essential attachment, measured independently of prefab creation and placement.
use crate::{auto_chain::AutoChainClock, transition_selection::PrefabCandidate};
use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex, World};

#[derive(Debug, PartialEq)]
pub struct Attachment {
    pub clock: Option<[u8; 8]>,
    pub child: [u8; 12],
    pub networked_child: Option<[u8; 12]>,
    pub active: [u8; 12],
    pub changed: [u8; 8],
    pub static_data: Option<StaticAttachment>,
}
pub struct StaticInput {
    pub prefab_compatible: [u8; 24],
    pub chain_index: Option<[u8; 4]>,
    pub prefab_mega_compatible: bool,
}
#[derive(Debug, PartialEq)]
pub struct StaticAttachment {
    pub compatible: [u8; 24],
    pub guaranteed_static: bool,
    pub mega_static: bool,
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
        static_data: None,
    }
}

/// Preserve prefab padding/transform bytes; only the use flag and packed index change.
pub fn plan_with_static(
    chain: Entity,
    new_instance: Entity,
    candidate: &PrefabCandidate,
    prefab_networked: bool,
    now: f64,
    static_input: Option<&StaticInput>,
) -> Attachment {
    let mut output = plan(chain, new_instance, candidate, prefab_networked, now);
    output.static_data = static_input.map(|input| {
        let has_index = input.chain_index.is_some();
        let mut compatible = input.prefab_compatible;
        compatible[0] = u8::from(has_index);
        compatible[2..6].copy_from_slice(&input.chain_index.unwrap_or([0; 4]));
        StaticAttachment {
            compatible,
            guaranteed_static: has_index,
            mega_static: has_index && input.prefab_mega_compatible,
        }
    });
    output
}

pub struct StaticAttachmentTypes {
    pub index: TypeIndex,
    pub mega_compatible: TypeIndex,
    pub guaranteed_static: TypeIndex,
    pub static_tag: TypeIndex,
    pub mega_static: TypeIndex,
}

pub struct AttachmentTypes {
    pub child: TypeIndex,
    pub disabled: TypeIndex,
    pub active: TypeIndex,
    pub clock: TypeIndex,
    pub networked_child: TypeIndex,
    pub static_compatible: TypeIndex,
    pub changed_event: TypeIndex,
    pub static_types: Option<StaticAttachmentTypes>,
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
    let static_input = world
        .store
        .try_component_bytes(candidate.prefab, types.static_compatible)?
        .map(|source| -> Result<StaticInput> {
            let bindings = types
                .static_types
                .as_ref()
                .context("static-transform type bindings are missing")?;
            let chain_index = world
                .store
                .try_component_bytes(chain, bindings.index)?
                .map(|value| -> Result<[u8; 4]> { Ok(value.try_into()?) })
                .transpose()?;
            Ok(StaticInput {
                prefab_compatible: source.try_into()?,
                prefab_mega_compatible: chain_index.is_some()
                    && world
                        .store
                        .try_component_bytes(candidate.prefab, bindings.mega_compatible)?
                        .is_some(),
                chain_index,
            })
        })
        .transpose()?;
    let networked = world
        .store
        .try_component_bytes(candidate.prefab, types.networked_child)?
        .is_some();
    let values = plan_with_static(
        chain,
        new_instance,
        candidate,
        networked,
        now,
        static_input.as_ref(),
    );
    if let Some(static_data) = &values.static_data {
        ensure!(
            world
                .store
                .component_bytes(new_instance, types.static_compatible)?
                .len()
                == 24,
            "static-compatible slot must be prepared"
        );
        let bindings = types
            .static_types
            .as_ref()
            .context("static type bindings")?;
        if static_data.guaranteed_static {
            for tag in [bindings.guaranteed_static, bindings.static_tag] {
                ensure!(
                    world
                        .store
                        .try_component_bytes(new_instance, tag)?
                        .is_some_and(|value| value.is_empty()),
                    "static tag must be prepared"
                );
            }
        }
        if static_data.mega_static {
            ensure!(
                world
                    .store
                    .try_component_bytes(new_instance, bindings.mega_static)?
                    .is_some_and(|value| value.is_empty()),
                "mega-static tag must be prepared"
            );
        }
    }
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
    if let Some(static_data) = values.static_data {
        world.store.set_component_bytes(
            new_instance,
            types.static_compatible,
            &static_data.compatible,
        )?;
    }
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
