@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>) -> Out {
    var out: Out;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    return out;
}
@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, in.uv);
}
