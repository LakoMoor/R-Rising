//! Format checks use synthetic bytes. The ignored test explicitly requires the reference install.
use rrising_content::catalog::RuntimeContentCatalog;
use std::path::PathBuf;

fn put_i32(bytes: &mut [u8], at: usize, value: i32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_array(bytes: &mut [u8], descriptor: usize, at: usize, count: i32) {
    put_i32(bytes, descriptor, at as i32 - descriptor as i32);
    put_i32(bytes, descriptor + 4, count);
}
fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 244];
    put_i32(&mut bytes, 0, 1);
    put_i32(&mut bytes, 12, 208);
    put_i32(&mut bytes, 16, 1);
    put_array(&mut bytes, 36, 76, 2);
    put_array(&mut bytes, 44, 108, 2);
    put_array(&mut bytes, 52, 156, 1);
    put_array(&mut bytes, 68, 220, 2);
    bytes[76] = 0x12;
    bytes[92] = 0x34;
    bytes[108] = 0x12;
    bytes[132] = 0x34;
    put_i32(&mut bytes, 148, 1);
    put_i32(&mut bytes, 152, 1);
    bytes[156] = 1;
    bytes[212] = 42;
    put_array(&mut bytes, 220, 236, 1);
    put_array(&mut bytes, 228, 240, 1);
    put_i32(&mut bytes, 236, 1);
    bytes
}

#[test]
fn blob_offsets_links_and_cycle_safe_closure() {
    let catalog = RuntimeContentCatalog::parse(&fixture()).expect("synthetic catalog");
    assert_eq!(catalog.archives.len(), 2);
    assert_eq!(catalog.files.len(), 2);
    assert_eq!(catalog.objects[0].file_index, 0);
    assert_eq!(catalog.objects[0].local_identifier, 42);
    assert_eq!(catalog.dependency_sets, vec![vec![1], vec![0]]);
    assert_eq!(catalog.dependency_closure(0).expect("closure"), vec![0, 1]);
    assert_eq!(catalog.dependency_closure(1).expect("closure"), vec![1, 0]);
    assert!(catalog.dependency_closure(2).is_err());
    assert_eq!(
        catalog.archive_name(0).as_deref(),
        Some("21000000000000000000000000000000")
    );
    assert_eq!(catalog.padding_bytes, 0);
}

#[test]
fn malformed_catalog_is_refused_before_allocation_or_indexing() {
    for (at, value) in [
        (0, 2),
        (12, 1),
        (16, 0),
        (36, i32::MAX),
        (40, -1),
        (40, i32::MAX),
        (124, 2),
        (128, 2),
        (204, 2),
        (236, 2),
    ] {
        let mut bytes = fixture();
        put_i32(&mut bytes, at, value);
        assert!(
            RuntimeContentCatalog::parse(&bytes).is_err(),
            "accepted invalid value {value} at {at}"
        );
    }
    for length in [0, 35, 36, 75, 100, 243] {
        assert!(RuntimeContentCatalog::parse(&fixture()[..length]).is_err());
    }
    let mut trailing = fixture();
    trailing.extend_from_slice(&[1, 0, 0, 0]);
    let length = trailing.len() as i32 - 36;
    put_i32(&mut trailing, 12, length);
    assert!(RuntimeContentCatalog::parse(&trailing).is_err());
}

#[test]
#[ignore = "requires the exact reference V Rising installation (set VRISING_DATA)"]
fn shipped_catalog_resolves_every_archive_and_file_dependency() {
    let data = std::env::var_os("VRISING_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../VRising_Data"));
    let directory = data.join("StreamingAssets/ContentArchives");
    let catalog = RuntimeContentCatalog::open(&directory.join("archive_dependencies.bin"))
        .expect("reference catalog is required");
    assert_eq!(catalog.archives.len(), 4410);
    assert_eq!(catalog.files.len(), 4410);
    assert_eq!(catalog.objects.len(), 1151);
    assert_eq!(catalog.scenes.len(), 0);
    assert_eq!(catalog.dependency_sets.len(), 1904);
    assert_eq!(
        catalog.dependency_sets.iter().map(Vec::len).sum::<usize>(),
        55316
    );
    assert_eq!(catalog.padding_bytes, 28);
    let archive_paths = catalog.archive_paths(&directory);
    assert_eq!(archive_paths.len(), 4409);
    assert!(!catalog.archives[4409].is_valid());
    for (i, path) in archive_paths.iter().enumerate() {
        assert!(
            path.is_file(),
            "missing catalog archive {i}: {}",
            path.display()
        );
    }
    for (i, file) in catalog.files.iter().enumerate() {
        assert_eq!(
            file.id, catalog.archives[file.archive_index],
            "file/archive ID link {i}"
        );
    }
    let largest = catalog
        .dependency_sets
        .iter()
        .map(Vec::len)
        .max()
        .expect("sets");
    let file = catalog
        .files
        .iter()
        .position(|f| catalog.dependency_sets[f.dependency_set_index].len() == largest)
        .expect("largest-set file");
    let closure = catalog
        .dependency_closure(file)
        .expect("real dependency closure");
    assert!(closure.len() > 1 && closure.len() <= catalog.files.len());
    println!("archives={} files={} objects={} scenes={} dependency_sets={} dependency_indices={} padding={} largest_dependency_set={} closure={}", catalog.archives.len(), catalog.files.len(), catalog.objects.len(), catalog.scenes.len(), catalog.dependency_sets.len(), catalog.dependency_sets.iter().map(Vec::len).sum::<usize>(), catalog.padding_bytes, largest, closure.len());
}
