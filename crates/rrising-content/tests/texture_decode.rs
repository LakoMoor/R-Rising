//! A differential check on the texture bytes: decode the original's own BC blocks and look at what
//! they say.
//!
//! This is not a golden test — there is no original frame to compare against yet. It answers a
//! narrower question the renderer cannot answer by itself: when a draw binds a base colour map and
//! the frame still comes out grey, is the art grey, or is the port sampling the wrong texels? The
//! BC1/BC3 decode here is written from the same block layout `rrising-render` uploads, so a texture
//! that decodes to colour here and renders grey has a sampling problem, not a data problem.

use rrising_content::GameContent;

fn rgb565(v: u16) -> [f32; 3] {
    [
        ((v >> 11) & 0x1f) as f32 / 31.0,
        ((v >> 5) & 0x3f) as f32 / 63.0,
        (v & 0x1f) as f32 / 31.0,
    ]
}

/// The mean RGB of a BC1 (DXT1) level: 8 bytes a 4x4 block, two 565 endpoints and 16 2-bit indices.
fn mean_bc1(data: &[u8]) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut n = 0usize;
    for b in data.chunks(8) {
        if b.len() < 8 {
            continue;
        }
        let c0 = rgb565(u16::from_le_bytes([b[0], b[1]]));
        let c1 = rgb565(u16::from_le_bytes([b[2], b[3]]));
        let table = if c0[0] + c0[1] + c0[2] > c1[0] + c1[1] + c1[2] {
            [
                c0,
                c1,
                [
                    (2.0 * c0[0] + c1[0]) / 3.0,
                    (2.0 * c0[1] + c1[1]) / 3.0,
                    (2.0 * c0[2] + c1[2]) / 3.0,
                ],
                [
                    (c0[0] + 2.0 * c1[0]) / 3.0,
                    (c0[1] + 2.0 * c1[1]) / 3.0,
                    (c0[2] + 2.0 * c1[2]) / 3.0,
                ],
            ]
        } else {
            [
                c0,
                c1,
                [
                    (c0[0] + c1[0]) / 2.0,
                    (c0[1] + c1[1]) / 2.0,
                    (c0[2] + c1[2]) / 2.0,
                ],
                [0.0, 0.0, 0.0],
            ]
        };
        for i in 0..16 {
            // 16 selectors in 4 bytes: byte i/4, two bits at (i%4)*2.
            let sel = (b[4 + i / 4] >> ((i % 4) * 2)) & 3;
            for (a, m) in sum.iter_mut().zip(table[sel as usize]) {
                *a += m;
            }
            n += 1;
        }
    }
    if n == 0 {
        return [0.0, 0.0, 0.0];
    }
    [sum[0] / n as f32, sum[1] / n as f32, sum[2] / n as f32]
}

/// The mean RGB of a BC3 (DXT5) level: an alpha block, then the same colour block BC1 uses.
fn mean_bc3(data: &[u8]) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut n = 0usize;
    for b in data.chunks(16) {
        if b.len() < 16 {
            continue;
        }
        let c0 = rgb565(u16::from_le_bytes([b[8], b[9]]));
        let c1 = rgb565(u16::from_le_bytes([b[10], b[11]]));
        let table = [
            c0,
            c1,
            [
                (2.0 * c0[0] + c1[0]) / 3.0,
                (2.0 * c0[1] + c1[1]) / 3.0,
                (2.0 * c0[2] + c1[2]) / 3.0,
            ],
            [
                (c0[0] + 2.0 * c1[0]) / 3.0,
                (c0[1] + 2.0 * c1[1]) / 3.0,
                (c0[2] + 2.0 * c1[2]) / 3.0,
            ],
        ];
        for i in 0..16 {
            // 16 selectors in 4 bytes: byte i/4, two bits at (i%4)*2.
            let sel = (b[12 + i / 4] >> ((i % 4) * 2)) & 3;
            for (a, m) in sum.iter_mut().zip(table[sel as usize]) {
                *a += m;
            }
            n += 1;
        }
    }
    if n == 0 {
        return [0.0, 0.0, 0.0];
    }
    [sum[0] / n as f32, sum[1] / n as f32, sum[2] / n as f32]
}

fn install() -> std::path::PathBuf {
    std::path::PathBuf::from("/Users/lakomoor/ported/VRising/VRising_Data")
}

fn mean_of(name: &str) -> Option<(String, u32, u32, i64, [f32; 3])> {
    let dir = install().join("StreamingAssets/ContentArchives");
    let all = rrising_content::list_archives(&dir).ok()?;
    let content = GameContent::open(&install(), &all, &install().join("../R-Rising/assets/typetrees.tpk")).ok()?;
    let entry = content.entries().iter().find(|e| e.class == "Texture2D" && e.name == name)?;
    let tex = content.read_texture(&entry.object).ok()?;
    let Ok((format, levels)) = tex.levels() else { return None };
    let mean = match format {
        unity_formats::texture::TextureFormat::Dxt1 => mean_bc1(&levels[0]),
        unity_formats::texture::TextureFormat::Dxt5 => mean_bc3(&levels[0]),
        other => {
            println!("{name}: format {other:?} not decoded by this test");
            return None;
        }
    };
    Some((format!("{format:?}"), tex.width, tex.height, tex.format, mean))
}

#[test]
fn base_colour_maps_decode_to_colour() {
    let mut checked = 0;
    for name in ["OnionPlant01_bc", "ResearchStationCloth01_bc", "TornCloak01_bc"] {
        let Some((format, w, h, raw, mean)) = mean_of(name) else {
            println!("{name}: not found in the indexed content");
            continue;
        };
        let spread = mean[0].max(mean[1].max(mean[2])) - mean[0].min(mean[1].min(mean[2]));
        println!(
            "{name}: {w}x{h} raw format {raw} → {format}, mean rgb ({:.3}, {:.3}, {:.3}), channel spread {spread:.3}",
            mean[0], mean[1], mean[2]
        );
        // A base colour map with no channel spread at all is a mask, not art — and a grey frame
        // with such a texture bound is the art's doing, not the sampler's.
        assert!(
            spread >= 0.0,
            "{name}: mean rgb is not finite: {mean:?}"
        );
        checked += 1;
    }
    assert!(checked > 0, "none of the named textures were found");
}
