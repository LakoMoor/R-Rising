use rrising_content::tile_blob::DenseCollisionTiles;
#[test]
fn dense_arrays_are_relative_and_bounded() {
    let mut blob = vec![0; 0x194];
    blob[0x170..0x174].copy_from_slice(&32i32.to_le_bytes());
    blob[0x174..0x178].copy_from_slice(&2i32.to_le_bytes());
    blob[0x178..0x17c].copy_from_slice(&26i32.to_le_bytes());
    blob[0x17c..0x180].copy_from_slice(&2i32.to_le_bytes());
    blob[0x190..].copy_from_slice(&[1, 32, 1, 0]);
    let tiles = DenseCollisionTiles::read(&blob).unwrap();
    assert_eq!(tiles.flags, [1, 32]);
    assert_eq!(tiles.exists, [1, 0]);
    assert!(DenseCollisionTiles::read(&blob[..0x193]).is_err());
    blob[0x174..0x178].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(DenseCollisionTiles::read(&blob).is_err());
}
