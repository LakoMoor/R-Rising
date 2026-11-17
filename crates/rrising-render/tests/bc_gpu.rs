//! Hardware decoder comparison, not an original-game rendering oracle. Synthetic blocks only.
use rrising_render::texture::TextureData;
use wgpu::TextureFormat as F;

fn sample(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) -> Vec<[f32; 4]> {
    let (width, height) = (view.texture().width(), view.texture().height());
    let size = u64::from(width) * u64::from(height) * 16;
    let storage = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("decoded texels"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texel readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("BC decoder comparison"),
        source: wgpu::ShaderSource::Wgsl(
            r"
            @group(0) @binding(0) var input: texture_2d<f32>;
            @group(0) @binding(1) var<storage, read_write> output: array<vec4f>;
            @compute @workgroup_size(8,8)
            fn main(@builtin(global_invocation_id) id: vec3u) {
                let dim = textureDimensions(input);
                if id.x < dim.x && id.y < dim.y {
                    output[id.y * dim.x + id.x] = textureLoad(input, vec2i(id.xy), 0);
                }
            }
        "
            .into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: storage.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, size);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let pixels = bytemuck::cast_slice(&readback.slice(..).get_mapped_range().unwrap()).to_vec();
    readback.unmap();
    pixels
}

fn fixture(format: F) -> TextureData {
    let bytes = format.block_copy_size(None).unwrap() as usize;
    let mut rng = 0x72697365u32;
    let mut level = vec![0; 8 * 8 * bytes];
    for (i, block) in level.chunks_exact_mut(bytes).enumerate() {
        for byte in block.iter_mut() {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *byte = rng as u8;
        }
        // Cover all legal BC7 and BC6H modes without relying on undefined reserved modes.
        if matches!(format, F::Bc7RgbaUnorm | F::Bc7RgbaUnormSrgb) {
            let mode = i % 8;
            block[0] = (block[0] & !(((1u16 << (mode + 1)) - 1) as u8)) | (1 << mode);
        }
        if matches!(format, F::Bc6hRgbFloat | F::Bc6hRgbUfloat) {
            let mode = [0, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15][i % 14];
            block[0] = (block[0] & if mode < 2 { !3 } else { !31 }) | mode;
        }
    }
    TextureData {
        name: format!("synthetic {format:?}"),
        width: 32,
        height: 32,
        format,
        levels: vec![level],
    }
}

#[test]
#[ignore = "requires a GPU with BC hardware decoding; also creates a device without the BC feature"]
fn bc_upload_works_without_bc_and_matches_hardware_decode() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TEXTURE_COMPRESSION_BC,
        ..Default::default()
    }))
    .unwrap();
    let (portable, portable_queue) =
        pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    assert!(!portable
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC));
    let partial_mip = TextureData {
        name: "synthetic partial block rows".into(),
        width: 12,
        height: 12,
        format: F::Bc1RgbaUnorm,
        levels: vec![vec![0; 9 * 8], vec![0; 4 * 8], vec![0; 8]],
    };
    // Even on a BC device, decode a chain whose 6x6 mip cannot flip by block permutation.
    let partial = partial_mip
        .upload(&device, &queue, device.features())
        .unwrap();
    assert_eq!(partial.view.texture().format(), F::Rgba8Unorm);
    assert_eq!(partial.view.texture().mip_level_count(), 3);
    let mut failures = Vec::new();
    for format in [
        F::Bc1RgbaUnorm,
        F::Bc1RgbaUnormSrgb,
        F::Bc3RgbaUnorm,
        F::Bc3RgbaUnormSrgb,
        F::Bc4RUnorm,
        F::Bc5RgUnorm,
        F::Bc6hRgbUfloat,
        F::Bc6hRgbFloat,
        F::Bc7RgbaUnorm,
        F::Bc7RgbaUnormSrgb,
    ] {
        let data = fixture(format);
        // This device cannot create a compressed texture: the real upload must fall back.
        let fallback = data
            .upload(&portable, &portable_queue, portable.features())
            .unwrap();
        assert!(fallback
            .view
            .texture()
            .format()
            .required_features()
            .is_empty());
        let actual = sample(&portable, &portable_queue, &fallback.view);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hardware BC reference"),
            size: wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &data.levels[0],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * format.block_copy_size(None).unwrap()),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
        );
        let reference = sample(&device, &queue, &texture.create_view(&Default::default()));
        let exact = matches!(
            format,
            F::Bc6hRgbFloat | F::Bc6hRgbUfloat | F::Bc7RgbaUnorm | F::Bc7RgbaUnormSrgb
        );
        // D3D11.3 19.5.2 permits endpoint-dependent BC1..5 interpolation variation.
        // The original one-step assumption failed on Metal; first differing blocks and
        // the reason for this bounded two-step regression are recorded in COMPATIBILITY.md.
        // This is not a universal spec-conformance bound. BC6H/BC7 remain exact.
        let tolerance = if exact {
            0.0
        } else if format.is_srgb() {
            2.4 * 2.0 / 255.0
        } else {
            2.0 / 255.0
        };
        let mut max_delta = 0f32;
        for y in 0..32 {
            for x in 0..32 {
                for c in 0..4 {
                    let delta = (actual[y * 32 + x][c] - reference[(31 - y) * 32 + x][c]).abs();
                    assert!(delta.is_finite());
                    if exact {
                        assert_eq!(
                            actual[y * 32 + x][c].to_bits(),
                            reference[(31 - y) * 32 + x][c].to_bits(),
                            "{format:?} ({x}, {y}) channel {c}"
                        );
                    }
                    max_delta = max_delta.max(delta);
                }
            }
        }
        println!(
            "{format:?}: 64 synthetic blocks, max channel delta {max_delta:e}, bound {tolerance:e}"
        );
        if max_delta > tolerance + f32::EPSILON {
            failures.push(format);
            for (i, pixel) in actual.iter().enumerate() {
                let expected = reference[(31 - i / 32) * 32 + i % 32];
                if pixel
                    .iter()
                    .zip(expected)
                    .any(|(a, b)| (a - b).abs() > tolerance + f32::EPSILON)
                {
                    let block = ((31 - i / 32) / 4 * 8 + (i % 32) / 4)
                        * format.block_copy_size(None).unwrap() as usize;
                    println!(
                        "FIRST at {i}: CPU {pixel:?}, GPU {expected:?}, block {:02x?}",
                        &data.levels[0]
                            [block..block + format.block_copy_size(None).unwrap() as usize]
                    );
                    break;
                }
            }
        }
    }
    assert!(failures.is_empty(), "decoder differences: {failures:?}");
}
