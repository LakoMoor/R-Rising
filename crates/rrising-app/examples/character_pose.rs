//! Offline diagnostic: render a selected prefab in its serialized pose.
//! Requires the owner's game data; this is not gameplay or an animation.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use rrising_math::{float3, float4x4};
use rrising_render::{
    mesh::upload_vertices, skin::skin_static_pose, DrawCall, FrameUniforms, Instance, Renderer,
};
use std::path::Path;
use unity_formats::scene::Scene;

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .context("usage: character_pose output.png [serialized_file [prefab_prefix]]")?;
    let file = args
        .next()
        .unwrap_or_else(|| "ade72d3099b8ad7fbfcba0827fe43cd7".into());
    let prefix = args
        .next()
        .unwrap_or_else(|| "HYB_VampireMale_CustomizationScreen_Prefab/".into());
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    let scene = Scene::read(content.player(), &file)?;
    let (renderers, _) = scene.renderers(content.player())?;
    let mut renderer = pollster::block_on(Renderer::new_offscreen(768, 768))?;
    let mut meshes = Vec::new();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for source in renderers
        .into_iter()
        .filter(|r| r.path.starts_with(&prefix) && r.skin.is_some())
    {
        let skin = source.skin.context("skin")?;
        let joint_world = skin
            .bones
            .iter()
            .map(|id| Ok(scene.world_of(id.context("missing bone")?)?))
            .collect::<Result<Vec<_>>>()?;
        let mesh = content.read_mesh(&source.mesh)?;
        let (vertices, indices) = skin_static_pose(&mesh, &joint_world)?;
        for v in &vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(v.position[axis]);
                max[axis] = max[axis].max(v.position[axis]);
            }
        }
        println!(
            "{}: {} skinned vertices, bounds {:?} to {:?}",
            source.path,
            vertices.len(),
            min,
            max
        );
        meshes.push(upload_vertices(
            renderer.device(),
            &mesh.name,
            &vertices,
            &indices,
            mesh.submeshes.len(),
            true,
        )?);
    }
    anyhow::ensure!(!meshes.is_empty(), "no candidate character meshes");
    let center = float3::new(
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    );
    let size = (max[1] - min[1]).max(max[0] - min[0]).max(0.1);
    let eye = center + float3::new(0.0, 0.0, size * 2.2);
    let camera = float4x4::look_at(eye, center, float3::new(0.0, 1.0, 0.0));
    let view = float4x4::scale(float3::new(1.0, 1.0, -1.0)) * camera.inverse();
    let proj =
        float4x4::perspective_fov(std::f32::consts::FRAC_PI_3, 1.0, size * 0.01, size * 20.0);
    let uniforms = FrameUniforms {
        view_proj: Instance::from_matrix(&(rrising_render::clip_fix() * proj * view)).model,
        camera_pos: [eye.x, eye.y, eye.z, 0.0],
        light_dir: [0.35, 0.85, 0.4, 0.85],
        ambient: [0.32, 0.36, 0.44, 0.0],
    };
    let instance = [Instance::from_matrix(&float4x4::identity)];
    let neutral = renderer.neutral_view();
    let draws: Vec<_> = meshes
        .iter()
        .map(|mesh| DrawCall {
            mesh,
            instances: &instance,
            texture: &neutral,
            cutout: false,
        })
        .collect();
    let (width, height, rgba) = renderer.render_to_image(&uniforms, &draws)?;
    image::save_buffer(output, &rgba, width, height, image::ColorType::Rgba8)?;
    Ok(())
}
