//! Reads the real install. Skipped when the game folder is absent so the suite stays portable.

use std::path::Path;
use std::time::Instant;

const DATA: &str = "/Users/lakomoor/ported/VRising/VRising_Data";
const TPK: &str = "/Users/lakomoor/ported/VRising/R-Rising/assets/typetrees.tpk";

fn install_present() -> bool {
    Path::new(DATA).is_dir() && Path::new(TPK).is_file()
}

#[test]
fn opens_a_slice_of_the_content_archives() {
    if !install_present() {
        eprintln!("skipping: no V Rising install at {DATA}");
        return;
    }
    let dir = Path::new(DATA).join("StreamingAssets/ContentArchives");
    let all = rrising_content::list_archives(&dir).unwrap();
    let slice: Vec<_> = all.iter().take(200).cloned().collect();
    let t = Instant::now();
    let content = rrising_content::GameContent::open(Path::new(DATA), &slice, Path::new(TPK)).unwrap();
    let elapsed = t.elapsed();
    let counts = content.class_counts();
    println!(
        "opened {} archives + player files in {:.1?}; {} files, {} indexed objects",
        slice.len(),
        elapsed,
        content.file_count(),
        content.entries().len()
    );
    for (k, v) in &counts {
        println!("  {k:24} {v}");
    }
    assert!(content.entries().len() > 1000, "expected a real index, got {}", content.entries().len());
    assert!(counts.contains_key("Mesh"), "no meshes indexed");
    assert!(counts.contains_key("Texture2D"), "no textures indexed");
}

#[test]
fn reads_a_real_mesh_and_its_textures() {
    if !install_present() {
        eprintln!("skipping: no V Rising install at {DATA}");
        return;
    }
    let dir = Path::new(DATA).join("StreamingAssets/ContentArchives");
    let all = rrising_content::list_archives(&dir).unwrap();
    let slice: Vec<_> = all.iter().take(400).cloned().collect();
    let content = rrising_content::GameContent::open(Path::new(DATA), &slice, Path::new(TPK)).unwrap();

    let mesh_entry = content
        .entries()
        .iter()
        .find(|e| e.class == "Mesh" && !e.name.is_empty())
        .expect("a named mesh");
    let mesh = content.read_mesh(&mesh_entry.object).unwrap();
    println!(
        "mesh {:?}: {} vertices, {} indices, {} submeshes, skin={}",
        mesh.name,
        mesh.vertex_count,
        mesh.indices.len(),
        mesh.submeshes.len(),
        mesh.skin.is_some()
    );
    assert!(mesh.vertex_count > 0, "mesh {} has no vertices", mesh.name);
    assert!(!mesh.indices.is_empty(), "mesh {} has no indices", mesh.name);
    // Positions are channel 0 in Unity's layout.
    let positions = mesh.channels[0].as_ref().expect("a position channel");
    assert_eq!(positions.dim, 3);
    assert_eq!(positions.values.len(), mesh.vertex_count * 3);
    // Every index must address a real vertex.
    let max_index = mesh.indices.iter().max().copied().unwrap_or(0);
    assert!(
        (max_index as usize) < mesh.vertex_count,
        "index {max_index} out of range for {} vertices",
        mesh.vertex_count
    );

    let tex_entry = content
        .entries()
        .iter()
        .find(|e| e.class == "Texture2D" && !e.name.is_empty())
        .expect("a named texture");
    let tex = content.read_texture(&tex_entry.object).unwrap();
    println!(
        "texture {:?}: {}x{} format={} mips={} srgb={} bytes={}",
        tex.name,
        tex.width,
        tex.height,
        tex.format,
        tex.mips,
        tex.srgb,
        tex.data.len()
    );
    assert!(tex.width > 0 && tex.height > 0);
    assert!(!tex.data.is_empty(), "texture {} carries no pixels", tex.name);

    let mat_entry = content
        .entries()
        .iter()
        .find(|e| e.class == "Material" && !e.name.is_empty())
        .expect("a named material");
    let mat = content.read_material(&mat_entry.object).unwrap();
    println!(
        "material {:?}: shader={:?} textures={} queue={}",
        mat.name,
        mat.shader.as_ref().map(|s| &s.file),
        mat.textures.len(),
        mat.render_queue
    );
    assert!(mat.shader.is_some(), "material {} names no shader", mat.name);
}

#[test]
fn localization_and_settings_load() {
    if !install_present() {
        eprintln!("skipping: no V Rising install at {DATA}");
        return;
    }
    let en = Path::new(DATA).join("StreamingAssets/Localization/English.json");
    let table = rrising_content::localization::LocalizationTable::load(&en).unwrap();
    println!("English localization: {} strings", table.len());
    assert!(table.len() > 100, "expected a real string table");

    let presets = rrising_content::settings::list_presets(
        &Path::new(DATA).join("StreamingAssets/GameSettingPresets"),
    )
    .unwrap();
    println!("setting presets: {:?}", presets.len());
    assert!(presets.iter().any(|p| p == "StandardPvE"), "no StandardPvE preset");

    let doc = rrising_content::settings::load_json(
        &Path::new(DATA).join("StreamingAssets/GameSettingPresets/StandardPvE.json"),
    )
    .unwrap();
    println!("StandardPvE top-level keys: {:?}", doc.as_object().map(|m| m.keys().take(6).collect::<Vec<_>>()));
    assert!(doc.is_object());
}

#[test]
fn opens_the_whole_content_archive_folder() {
    if !install_present() {
        eprintln!("skipping: no V Rising install at {DATA}");
        return;
    }
    let dir = Path::new(DATA).join("StreamingAssets/ContentArchives");
    let all = rrising_content::list_archives(&dir).unwrap();
    println!("archives on disk: {}", all.len());
    let t = Instant::now();
    let content = rrising_content::GameContent::open(Path::new(DATA), &all, Path::new(TPK)).unwrap();
    let elapsed = t.elapsed();
    let counts = content.class_counts();
    let total: usize = counts.values().sum();
    println!(
        "FULL OPEN: {} archives in {:.1?} -> {} files, {} indexed objects",
        all.len(),
        elapsed,
        content.file_count(),
        total
    );
    for (k, v) in &counts {
        println!("  {k:24} {v}");
    }
    // 4411 files sit in the folder; two are not bundles (the dependency catalog and a .bin).
    assert_eq!(all.len(), 4409, "expected every bundle in the folder");
    assert!(total > 150_000, "expected the whole index, got {total}");
    // A mesh the game actually ships, found by name across the whole store.
    let found = content.find("Mesh", "Gloomrot_Sludgepool_Scenery02");
    println!("named mesh lookup: {} hits", found.len());
    assert!(!found.is_empty(), "the store cannot find a mesh by name");
}
