//! Unity.Rendering.MaterialMeshInfo's 12-byte scene component.
//! Static array indices are negative, runtime batch IDs nonnegative. Index ranges
//! use the high bit in the packed third word. No GPU or asset lookup lives here.
use anyhow::{ensure, Result};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialMeshInfo {
    pub material: i32,
    pub mesh: i32,
    pub sub_mesh_index_info: u32,
}

impl MaterialMeshInfo {
    /// src: ArrayIndexToStaticIndex @ 0x8f46ef0. Negative values pass through.
    pub const fn array_index_to_static_index(index: i32) -> i32 {
        if index < 0 {
            index
        } else {
            !index
        }
    }
    /// src: StaticIndexToArrayIndex @ 0x8f46f00. Preserve MIN overflow.
    pub const fn static_index_to_array_index(index: i32) -> i32 {
        let negative = index.wrapping_neg();
        let absolute = if negative <= index { index } else { negative };
        absolute.wrapping_sub(1)
    }
    /// src: FromRenderMeshArrayIndices @ 0x8f46f10.
    pub const fn from_render_mesh_array_indices(material: i32, mesh: i32, sub_mesh: u16) -> Self {
        Self {
            material: Self::array_index_to_static_index(material),
            mesh: Self::array_index_to_static_index(mesh),
            sub_mesh_index_info: sub_mesh as u32,
        }
    }
    /// src: constructor @ 0x8f46f80 stores raw IDs and the full ushort.
    pub const fn new(material: i32, mesh: i32, sub_mesh: u16) -> Self {
        Self {
            material,
            mesh,
            sub_mesh_index_info: sub_mesh as u32,
        }
    }
    /// src: SubMeshIndexInfo32 range constructor @ 0x8f577b0.
    /// The original assertion's invalid length path is not emulated; return an error.
    pub fn from_material_mesh_index_range(start: u16, length: u8) -> Result<Self> {
        ensure!(length < 128, "material/mesh range length exceeds 7 bits");
        Ok(Self {
            material: 0,
            mesh: 0,
            sub_mesh_index_info: 0x8000_0000 | ((length as u32) << 20) | start as u32,
        })
    }
    /// src: get_SubMesh @ 0x8f46e70 reads only eight bits, unlike its setter.
    pub const fn sub_mesh(self) -> u16 {
        (self.sub_mesh_index_info & 0xff) as u16
    }
    /// src: set_SubMesh @ 0x8f46e80 replaces the entire packed word.
    pub fn set_sub_mesh(&mut self, value: u16) {
        self.sub_mesh_index_info = value as u32;
    }
    /// src: get_HasMaterialMeshIndexRange @ 0x8f46ed0.
    pub const fn has_material_mesh_index_range(self) -> bool {
        self.sub_mesh_index_info & 0x8000_0000 != 0
    }
    /// src: get_MaterialMeshIndexRange @ 0x8f46e90; extracts ushort start and byte length.
    pub const fn material_mesh_index_range(self) -> Option<(u16, u8)> {
        if self.has_material_mesh_index_range() {
            Some((
                self.sub_mesh_index_info as u16,
                ((self.sub_mesh_index_info >> 20) & 0x7f) as u8,
            ))
        } else {
            None
        }
    }
    /// src: get_IsRuntimeMaterial/get_IsRuntimeMesh @ 0x8f47010/0x8f47030.
    pub const fn is_runtime_material(self) -> bool {
        !self.has_material_mesh_index_range() && self.material >= 0
    }
    pub const fn is_runtime_mesh(self) -> bool {
        !self.has_material_mesh_index_range() && self.mesh >= 0
    }
    /// src: get_MaterialArrayIndex/get_MeshArrayIndex @ 0x8f47090/0x8f47050.
    pub const fn material_array_index(self) -> i32 {
        if self.is_runtime_material() {
            -1
        } else {
            Self::static_index_to_array_index(self.material)
        }
    }
    pub const fn mesh_array_index(self) -> i32 {
        if self.is_runtime_mesh() {
            -1
        } else {
            Self::static_index_to_array_index(self.mesh)
        }
    }
    /// src: set_MaterialArrayIndex/set_MeshArrayIndex @ 0x8f470c0/0x8f47080.
    pub fn set_material_array_index(&mut self, value: i32) {
        self.material = Self::array_index_to_static_index(value);
    }
    pub fn set_mesh_array_index(&mut self, value: i32) {
        self.mesh = Self::array_index_to_static_index(value);
    }

    /// Safe read from component storage. Never follows a native pointer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == 12,
            "MaterialMeshInfo requires exactly 12 bytes"
        );
        Ok(Self {
            material: i32::from_le_bytes(bytes[0..4].try_into()?),
            mesh: i32::from_le_bytes(bytes[4..8].try_into()?),
            sub_mesh_index_info: u32::from_le_bytes(bytes[8..12].try_into()?),
        })
    }
}
