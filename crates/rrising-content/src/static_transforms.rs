use std::{collections::BTreeMap, path::Path};

use anyhow::{ensure, Context, Result};
use rrising_ecs::Hash128;
use rrising_math::{float3, float4x4, quaternion};

use crate::{entity_scenes::SceneFile, type_layouts::TypeLayouts};

const TRANSFORM: u64 = 0x568b_c071_6c2b_266f;
const TERRAIN_METADATA: u64 = 0x047d_12ed_8a2c_93f7;
const ELEMENT_SIZE: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakedStaticTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Debug, Default)]
pub struct StaticTransformCatalog {
    chunks: BTreeMap<u16, TerrainTransformSource>,
    templates: BTreeMap<Hash128, Vec<BakedStaticTransform>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainTransformSource {
    pub scene: Hash128,
    pub rotation_degrees: i32,
}

impl StaticTransformCatalog {
    /// Reads original section buffers by their packed TerrainChunk coordinates.
    pub fn read_server(data: &Path, layouts: &TypeLayouts) -> Result<Self> {
        let mut catalog = Self::default();
        for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "entities") {
                continue;
            }
            let scene = SceneFile::open(&path)?;
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .context("invalid server scene filename")?;
            let guid =
                Hash128::from_guid(filename.split('.').next().context("missing scene GUID")?);
            ensure!(guid.is_valid(), "invalid scene GUID");
            catalog
                .read_scene(guid, &scene, layouts)
                .with_context(|| format!("reading static transforms from {}", path.display()))?;
        }
        Ok(catalog)
    }

    pub fn read_scene(
        &mut self,
        guid: Hash128,
        scene: &SceneFile,
        layouts: &TypeLayouts,
    ) -> Result<()> {
        let archetypes = scene.archetypes()?;
        if !archetypes
            .unique_type_hashes
            .iter()
            .any(|hash| [TRANSFORM, TERRAIN_METADATA].contains(hash))
        {
            return Ok(());
        }
        let resolved = layouts.resolve_scene(scene)?;
        ensure!(
            resolved.summary.unresolved_archetypes.is_empty(),
            "unresolved terrain archetype"
        );
        let buffers = scene.buffer_data()?;
        for (chunk_index, chunk) in resolved.chunks.iter().enumerate() {
            for row in 0..usize::try_from(chunk.header.count)? {
                if let Some(metadata) =
                    resolved.component_bytes(chunk_index, row, TERRAIN_METADATA)?
                {
                    ensure!(metadata.len() == 160, "unexpected terrain metadata size");
                    let coordinate = u16::from_le_bytes(metadata[..2].try_into()?);
                    let source = TerrainTransformSource {
                        scene: Hash128::from_bytes(metadata[4..20].try_into()?),
                        rotation_degrees: i32::from_le_bytes(metadata[20..24].try_into()?),
                    };
                    if source.scene.is_valid() {
                        let previous = self.chunks.insert(coordinate, source);
                        ensure!(previous.is_none_or(|previous| previous == source),
                            "conflicting terrain metadata at {coordinate:#06x}: {previous:?} vs {source:?}");
                    }
                }
                let Some(data) =
                    resolved.buffer_payload(&buffers, chunk_index, row, TRANSFORM, ELEMENT_SIZE)?
                else {
                    continue;
                };
                let transforms = data
                    .as_chunks::<ELEMENT_SIZE>()
                    .0
                    .iter()
                    .map(|element| {
                        let values = element.as_chunks::<4>().0;
                        let floats: [f32; 7] =
                            std::array::from_fn(|index| f32::from_le_bytes(values[index]));
                        BakedStaticTransform {
                            position: [floats[0], floats[1], floats[2]],
                            rotation: [floats[3], floats[4], floats[5], floats[6]],
                        }
                    })
                    .collect();
                ensure!(
                    self.templates.insert(guid, transforms).is_none(),
                    "duplicate static transform buffer for {guid:?}"
                );
            }
        }
        Ok(())
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn transform_count(&self) -> usize {
        self.templates.values().map(Vec::len).sum()
    }

    pub fn source(&self, terrain_chunk: u16) -> Option<TerrainTransformSource> {
        self.chunks.get(&terrain_chunk).copied()
    }

    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// Transform a terrain section's local coordinates into world coordinates.
    /// Uses the same placement as the recovered static-object restore path.
    pub fn placement_matrix(&self, terrain_chunk: u16) -> Option<float4x4> {
        let source = self.source(terrain_chunk)?;
        let [x, z] = terrain_chunk
            .to_le_bytes()
            .map(|axis| f32::from(axis as i8));
        let origin = float3::new(x * 160.0 - 3200.0, 0.0, z * 160.0 - 3200.0);
        let center = float3::new(80.0, 0.0, 80.0);
        let yaw = (source.rotation_degrees as f32 / 180.0) * std::f32::consts::PI;
        Some(float4x4::mul_matrix(
            float4x4::translate(origin),
            float4x4::mul_matrix(
                float4x4::translate(center),
                float4x4::mul_matrix(
                    float4x4::rotate(quaternion::rotate_y(yaw)),
                    float4x4::translate(-center),
                ),
            ),
        ))
    }

    /// Reads the baked section-local value. Terrain placement is a separate operation.
    pub fn get(&self, terrain_chunk: u16, transform_index: u16) -> Option<&BakedStaticTransform> {
        let index = usize::try_from(transform_index as i16).ok()?;
        self.templates
            .get(&self.chunks.get(&terrain_chunk)?.scene)?
            .get(index)
    }

    /// Applies the terrain load transform before selecting the runtime position and rotation.
    pub fn placed(&self, terrain_chunk: u16, transform_index: u16) -> Option<BakedStaticTransform> {
        let baked = self.get(terrain_chunk, transform_index)?;
        let [px, py, pz] = baked.position;
        let [qx, qy, qz, qw] = baked.rotation;
        let terrain = self.placement_matrix(terrain_chunk)?;
        let world = float4x4::mul_matrix(
            terrain,
            float4x4::trs(
                float3::new(px, py, pz),
                quaternion::new(qx, qy, qz, qw),
                float3::new(1.0, 1.0, 1.0),
            ),
        );
        let orientation = quaternion::look_rotation_safe(world.c2.xyz(), world.c1.xyz()).value;
        Some(BakedStaticTransform {
            position: [world.c3.x, world.c3.y, world.c3.z],
            rotation: [orientation.x, orientation.y, orientation.z, orientation.w],
        })
    }
}
