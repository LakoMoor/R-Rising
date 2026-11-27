//! Prefab candidate selection, measured against the pinned isolated native function.
//! Placement, secondary-ID retry, consumption and active-child replacement remain caller-owned.
use crate::{auto_chain::forwarded_remainder, transition_queue::id_equal};
use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex, World};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoConfig {
    pub transition_id: [u8; 32],
    pub transition_index: i32,
    pub attempt_cooldown: f32,
    pub duration: f32,
}
pub struct Element {
    pub prefab_guid: i32,
    pub auto_transition: Option<AutoConfig>,
}
pub struct Transition {
    pub id: [u8; 32],
    pub target: i32,
    pub importance: i32,
    pub end_of_chain: bool,
}
/// Shared native/web boundary; no archive reader or dynamic-library loader is required.
pub trait ChainSource {
    fn main_index(&self) -> i32;
    fn transition_count(&self, element: i32) -> Option<usize>;
    fn transition(&self, element: i32, index: usize) -> Option<Transition>;
    fn element(&self, index: i32) -> Option<Element>;
}
#[cfg(feature = "content")]
impl From<&rrising_content::spawn_chain::AutoTransitionConfig> for AutoConfig {
    fn from(value: &rrising_content::spawn_chain::AutoTransitionConfig) -> Self {
        Self {
            transition_id: value.transition_id,
            transition_index: value.transition_index,
            attempt_cooldown: value.attempt_cooldown,
            duration: value.duration,
        }
    }
}
#[cfg(feature = "content")]
impl ChainSource for rrising_content::spawn_chain::ChainDefinition {
    fn main_index(&self) -> i32 {
        self.main_index
    }
    fn transition_count(&self, element: i32) -> Option<usize> {
        Some(
            self.elements
                .get(usize::try_from(element).ok()?)?
                .transitions
                .len(),
        )
    }
    fn transition(&self, element: i32, index: usize) -> Option<Transition> {
        let source = self
            .elements
            .get(usize::try_from(element).ok()?)?
            .transitions
            .get(index)?;
        Some(Transition {
            id: source.id,
            target: source.target,
            importance: source.importance,
            end_of_chain: source.end_of_chain,
        })
    }
    fn element(&self, index: i32) -> Option<Element> {
        let source = self.elements.get(usize::try_from(index).ok()?)?;
        Some(Element {
            prefab_guid: source.prefab_guid,
            auto_transition: source
                .use_auto_transition
                .then(|| AutoConfig::from(&source.auto_transition)),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrefabCandidate {
    pub element_index: i32,
    pub local_to_world: [f32; 16],
    pub prefab: Entity,
    pub destroy_chain_on_end: bool,
    pub forwarded_seconds: f64,
    pub auto_transition: Option<AutoConfig>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub importance: i32,
    pub candidate: Option<PrefabCandidate>,
}
pub struct SelectionRequest {
    pub transition_id: [u8; 32],
    pub start_element: i32,
    pub loop_on_end: bool,
    pub forwarded_seconds: f64,
    pub local_to_world: [f32; 16],
    pub last_failed_importance: Option<i32>,
}
/// Equal-priority candidates replace earlier ones. Failed priorities form an exclusive ceiling.
/// Missing prefab logging/error behavior and invalid original blob accesses are not emulated.
pub fn select(
    chain: &impl ChainSource,
    request: &SelectionRequest,
    mut resolve_prefab: impl FnMut(i32) -> Option<Entity>,
) -> Result<Selection> {
    let mut result = Selection {
        importance: i32::MIN,
        candidate: None,
    };
    if id_equal(&request.transition_id, &[0; 32])? {
        return Ok(result);
    }
    let count = chain
        .transition_count(request.start_element)
        .context("invalid transition start element")?;
    for index in 0..count {
        let transition = chain
            .transition(request.start_element, index)
            .context("missing transition entry")?;
        if !id_equal(&request.transition_id, &transition.id)?
            || request
                .last_failed_importance
                .is_some_and(|ceiling| transition.importance >= ceiling)
            || transition.importance < result.importance
        {
            continue;
        }
        let target = chain
            .element(transition.target)
            .context("invalid transition target element")?;
        let prefab = resolve_prefab(target.prefab_guid)
            .context("missing prefab: original logging/error path is not implemented")?;
        let mut forwarded = request.forwarded_seconds;
        if transition.target == chain.main_index() {
            if let Some(config) = target.auto_transition.filter(|config| config.duration > 0.) {
                forwarded = forwarded_remainder(forwarded, f64::from(config.duration))?;
            }
        }
        result.importance = transition.importance;
        result.candidate = Some(PrefabCandidate {
            element_index: transition.target,
            local_to_world: request.local_to_world,
            prefab,
            destroy_chain_on_end: !request.loop_on_end && transition.end_of_chain,
            forwarded_seconds: forwarded,
            auto_transition: target.auto_transition,
        });
    }
    Ok(result)
}

pub struct SelectionTypes {
    pub queued: TypeIndex,
    pub instance: TypeIndex,
    pub local_to_world: TypeIndex,
}
/// Select an explicitly indexed queued request's primary ID through actual ECS buffer storage.
/// Does not remove the request, test placement, retry the secondary ID or mutate the active child.
pub fn select_queued_primary(
    world: &World,
    chain_entity: Entity,
    queued_index: usize,
    types: &SelectionTypes,
    definition: &impl ChainSource,
    last_failed_importance: Option<i32>,
    resolve_prefab: impl FnMut(i32) -> Option<Entity>,
) -> Result<Selection> {
    let payload = world
        .store
        .buffer_payload(&world.types, chain_entity, types.queued)?;
    ensure!(payload.len().is_multiple_of(96), "partial queued request");
    let event = payload
        .as_chunks::<96>()
        .0
        .get(queued_index)
        .context("queued request index out of bounds")?;
    let instance = world.store.component_bytes(chain_entity, types.instance)?;
    ensure!(
        instance.len() == 1 && instance[0] <= 1,
        "invalid chain instance"
    );
    let transform = world
        .store
        .component_bytes(chain_entity, types.local_to_world)?;
    let transform: &[u8; 64] = transform.try_into()?;
    let mut matrix = [0.; 16];
    for (output, bytes) in matrix.iter_mut().zip(transform.as_chunks::<4>().0) {
        *output = f32::from_le_bytes(*bytes);
    }
    select(
        definition,
        &SelectionRequest {
            transition_id: event[..32].try_into()?,
            start_element: i32::from_le_bytes(event[84..88].try_into()?),
            loop_on_end: instance[0] != 0,
            forwarded_seconds: f64::from_le_bytes(event[88..96].try_into()?),
            local_to_world: matrix,
            last_failed_importance,
        },
        resolve_prefab,
    )
}
