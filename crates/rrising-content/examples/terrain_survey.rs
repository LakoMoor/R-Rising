//! Inventory original Unity Terrain and TerrainData objects in the owner's installation.
use rrising_content::{list_archives, GameContent};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    let mut count = 0;
    for (file, serialized) in &content.player().files {
        for object in &serialized.objects {
            if [156, 218].contains(&object.class_id) {
                let reference = unity_formats::player::ObjRef {
                    file: file.clone(),
                    path_id: object.path_id,
                };
                println!(
                    "class id {} class {:?} {:?}",
                    object.class_id,
                    content.player().class_name(&reference),
                    reference
                );
                count += 1;
            }
        }
    }
    println!("TerrainData (156) / Terrain (218) objects: {count}");
    Ok(())
}
