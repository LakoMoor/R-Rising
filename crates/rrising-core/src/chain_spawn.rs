//! Spawn-wrapper transforms; original prefab cloning and deferred scheduling have separate boundaries.
use rrising_math::{float3, float4x4, quaternion};
pub struct SpawnPose {
    pub translation: [u8; 12],
    pub rotation: [u8; 16],
    pub local_transform: [u8; 32],
    pub local_to_world: [u8; 64],
}
fn packed<const N: usize, const B: usize>(values: [f32; N]) -> [u8; B] {
    assert_eq!(B, N * 4);
    let mut out = [0; B];
    for (slot, value) in out.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        *slot = value.to_le_bytes();
    }
    out
}
impl SpawnPose {
    /// Explicit translation/rotation supplied by the caller, like SpawnChainPrefabInstance.
    /// Does not derive a rotation from the selected candidate's matrix or normalize it.
    pub fn new(position: [f32; 3], rotation: [f32; 4]) -> Self {
        let matrix = float4x4::trs(
            float3::new(position[0], position[1], position[2]),
            quaternion::new(rotation[0], rotation[1], rotation[2], rotation[3]),
            float3::new(1., 1., 1.),
        );
        let columns = [matrix.c0, matrix.c1, matrix.c2, matrix.c3];
        let flat = std::array::from_fn::<_, 16, _>(|i| {
            let c = columns[i / 4];
            [c.x, c.y, c.z, c.w][i % 4]
        });
        Self {
            translation: packed(position),
            rotation: packed(rotation),
            local_transform: packed([
                position[0],
                position[1],
                position[2],
                1.,
                rotation[0],
                rotation[1],
                rotation[2],
                rotation[3],
            ]),
            local_to_world: packed(flat),
        }
    }
}

#[cfg(feature = "content")]
pub mod content {
    use super::SpawnPose;
    use crate::{
        chain_attachment::{apply_existing, required_root_components, AttachmentTypes},
        transition_selection::PrefabCandidate,
    };
    use anyhow::{ensure, Context, Result};
    use rrising_content::{
        prefab_catalog::{LinkedPrefabTemplate, PrefabMemberKey},
        type_layouts::TypeLayouts,
    };
    use rrising_ecs::{Entity, TypeIndex, World};
    pub struct PoseTypes {
        pub translation: TypeIndex,
        pub rotation: TypeIndex,
        pub local_transform: TypeIndex,
        pub local_to_world: TypeIndex,
    }
    pub struct SpawnRequest<'a> {
        pub chain: Entity,
        pub candidate: &'a PrefabCandidate,
        pub template: &'a LinkedPrefabTemplate,
        pub layouts: &'a TypeLayouts,
        pub attachment_types: &'a AttachmentTypes,
        pub pose_types: &'a PoseTypes,
        pub position: [f32; 3],
        pub rotation: [f32; 4],
        pub now: f64,
    }
    pub struct SpawnedChain {
        pub members: Vec<Entity>,
        pub changed_event: Entity,
    }
    /// Clone the existing admitted prefab projection and execute measured root writes.
    /// Deferred ECB behavior, activation, placement and old-child disposal remain absent.
    pub fn spawn_linked(
        world: &mut World,
        request: &SpawnRequest<'_>,
        resolve: impl FnMut(PrefabMemberKey) -> Option<Entity>,
    ) -> Result<SpawnedChain> {
        let attachment = request.attachment_types;
        ensure!(
            world
                .store
                .component_bytes(request.chain, attachment.active)?
                .len()
                == 12,
            "active size"
        );
        if request.candidate.auto_transition.is_some() {
            ensure!(
                world
                    .store
                    .component_bytes(request.chain, attachment.clock)?
                    .len()
                    == 8,
                "clock slot must be prepared"
            );
        }
        let root = request
            .template
            .members
            .first()
            .context("prefab template has no root")?;
        // Original calls SetComponent, not AddComponent, for these transform slots.
        for (ty, size) in [
            (request.pose_types.translation, 12),
            (request.pose_types.rotation, 16),
            (request.pose_types.local_transform, 32),
            (request.pose_types.local_to_world, 64),
        ] {
            let info = world
                .types
                .type_info(ty)
                .context("pose type is not registered")?;
            ensure!(info.size_in_chunk == size, "pose type size");
            ensure!(
                root.iter()
                    .any(|value| value.stable_hash == info.stable_type_hash
                        && !value.is_buffer
                        && value.bytes.len() == size as usize),
                "prefab projection lacks required transform component"
            );
        }
        let extra =
            required_root_components(world, request.chain, request.candidate.prefab, attachment)?;
        let members = request.template.instantiate_prepared(
            &mut world.store,
            &mut world.types,
            request.layouts,
            &extra,
            resolve,
        )?;
        let child = *members.first().context("empty cloned prefab")?;
        let pose = SpawnPose::new(request.position, request.rotation);
        let result = (|| -> Result<Entity> {
            for (ty, bytes) in [
                (request.pose_types.translation, pose.translation.as_slice()),
                (request.pose_types.rotation, pose.rotation.as_slice()),
                (
                    request.pose_types.local_transform,
                    pose.local_transform.as_slice(),
                ),
                (
                    request.pose_types.local_to_world,
                    pose.local_to_world.as_slice(),
                ),
            ] {
                world.store.set_component_bytes(child, ty, bytes)?;
            }
            apply_existing(
                world,
                request.chain,
                child,
                request.candidate,
                attachment,
                request.now,
            )
        })();
        match result {
            Ok(changed_event) => Ok(SpawnedChain {
                members,
                changed_event,
            }),
            Err(error) => {
                world
                    .store
                    .destroy_entities(&members)
                    .context("cleaning failed prefab projection")?;
                Err(error)
            }
        }
    }
}
