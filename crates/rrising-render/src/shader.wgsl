// The port's first lighting pass.
//
// V Rising renders through HDRP, whose deferred passes, shadowing, volumetrics and clustered lights
// are far larger than this. This shader reproduces the *result* the original's own fallback uses for
// a plain lit prop — albedo from the material's base map, a hemisphere ambient and one directional
// term — so the port has something to compare against while the HDRP layer is still unported. The
// gap is recorded in docs/PORT.md rather than claimed as parity.

struct Frame {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var albedo_map: texture_2d<f32>;
@group(0) @binding(2) var albedo_sampler: sampler;

struct In {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) model_col0: vec4<f32>,
    @location(4) model_col1: vec4<f32>,
    @location(5) model_col2: vec4<f32>,
    @location(6) model_col3: vec4<f32>,
    ) -> In {
    let model = mat4x4<f32>(model_col0, model_col1, model_col2, model_col3);
    let world = model * vec4<f32>(position, 1.0);
    // Normals transform by the inverse transpose; the port's instances are rigid or uniformly
    // scaled, for which the rotation part is its own inverse transpose.
    let rot = mat3x3<f32>(model_col0.xyz, model_col1.xyz, model_col2.xyz);
    var out: In;
    out.pos = frame.view_proj * world;
    out.world = world.xyz;
    out.normal = normalize(rot * normal);
    out.uv = uv;
    return out;
}

fn shade(in: In, albedo: vec4<f32>) -> vec4<f32> {
    let n = normalize(in.normal);
    let l = normalize(frame.light_dir.xyz);
    // Half-Lambert keeps the dark side readable the way the original's ambient does.
    let ndl = dot(n, l) * 0.5 + 0.5;
    let sky = mix(0.35, 1.0, n.y * 0.5 + 0.5);
    let lit = frame.ambient.rgb * sky + frame.light_dir.w * ndl;
    // WGSL has no swizzle assignment, so the colour is composed in one constructor.
    let rgb = albedo.rgb * lit;
    return vec4<f32>(rgb, albedo.a);
}

@fragment
fn fs_main(in: In) -> @location(0) vec4<f32> {
    return shade(in, textureSample(albedo_map, albedo_sampler, in.uv));
}

// Alpha-tested draws (materials with `_ALPHATEST_ON`). The material's own `_AlphaCutoff` is not
// read; the threshold is a fixed 0.5.
@fragment
fn fs_cutout(in: In) -> @location(0) vec4<f32> {
    let albedo = textureSample(albedo_map, albedo_sampler, in.uv);
    if (albedo.a < 0.5) {
        discard;
    }
    return shade(in, albedo);
}
