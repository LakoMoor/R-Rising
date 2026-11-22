//! `Unity.Transforms` composition, ported on plain `f32` arrays.
//!
//! Component values stay raw serialized floats (matrices column-major as `c0..c3`), so a caller
//! holding scene bytes needs no math types; the arithmetic goes through `rrising-math`.
//!
//! Provenance: the corpus under `evidence/decompiled` has no `Unity.Transforms` module, so the
//! system-level composition is not translated from a decompiled body. The leaf math it calls
//! (`float4x4.TRS`, `float3x3(quaternion)`, `mul(float4x4, float4x4)`, `mul(quaternion, quaternion)`)
//! is bit-identical to the original's, replayed under Rosetta (`tools/capture_math_matrix.py`). The formulas are the Unity Entities `LocalTransform` / `Translation` / `Rotation` /
//! `PostTransformMatrix` / `Parent` semantics, selected by *measuring bitwise agreement with the
//! `LocalToWorld` values the original stored in its scene files*
//! (`crates/rrising-content/tests/transform_golden.rs`, `docs/COMPATIBILITY.md`). Where a variant was
//! measured and rejected, the module says so rather than keeping both.
use anyhow::{ensure, Result};
use rrising_math::{float3, float4, float4x4, quaternion};

use crate::chunk::ChunkIndex;
use crate::entity::Entity;
use crate::schedule::{FrameContext, System};
use crate::store::EntityQuery;
use crate::type_table::TypeIndex;
use crate::world::World;

/// A column-major 4x4 as serialized (`c0.xyzw, c1.xyzw, c2.xyzw, c3.xyzw`).
pub type Mat = [f32; 16];

/// `Unity.Transforms.LocalTransform` (Position @0, Scale @12, Rotation @16; 32 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalTransform {
    pub position: [f32; 3],
    pub scale: f32,
    pub rotation: [f32; 4],
}

pub fn to_mat(m: float4x4) -> Mat {
    [
        m.c0.x, m.c0.y, m.c0.z, m.c0.w, m.c1.x, m.c1.y, m.c1.z, m.c1.w, m.c2.x, m.c2.y, m.c2.z,
        m.c2.w, m.c3.x, m.c3.y, m.c3.z, m.c3.w,
    ]
}

pub fn from_mat(m: &Mat) -> float4x4 {
    float4x4::new(
        float4::new(m[0], m[1], m[2], m[3]),
        float4::new(m[4], m[5], m[6], m[7]),
        float4::new(m[8], m[9], m[10], m[11]),
        float4::new(m[12], m[13], m[14], m[15]),
    )
}

/// `math.mul(float4x4, float4x4)`.
pub fn mul(a: &Mat, b: &Mat) -> Mat {
    to_mat(float4x4::mul_matrix(from_mat(a), from_mat(b)))
}

/// `LocalTransform.ToMatrix()` = `float4x4.TRS(Position, Rotation, float3(Scale))`.
///
/// `rrising_math::float4x4::trs` is bit-identical to the original's `float4x4.TRS` (and its
/// `float3x3(quaternion)` constructor) replayed from the pinned binary on 2,048 inputs
/// (`crates/rrising-math/tests/oracle_matrix.rs`), including the `+0.0` the constructor writes where
/// a plain product form leaves `-0.0`. Against stored scene `LocalToWorld` this form matches
/// bit for bit for 58,021 of 58,597 (99.0%) root `LocalTransform+Translation+Rotation` entities;
/// the remainder is discussed in `docs/COMPATIBILITY.md`.
pub fn local_transform_matrix(lt: &LocalTransform) -> Mat {
    to_mat(float4x4::trs(
        float3::new(lt.position[0], lt.position[1], lt.position[2]),
        quaternion::new(
            lt.rotation[0],
            lt.rotation[1],
            lt.rotation[2],
            lt.rotation[3],
        ),
        float3::new(lt.scale, lt.scale, lt.scale),
    ))
}

/// Translation / Rotation / uniform scale: the original's `float4x4.TRS` (see
/// [`local_transform_matrix`]). Earlier revisions composed `Translate * (Rotate * Scale)` to
/// reproduce a stored `+0.0`; with the constructor's real operation order that workaround is
/// unnecessary and `TRS` matches the stored values better (58,021 vs 56,159 of 58,597 bit-identical on
/// those roots).
pub fn trs_matrix(t: [f32; 3], r: [f32; 4], s: f32) -> Mat {
    local_transform_matrix(&LocalTransform {
        position: t,
        scale: s,
        rotation: r,
    })
}

/// `PostTransformMatrix`: `mul(local, post)`. `mul` is bit-identical to the original's
/// `math.mul(float4x4, float4x4)` (replayed). Measured against stored matrices this order is the
/// right one (the reverse order is far worse) but only ~27% of entities are bit-exact and ~98.6%
/// are within 1e-5; stored matrices keep the local translation exactly where the product adds the
/// post matrix's tiny translation terms, and even an identity post does not reproduce the stored
/// rotation bits for ~59% of such entities. The original's combination is not recovered.
pub fn post_transform(local: &Mat, post: &Mat) -> Mat {
    mul(local, post)
}

/// Child world matrix: `mul(parent_world, local)`.
pub fn child_to_world(parent_world: &Mat, local: &Mat) -> Mat {
    mul(parent_world, local)
}

/// The authoring components of one entity (raw serialized values), before composition.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Authoring {
    pub lt: Option<LocalTransform>,
    pub translation: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<f32>,
    pub post: Option<Mat>,
}

fn f32s<const N: usize>(b: &[u8]) -> [f32; N] {
    let mut r = [0f32; N];
    for (v, c) in r.iter_mut().zip(b.as_chunks::<4>().0) {
        *v = f32::from_le_bytes(*c);
    }
    r
}

fn mat_bytes(m: &Mat) -> [u8; 64] {
    let mut out = [0u8; 64];
    for (o, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(m) {
        o.copy_from_slice(&v.to_le_bytes());
    }
    out
}

impl Authoring {
    /// Source rule measured against stored scene data (module docs, `docs/COMPATIBILITY.md`):
    /// `Translation`/`Rotation` replace the corresponding `LocalTransform` fields when present;
    /// absent fields retain their existing local values. `Scale` overrides the local scale; then
    /// `PostTransformMatrix`. `None` when there is no local transform at all.
    pub fn local_matrix(&self) -> Option<Mat> {
        let m = if self.translation.is_some() || self.rotation.is_some() {
            let scale = self.scale.or(self.lt.map(|l| l.scale)).unwrap_or(1.0);
            // ProjectM.UpdateLocalTransformJob (client RVA 0x77651f0) updates only the
            // fields for which Translation/Rotation are present, preserving the other field.
            trs_matrix(
                self.translation
                    .or(self.lt.map(|l| l.position))
                    .unwrap_or([0.0; 3]),
                self.rotation
                    .or(self.lt.map(|l| l.rotation))
                    .unwrap_or([0.0, 0.0, 0.0, 1.0]),
                scale,
            )
        } else if let Some(lt) = &self.lt {
            local_transform_matrix(lt)
        } else {
            return None;
        };
        Some(match &self.post {
            Some(p) => post_transform(&m, p),
            None => m,
        })
    }
}

/// Component types the transform system reads/writes. Optional ones may be absent from the world.
#[derive(Clone, Copy, Debug)]
pub struct TransformTypes {
    /// `Unity.Transforms.LocalToWorld` (64 bytes), the output.
    pub local_to_world: TypeIndex,
    /// `LocalTransform` (32 bytes).
    pub local_transform: Option<TypeIndex>,
    /// `Translation` (12 bytes).
    pub translation: Option<TypeIndex>,
    /// `Rotation` (16 bytes).
    pub rotation: Option<TypeIndex>,
    /// Uniform `Scale` (4 bytes).
    pub scale: Option<TypeIndex>,
    /// `PostTransformMatrix` (64 bytes).
    pub post_transform: Option<TypeIndex>,
    /// `Parent` (8 bytes, `Value` entity first).
    pub parent: Option<TypeIndex>,
}

/// What the last [`TransformSystem`] update did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransformStats {
    pub root_chunks_written: usize,
    pub root_chunks_skipped: usize,
    pub root_entities_written: usize,
    pub children_written: usize,
    pub children_skipped: usize,
    /// Children left untouched: parent not live, no matrix to inherit, or chain deeper than 64
    /// (or cyclic).
    pub children_unresolved: usize,
}

impl TransformStats {
    pub fn entities_written(&self) -> usize {
        self.root_entities_written + self.children_written
    }
}

/// Computes `LocalToWorld` from authoring components and the `Parent` hierarchy, parents before
/// children, skipping chunks whose inputs did not change since the system last ran.
///
/// Roots (no `Parent`) are handled chunk-wise: if no input component's chunk change version is
/// newer than the system's last version the chunk is skipped. Children (with `Parent`) are ordered
/// by hierarchy depth (ties by query order, so the order is deterministic) and recomputed when
/// their own inputs changed or the parent's `LocalToWorld` chunk is newer than the last run (which
/// includes parents rewritten earlier in this very update; the check is per chunk, so it is
/// conservative). A parent that has no authoring source of its own is not recomputed: its current
/// `LocalToWorld` bytes in the world are inherited, which for scene data are the stored ones.
/// Entities with a `Parent` but no authoring source keep their `LocalToWorld` untouched.
///
/// Not a translation of the original system (no body exists in the corpus): see the module docs.
#[derive(Debug)]
pub struct TransformSystem {
    types: TransformTypes,
    stats: TransformStats,
}

const MAX_DEPTH: u32 = 64;
const UNSET: u32 = u32::MAX;
const BAD: u32 = u32::MAX - 1;

struct Child {
    entity: Entity,
    parent: Entity,
    chunk: ChunkIndex,
    slot: usize,
}

impl TransformSystem {
    pub const NAME: &'static str = "TransformSystem";

    pub fn new(types: TransformTypes) -> Self {
        Self {
            types,
            stats: TransformStats::default(),
        }
    }

    /// Stats of the most recent update.
    pub fn stats(&self) -> TransformStats {
        self.stats
    }

    fn inputs(&self) -> Vec<TypeIndex> {
        let t = &self.types;
        [
            t.local_transform,
            t.translation,
            t.rotation,
            t.scale,
            t.post_transform,
            t.parent,
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn has_source(&self, w: &World, c: ChunkIndex) -> bool {
        let t = &self.types;
        [t.local_transform, t.translation, t.rotation]
            .into_iter()
            .flatten()
            .any(|ty| w.store.chunk_has_component(c, ty))
    }

    fn chunk_inputs_changed(&self, w: &World, c: ChunkIndex, ctx: &FrameContext) -> Result<bool> {
        for ty in self.inputs() {
            if w.store.chunk_has_component(c, ty)
                && ctx.did_change(w.store.chunk_change_version(c, ty)?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Authoring of slot `i` from per-chunk (or per-entity, `i == 0`) byte arrays.
    fn authoring_at(&self, w: &World, c: ChunkIndex, i: usize, count: usize) -> Result<Authoring> {
        let t = &self.types;
        let get = |ty: Option<TypeIndex>, size: usize| -> Result<Option<&[u8]>> {
            let Some(ty) = ty else { return Ok(None) };
            if !w.store.chunk_has_component(c, ty) {
                return Ok(None);
            }
            let b = w.store.chunk_component_bytes(c, ty)?;
            ensure!(
                b.len() == size * count,
                "component {:?} is {} bytes for {count} entities, expected stride {size}",
                ty,
                b.len()
            );
            Ok(Some(&b[i * size..(i + 1) * size]))
        };
        Ok(Authoring {
            lt: get(t.local_transform, 32)?.map(|b| {
                let v = f32s::<8>(b);
                LocalTransform {
                    position: [v[0], v[1], v[2]],
                    scale: v[3],
                    rotation: [v[4], v[5], v[6], v[7]],
                }
            }),
            translation: get(t.translation, 12)?.map(f32s::<3>),
            rotation: get(t.rotation, 16)?.map(f32s::<4>),
            scale: get(t.scale, 4)?.map(|b| f32s::<1>(b)[0]),
            post: get(t.post_transform, 64)?.map(f32s::<16>),
        })
    }

    fn update_roots(&mut self, w: &mut World, ctx: &FrameContext) -> Result<()> {
        let ltw = self.types.local_to_world;
        let q = EntityQuery {
            all: vec![ltw],
            none: self.types.parent.into_iter().collect(),
            include_disabled: true,
            include_prefab: true,
        };
        for c in w.store.query_chunks(&q) {
            if !self.has_source(w, c) {
                continue;
            }
            if !self.chunk_inputs_changed(w, c, ctx)? {
                self.stats.root_chunks_skipped += 1;
                continue;
            }
            let count = w.store.chunk(c).map_or(0, |ch| ch.count as usize);
            let mut mats: Vec<Option<Mat>> = Vec::with_capacity(count);
            for i in 0..count {
                mats.push(self.authoring_at(w, c, i, count)?.local_matrix());
            }
            let out = w.store.chunk_component_bytes_mut(c, ltw)?;
            ensure!(out.len() == 64 * count, "LocalToWorld is not 64 bytes");
            for (i, m) in mats.iter().enumerate() {
                if let Some(m) = m {
                    out[i * 64..(i + 1) * 64].copy_from_slice(&mat_bytes(m));
                    self.stats.root_entities_written += 1;
                }
            }
            self.stats.root_chunks_written += 1;
        }
        Ok(())
    }

    fn update_children(&mut self, w: &mut World, ctx: &FrameContext) -> Result<()> {
        let (ltw, Some(parent_ty)) = (self.types.local_to_world, self.types.parent) else {
            return Ok(());
        };
        let q = EntityQuery {
            all: vec![ltw, parent_ty],
            none: Vec::new(),
            include_disabled: true,
            include_prefab: true,
        };
        let mut kids: Vec<Child> = Vec::new();
        for c in w.store.query_chunks(&q) {
            if !self.has_source(w, c) {
                continue;
            }
            let ents = w.store.chunk_entities(c)?;
            let pb = w.store.chunk_component_bytes(c, parent_ty)?;
            ensure!(pb.len() == 8 * ents.len(), "Parent is not 8 bytes");
            for (slot, (e, b)) in ents.iter().zip(pb.as_chunks::<8>().0).enumerate() {
                kids.push(Child {
                    entity: *e,
                    parent: Entity::new(
                        i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    ),
                    chunk: c,
                    slot,
                });
            }
        }
        // entity index -> position in `kids` (dense table, no hash iteration)
        let cap = usize::try_from(w.store.capacity()).unwrap_or(0);
        let mut pos = vec![UNSET; cap.max(1)];
        for (i, k) in kids.iter().enumerate() {
            if let Some(p) = pos.get_mut(k.entity.index as usize) {
                *p = i as u32;
            }
        }
        let in_list = |p: Entity| -> Option<usize> {
            let i = *pos.get(usize::try_from(p.index).ok()?)?;
            (i != UNSET && kids[i as usize].entity == p).then_some(i as usize)
        };
        // depth among listed ancestors; BAD = unresolvable/cyclic/too deep
        let mut depth = vec![UNSET; kids.len()];
        for start in 0..kids.len() {
            if depth[start] != UNSET {
                continue;
            }
            let mut path: Vec<usize> = Vec::new();
            let mut cur = start;
            // depth of the chain's top element: Some(d) once known, None while unresolved
            let top = loop {
                if path.contains(&cur) || path.len() as u32 > MAX_DEPTH {
                    break BAD; // cycle or too deep
                }
                path.push(cur);
                match in_list(kids[cur].parent) {
                    Some(n) if depth[n] != UNSET => {
                        break if depth[n] == BAD || depth[n] >= MAX_DEPTH {
                            BAD
                        } else {
                            depth[n] + 1
                        };
                    }
                    Some(n) => cur = n,
                    None => break 0, // parent is not a listed child: this one is a chain root
                }
            };
            let mut d = top;
            for &n in path.iter().rev() {
                depth[n] = d;
                d = if d == BAD || d >= MAX_DEPTH {
                    BAD
                } else {
                    d + 1
                };
            }
        }
        let mut order: Vec<usize> = (0..kids.len()).filter(|&i| depth[i] != BAD).collect();
        self.stats.children_unresolved += kids.len() - order.len();
        order.sort_by_key(|&i| (depth[i], i));

        let mut last_chunk: Option<(ChunkIndex, bool)> = None;
        for i in order {
            let k = &kids[i];
            let count = w.store.chunk(k.chunk).map_or(0, |ch| ch.count as usize);
            // the parent's current matrix (stored, or computed earlier because depth is smaller)
            let Some(eic) = w
                .resolve(k.parent)
                .and_then(|p| w.store.entity_in_chunk(p.index))
                .map(|e| e.chunk)
            else {
                self.stats.children_unresolved += 1;
                continue;
            };
            let Some(pw) = w.store.try_component_bytes(k.parent, ltw)? else {
                self.stats.children_unresolved += 1;
                continue;
            };
            let pw: Mat = f32s::<16>(pw);
            let own_changed = match last_chunk {
                Some((c, v)) if c == k.chunk => v,
                _ => {
                    let v = self.chunk_inputs_changed(w, k.chunk, ctx)?;
                    last_chunk = Some((k.chunk, v));
                    v
                }
            };
            let parent_changed = ctx.did_change(w.store.chunk_change_version(eic, ltw)?);
            if !(own_changed || parent_changed) {
                self.stats.children_skipped += 1;
                continue;
            }
            let Some(local) = self.authoring_at(w, k.chunk, k.slot, count)?.local_matrix() else {
                self.stats.children_unresolved += 1;
                continue;
            };
            let world_m = child_to_world(&pw, &local);
            w.store
                .set_component_bytes(k.entity, ltw, &mat_bytes(&world_m))?;
            self.stats.children_written += 1;
        }
        Ok(())
    }
}

impl System for TransformSystem {
    fn name(&self) -> &str {
        Self::NAME
    }
    fn update(&mut self, world: &mut World, ctx: &FrameContext) -> Result<()> {
        self.stats = TransformStats::default();
        self.update_roots(world, ctx)?;
        self.update_children(world, ctx)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const I: Mat = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    #[test]
    fn identity_trs_has_positive_zeros() {
        // A rotation with negative-zero lanes must not leak `-0.0` into the product form.
        let m = trs_matrix([0.0; 3], [-0.0, 0.0, -0.0, 1.0], 1.0);
        assert!(m.iter().zip(&I).all(|(a, b)| a.to_bits() == b.to_bits()));
    }

    #[test]
    fn child_composes_translation_and_scale() {
        let parent = trs_matrix([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], 2.0);
        let child = trs_matrix([1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], 1.0);
        let w = child_to_world(&parent, &child);
        assert_eq!(&w[12..15], &[3.0, 2.0, 3.0]);
        assert_eq!(w[0], 2.0);
        let lt = LocalTransform {
            position: [1.0, 2.0, 3.0],
            scale: 2.0,
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(local_transform_matrix(&lt), parent);
    }
}
