//! Resolve candidate referenced Unity object tables of the hybrid streaming scene.
use anyhow::Result;
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
                        println!(
                            "header_offset={offset:#x} table={obj:?} {} {} {:?}",
                            named.class, named.name, r
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
