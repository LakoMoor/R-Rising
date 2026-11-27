//! Experimental ECS bridge for the bounded non-normalizing automatic-event path.
//! Caller owns binding/order; original scheduling, debug and event consumption are absent.
use crate::auto_chain::{progress, AutoProgressElement};
use anyhow::{ensure, Context, Result};
use rrising_content::spawn_chain::ChainDefinition;
use rrising_ecs::{Entity, TypeIndex, World};
pub struct AutoEventTypes {
    pub clock: TypeIndex,
    pub active: TypeIndex,
    pub instance: TypeIndex,
    pub event: TypeIndex,
}
#[derive(Debug, PartialEq)]
pub enum AutoEventOutcome {
    Skipped,
    Initialized,
    Disabled,
    Retry,
    Queued(Entity),
    UnsupportedNormalization,
}
/// Process one explicitly bound chain. Caller limits total emitted events per original job.
pub fn apply_entity(
    world: &mut World,
    entity: Entity,
    chain: &ChainDefinition,
    types: &AutoEventTypes,
    now: f64,
) -> Result<AutoEventOutcome> {
    ensure!(now.is_finite(), "nonfinite clock outside bridge scope");
    let clock = world.store.component_bytes(entity, types.clock)?;
    ensure!(clock.len() == 8, "clock size");
    let next = f64::from_le_bytes(clock.try_into()?);
    let active = world.store.component_bytes(entity, types.active)?.to_vec();
    ensure!(active.len() == 12, "active child size");
    let index = i32::from_le_bytes(active[..4].try_into()?);
    if index == -1 || next == -1. {
        return Ok(AutoEventOutcome::Skipped);
    }
    ensure!(next.is_finite(), "nonfinite deadline outside bridge scope");
    let element = chain
        .elements
        .get(usize::try_from(index)?)
        .context("invalid active element")?;
    let set_clock = |world: &mut World, value: f64| {
        world
            .store
            .set_component_bytes(entity, types.clock, &value.to_le_bytes())
    };
    if !element.use_auto_transition {
        set_clock(world, -1.)?;
        return Ok(AutoEventOutcome::Disabled);
    }
    if next == 0. {
        set_clock(world, now + f64::from(element.auto_transition.duration))?;
        return Ok(AutoEventOutcome::Initialized);
    }
    if now <= next {
        return Ok(AutoEventOutcome::Skipped);
    }
    let instance = world.store.component_bytes(entity, types.instance)?;
    ensure!(instance.len() == 1, "chain instance size");
    let loop_on_end = instance[0] != 0;
    let mut elements = Vec::with_capacity(chain.elements.len());
    for element in &chain.elements {
        let transition = if element.use_auto_transition {
            Some(
                element
                    .transitions
                    .get(usize::try_from(element.auto_transition.transition_index)?)
                    .context("invalid auto transition")?,
            )
        } else {
            None
        };
        elements.push(AutoProgressElement {
            enabled: element.use_auto_transition,
            duration: element.auto_transition.duration,
            target: transition.map_or(-1, |t| t.target),
            end: transition.is_some_and(|t| t.end_of_chain),
            event: element.auto_transition.transition_id,
        });
    }
    let result = progress(
        &elements,
        chain.main_index,
        index,
        loop_on_end,
        false,
        true,
        now - next,
        256,
    )?;
    if result.success
        && usize::try_from(result.target)
            .ok()
            .and_then(|i| chain.elements.get(i))
            .is_some_and(|e| e.use_auto_transition)
    {
        return Ok(AutoEventOutcome::UnsupportedNormalization);
    }
    let outcome = if result.success {
        let mut event = [0u8; 96];
        event[..32].copy_from_slice(&result.event);
        event[64..68].copy_from_slice(&entity.index.to_le_bytes());
        event[68..72].copy_from_slice(&entity.version.to_le_bytes());
        event[72..80].copy_from_slice(&active[4..12]);
        event[80..84].copy_from_slice(&index.to_le_bytes());
        event[84..88].copy_from_slice(&result.origin.to_le_bytes());
        event[88..].copy_from_slice(&result.forwarded.to_le_bytes());
        let archetype = world
            .store
            .get_or_create_archetype(&world.types, &[types.event])?;
        let queued = world.store.create_entity(archetype)?;
        world
            .store
            .set_component_bytes(queued, types.event, &event)?;
        AutoEventOutcome::Queued(queued)
    } else {
        AutoEventOutcome::Retry
    };
    set_clock(
        world,
        now + f64::from(element.auto_transition.attempt_cooldown),
    )?;
    Ok(outcome)
}
