//! Transition-event key and bounded queue bridge; event lifecycle/processing remain caller-owned.
use anyhow::{ensure, Result};
use rrising_ecs::{Entity, TypeIndex, World};
pub(crate) fn id_equal(a: &[u8], b: &[u8]) -> Result<bool> {
    let n = usize::from(u16::from_le_bytes(a[..2].try_into()?));
    let m = usize::from(u16::from_le_bytes(b[..2].try_into()?));
    ensure!(n <= 29 && m <= 29, "invalid FixedString32 length");
    Ok(n == m && a[2..2 + n] == b[2..2 + m])
}
/// Forwarded time and unused FixedString padding are excluded by original event equality.
pub fn same_key(a: &[u8; 96], b: &[u8; 96]) -> Result<bool> {
    Ok(
        id_equal(&a[..32], &b[..32])?
            && id_equal(&a[32..64], &b[32..64])?
            && a[64..88] == b[64..88],
    )
}
pub struct TransitionQueueTypes {
    pub event: TypeIndex,
    pub active: TypeIndex,
    pub queued: TypeIndex,
    pub disabled: Option<TypeIndex>,
}
#[derive(Debug, PartialEq)]
pub enum EnqueueResult {
    Rejected,
    Accepted {
        chain: Entity,
        replaced: bool,
        ready: bool,
    },
}
/// Explicitly supplied event; does not destroy its entity or choose scheduling order.
pub fn enqueue(
    world: &mut World,
    event_entity: Entity,
    types: &TransitionQueueTypes,
) -> Result<EnqueueResult> {
    let event: [u8; 96] = world
        .store
        .component_bytes(event_entity, types.event)?
        .try_into()?;
    let chain = Entity::new(
        i32::from_le_bytes(event[64..68].try_into()?),
        i32::from_le_bytes(event[68..72].try_into()?),
    );
    if !world.store.exists(chain)? {
        return Ok(EnqueueResult::Rejected);
    }
    let Some(active) = world.store.try_component_bytes(chain, types.active)? else {
        return Ok(EnqueueResult::Rejected);
    };
    ensure!(active.len() == 12, "invalid active child size");
    if active[..4] != event[80..84] || active[4..12] != event[72..80] {
        return Ok(EnqueueResult::Rejected);
    }
    let mut payload = world
        .store
        .buffer_payload(&world.types, chain, types.queued)?
        .to_vec();
    ensure!(
        payload.len().is_multiple_of(96),
        "partial transition request"
    );
    let mut replace = None;
    for (index, queued) in payload.as_chunks::<96>().0.iter().enumerate() {
        if same_key(queued, &event)? {
            replace = Some(index);
            break;
        }
    }
    if let Some(index) = replace {
        payload[index * 96..index * 96 + 96].copy_from_slice(&event);
    } else {
        payload.extend_from_slice(&event);
    }
    let ready = match types.disabled {
        Some(ty) => world.store.try_component_bytes(chain, ty)?.is_none(),
        None => true,
    };
    world
        .store
        .set_buffer_payload(&world.types, chain, types.queued, &payload)?;
    Ok(EnqueueResult::Accepted {
        chain,
        replaced: replace.is_some(),
        ready,
    })
}
