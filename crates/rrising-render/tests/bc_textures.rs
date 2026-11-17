use rrising_render::texture::TextureData;
use wgpu::TextureFormat as F;

fn texture(format: F, width: u32, height: u32, levels: Vec<Vec<u8>>) -> TextureData {
    TextureData {
        name: "synthetic BC fixture".into(),
        width,
        height,
        format,
        levels,
    }
}

#[test]
fn bc1_preserves_srgb_rows_mips_and_crops_padding() {
    // RGB565 red/green endpoints; one solid palette index per stored row.
    let block = [0x00, 0xf8, 0xe0, 0x07, 0x00, 0x55, 0xaa, 0xff];
    let data = texture(
        F::Bc1RgbaUnormSrgb,
        5,
        3,
        vec![block.repeat(2), block.to_vec(), block.to_vec()],
    );
    let decoded = data.decoded_bc().unwrap();
    assert_eq!(decoded.format, F::Rgba8UnormSrgb);
    assert_eq!(
        decoded.levels.iter().map(Vec::len).collect::<Vec<_>>(),
        [60, 8, 4]
    );
    let expected = [[255, 0, 0, 255], [0, 255, 0, 255], [170, 85, 0, 255]];
    for (row, expected) in decoded.levels[0].as_chunks::<20>().0.iter().zip(expected) {
        assert!(row
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == expected));
    }
    assert_eq!(decoded.levels[2], [255, 0, 0, 255]);
    // Decoding must not mutate the source, which remains exportable as its original blocks.
    assert_eq!(data.levels[0], block.repeat(2));
}

#[test]
fn bc3_alpha_and_bc4_bc5_channel_defaults_survive_decode() {
    let alpha = [255, 0, 0, 0, 0, 0, 0, 0];
    let color = [0x00, 0xf8, 0xe0, 0x07, 0x55, 0x55, 0x55, 0x55];
    let bc3 = texture(F::Bc3RgbaUnorm, 4, 4, vec![[alpha, color].concat()])
        .decoded_bc()
        .unwrap();
    assert_eq!(bc3.format, F::Rgba8Unorm);
    assert!(bc3.levels[0]
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 255, 0, 255]));
    let bc4 = texture(F::Bc4RUnorm, 4, 4, vec![alpha.to_vec()])
        .decoded_bc()
        .unwrap();
    assert_eq!(bc4.format, F::R8Unorm);
    assert_eq!(bc4.levels[0], vec![255; 16]);
    let bc5 = texture(F::Bc5RgUnorm, 4, 4, vec![[alpha, [0; 8]].concat()])
        .decoded_bc()
        .unwrap();
    assert_eq!(bc5.format, F::Rg8Unorm);
    assert!(bc5.levels[0]
        .as_chunks::<2>()
        .0
        .iter()
        .all(|p| *p == [255, 0]));
}

#[test]
fn bc6_preserves_half_float_storage_and_opaque_alpha() {
    for format in [F::Bc6hRgbUfloat, F::Bc6hRgbFloat] {
        let decoded = texture(format, 4, 4, vec![vec![0; 16]])
            .decoded_bc()
            .unwrap();
        assert_eq!(decoded.format, F::Rgba16Float);
        assert!(decoded.levels[0]
            .as_chunks::<8>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 0, 0, 0, 0, 0x3c]));
    }
}

#[test]
fn invalid_bc_data_is_rejected_before_the_block_decoder() {
    for source in [
        texture(F::Bc1RgbaUnorm, 0, 4, vec![vec![0; 8]]),
        texture(F::Bc1RgbaUnorm, 4, 4, vec![vec![0; 7]]),
        texture(F::Bc7RgbaUnorm, 5, 3, vec![vec![0; 16]]),
        texture(F::Bc1RgbaUnorm, 1, 1, vec![vec![0; 8]; 2]),
        texture(F::Rgba8Unorm, 4, 4, vec![vec![0; 64]]),
    ] {
        assert!(source.decoded_bc().is_err());
    }
}

#[cfg(feature = "unity")]
#[test]
fn unity_bc6_is_unsigned() {
    assert_eq!(
        rrising_render::unity_texture::map_format(
            unity_formats::texture::TextureFormat::Bc6h,
            false
        )
        .unwrap(),
        F::Bc6hRgbUfloat
    );
}
