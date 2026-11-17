use std::path::PathBuf;

use anyhow::{Context, Result};
use rrising_content::persistence::{PersistentSave, SavedWorldData, SavedWorldDataStats};

const SAMPLE: [u8; 69] = [
    0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x20, 0x64, 0xa8, 0xec, 0x30, 0x75, 0xb9, 0xfd,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn reads_unaligned_hashes_and_chunk_indices() -> Result<()> {
    let world = SavedWorldData::read(&SAMPLE)?;
    assert_eq!(world.entity_capacity(), 3);
    assert_eq!(world.entity_versions().collect::<Vec<_>>(), [2, 7, 9]);
    assert_eq!(world.entity_version(3), None);
    assert_eq!(world.parsed_bits(), 513);
    assert_eq!(world.trailing_zero_bytes(), 4);
    assert_eq!(
        world.stats(),
        SavedWorldDataStats {
            archetypes: 1,
            chunks: 2,
            entities: 3,
            type_hashes: 2
        }
    );
    let archetypes = world
        .nonpersistent_archetypes
        .as_ref()
        .context("missing debug data")?;
    let archetype = &archetypes[0];
    assert_eq!(archetype.offset_bits, 161);
    assert_eq!(
        archetype.type_hashes().collect::<Vec<_>>(),
        [0xfedc_ba98_7654_3210, 1]
    );
    assert_eq!(
        archetype.chunks[0].entity_indices().collect::<Vec<_>>(),
        [2, 0]
    );
    assert_eq!(
        archetype.chunks[1].entity_indices().collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(encode_world(&world), SAMPLE);
    Ok(())
}

#[test]
fn accepts_no_debug_and_rejects_truncated_counts_and_invalid_indices() -> Result<()> {
    let mut bytes = SAMPLE[..17].to_vec();
    bytes[16] = 0;
    let world = SavedWorldData::read(&bytes)?;
    assert!(world.nonpersistent_archetypes.is_none());
    assert_eq!(world.parsed_bits(), 129);
    assert_eq!(world.trailing_zero_bytes(), 0);
    assert_eq!(encode_world(&world), bytes);
    for size in 0..65 {
        assert!(
            SavedWorldData::read(&SAMPLE[..size]).is_err(),
            "prefix {size}"
        );
    }
    let mut invalid = SAMPLE;
    invalid[60] = 8;
    assert!(format!("{:#}", SavedWorldData::read(&invalid).unwrap_err())
        .contains("nonpersistent entity index exceeds saved world capacity"));
    invalid = SAMPLE;
    invalid[68] = 1;
    assert!(SavedWorldData::read(&invalid).is_err());
    Ok(())
}

#[test]
#[ignore = "requires RRISING_SAVE_DIR with original reference autosaves 4 and 5"]
fn original_world_members_match_observations_and_reencode_exactly() -> Result<()> {
    let folder =
        PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("set RRISING_SAVE_DIR")?);
    for (number, fingerprint, changing_versions) in [
        (4, 0x0852_bf1e_e493_2871, [11, 7, 3, 3]),
        (5, 0xde79_68ef_5b1c_9469, [13, 9, 6, 6]),
    ] {
        let save = PersistentSave::open(&folder.join(format!("AutoSave_{number}.save.gz")))?;
        let index = save
            .members()
            .iter()
            .position(|member| member.name == "WorldData.save")
            .context("missing WorldData.save")?;
        let raw = save.member_data(index).context("missing world bytes")?;
        assert_eq!(raw.len(), 6_652_241);
        assert_eq!(
            raw.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            }),
            fingerprint
        );
        let world = SavedWorldData::read(raw)?;
        assert_eq!(world.entity_capacity(), 1_048_576);
        assert_eq!(world.entity_version(152_830), Some(4));
        for (slot, expected) in [152_847, 559_505, 559_507, 559_509]
            .into_iter()
            .zip(changing_versions)
        {
            assert_eq!(world.entity_version(slot), Some(expected));
        }
        assert_eq!(world.parsed_bits(), 53_217_889);
        assert_eq!(world.trailing_zero_bytes(), 4);
        assert_eq!(
            world.stats(),
            SavedWorldDataStats {
                archetypes: 3_702,
                chunks: 6_577,
                entities: 284_158,
                type_hashes: 158_171
            }
        );
        let archetypes = world
            .nonpersistent_archetypes
            .as_ref()
            .context("missing original debug data")?;
        let first = &archetypes[0];
        assert_eq!(first.offset_bits, 33_554_497);
        assert_eq!(first.type_hashes().next(), Some(0x07ba_ba3b_b51b_4d74));
        assert_eq!(
            first
                .chunks
                .iter()
                .map(|chunk| chunk.entity_indices().len())
                .collect::<Vec<_>>(),
            [128, 128, 2]
        );
        let last = archetypes.last().context("missing last archetype")?;
        assert_eq!(last.offset_bits, 53_217_249);
        assert_eq!(
            last.chunks[0].entity_indices().collect::<Vec<_>>(),
            (2013..=2019).collect::<Vec<_>>()
        );
        assert_eq!(encode_world(&world), raw);
    }
    Ok(())
}

fn encode_world(world: &SavedWorldData<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend((world.entity_capacity() as u32).to_le_bytes());
    for version in world.entity_versions() {
        bytes.extend(version.to_le_bytes());
    }
    bytes.push(u8::from(world.nonpersistent_archetypes.is_some()));
    if let Some(archetypes) = &world.nonpersistent_archetypes {
        extend_unaligned(&mut bytes, (archetypes.len() as u32).to_le_bytes());
        for archetype in archetypes {
            let hashes = archetype.type_hashes();
            extend_unaligned(&mut bytes, (hashes.len() as u32).to_le_bytes());
            for hash in hashes {
                extend_unaligned(&mut bytes, hash.to_le_bytes());
            }
            extend_unaligned(&mut bytes, (archetype.chunks.len() as u32).to_le_bytes());
            for chunk in &archetype.chunks {
                let indices = chunk.entity_indices();
                extend_unaligned(&mut bytes, (indices.len() as u32).to_le_bytes());
                for index in indices {
                    extend_unaligned(&mut bytes, (index as u32).to_le_bytes());
                }
            }
        }
    }
    bytes.resize(bytes.len() + world.trailing_zero_bytes(), 0);
    bytes
}

fn extend_unaligned(bytes: &mut Vec<u8>, source: impl IntoIterator<Item = u8>) {
    for next in source {
        let last = bytes.len() - 1;
        bytes[last] |= next << 1;
        bytes.push(next >> 7);
    }
}
