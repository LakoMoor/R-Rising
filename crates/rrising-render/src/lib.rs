//! `rrising-render` — the port's wgpu renderer.
//!
//! It draws the original's own meshes and textures, using hardware BC when possible and a
//! CPU decoder where compression support or texture orientation requires it. It is deliberately not a reimplementation of HDRP: the game's real pipeline is
//! deferred, clustered and shadowed, and that layer is ported later. What lives here is enough to
//! put real content on screen and to read frames back for comparison.

mod bc;
pub mod mesh;
pub mod renderer;
#[cfg(feature = "unity")]
pub mod skin;
pub mod snapshot;
pub mod texture;
#[cfg(feature = "unity")]
pub mod unity_texture;

pub use mesh::{GpuMesh, Vertex};
pub use renderer::{clip_fix, DrawCall, FrameUniforms, Instance, Renderer, DEPTH_FORMAT};
pub use texture::GpuTexture;
