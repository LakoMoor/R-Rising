//! Resolve candidate referenced Unity object tables of the hybrid streaming scene.
use anyhow::{ensure, Result};
use rrising_content::{catalog::RuntimeContentCatalog, list_archives, GameContent};
use std::path::PathBuf;
use unity_formats::player::{ObjRef, PPtr};

fn main() -> Result<()> {
    let data = PathBuf::from(
        std::env::var_os("VRISING_DATA").ok_or_else(|| anyhow::anyhow!("VRISING_DATA"))?,
    );
    let dir = data.join("StreamingAssets/ContentArchives");
    let content = GameContent::open(
        &data,
        &list_archives(&dir)?,
        &PathBuf::from("assets/typetrees.tpk"),
    )?;
    let catalog = RuntimeContentCatalog::open(&dir.join("archive_dependencies.bin"))?;
    let header = std::fs::read(
        data.join("StreamingAssets/EntityScenes/ddfc6a4db4009e2448b8f8b6f0965b8f.entityheader"),
    )?;
    let mut verified = 0;
    for object in &catalog.objects {
        let guid = &object.object_id[16..32];
        let Some(offset) = header
            .windows(16)
            .position(|w| w == guid && w.iter().any(|x| *x != 0))
        else {
            continue;
        };
        let file: String = catalog.files[object.file_index]
            .id
            .to_bytes()
            .iter()
            .map(|b| format!("{:02x}", b.rotate_left(4)))
            .collect();
        let obj = ObjRef {
            file,
            path_id: object.local_identifier,
        };
        if content
            .player()
            .script_class(&obj)
            .ok()
            .flatten()
            .as_deref()
            != Some("Unity.Scenes.ReferencedUnityObjects")
        {
            continue;
        }
        let (_, bytes) = content
            .player()
            .behaviour(&obj)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some(n) = bytes.get(..4) else { continue };
        let n = u32::from_le_bytes(n.try_into()?) as usize;
        if 4 + n * 12 > bytes.len() {
            continue;
        }
        for entry in bytes[4..4 + n * 12].as_chunks::<12>().0 {
            let p = PPtr {
                file_id: i32::from_le_bytes(entry[..4].try_into()?),
                path_id: i64::from_le_bytes(entry[4..].try_into()?),
            };
            if let Some(r) = content.player().resolve_pptr(&obj.file, p).ok().flatten() {
                if let Some(named) = content.entries().iter().find(|e| e.object == r) {
                    let name = named.name.to_lowercase();
                    if name.contains("vampire") || name.contains("player") {
                        let section = i64::from_le_bytes(object.object_id[..8].try_into()?);
                        let generation = u16::from_le_bytes(object.object_id[40..42].try_into()?);
                        println!(
                            "header_offset={offset:#x} section_id={section} generation={generation} object_id={:02x?} table={obj:?} {} {} {:?}",
                            object.object_id, named.class, named.name, r
                        );
                        let expected = match named.name.as_str() {
                            "HYB_VampireMale_Prefab" => Some((
                                406,
                                [
                                    0x60, 0x39, 0x8c, 0xf9, 0x0a, 0x04, 0x7e, 0xc4, 0xc8, 0xce,
                                    0x96, 0x2b, 0x8b, 0x8b, 0x89, 0xca,
                                ],
                            )),
                            "HYB_VampireFemale_Prefab" => Some((
                                407,
                                [
                                    0xcd, 0x32, 0x3c, 0xaa, 0x3f, 0x64, 0x21, 0xf4, 0xdb, 0xcb,
                                    0x08, 0x27, 0xb8, 0xaa, 0xa4, 0xc4,
                                ],
                            )),
                            _ => None,
                        };
                        if let Some((expected_section, guid)) = expected {
                            ensure!(
                                section == expected_section && generation == 6,
                                "unexpected section identity for {}",
                                named.name
                            );
                            let path = data.join(format!("StreamingAssets/EntityScenes/ddfc6a4db4009e2448b8f8b6f0965b8f.{section}.entities"));
                            let bytes = std::fs::read(path)?;
                            ensure!(
                                bytes.windows(16).filter(|w| *w == guid).count() == 1,
                                "expected one model GUID in section {section}"
                            );
                            verified += 1;
                        }
                    }
                }
            }
        }
    }
    ensure!(
        verified == 2,
        "expected two distinct player model section tables, found {verified}"
    );
    eprintln!("verified male/female model GUID bytes against sections 406/407 and their referenced-object tables");
    Ok(())
}
