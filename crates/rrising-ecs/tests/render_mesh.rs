use rrising_ecs::MaterialMeshInfo;

#[test]
fn matches_five_original_leaf_functions_for_1024_inputs() {
    let source = include_str!("../../../tests/golden/material_mesh.tsv");
    let mut cases = 0;
    for line in source
        .lines()
        .filter(|s| !s.starts_with('#') && !s.is_empty())
    {
        let p: Vec<_> = line.split('\t').collect();
        assert_eq!(p.len(), 6);
        let raw = u32::from_str_radix(p[0], 16).expect("raw");
        assert_eq!(
            MaterialMeshInfo::array_index_to_static_index(raw as i32),
            p[1].parse::<i32>().expect("static"),
            "flags {raw:x}"
        );
        assert_eq!(
            MaterialMeshInfo::static_index_to_array_index(raw as i32),
            p[2].parse::<i32>().expect("array"),
            "flags {raw:x}"
        );
        let mut info = MaterialMeshInfo {
            material: 42,
            mesh: 17,
            sub_mesh_index_info: raw,
        };
        assert_eq!(info.sub_mesh(), p[3].parse::<u16>().expect("submesh"));
        assert_eq!(info.has_material_mesh_index_range(), p[4] == "1");
        info.set_sub_mesh((raw >> 16) as u16);
        assert_eq!(
            info.sub_mesh_index_info,
            u32::from_str_radix(p[5], 16).expect("setter")
        );
        cases += 1;
    }
    assert_eq!(cases, 1024);
}

#[test]
fn index_ranges_and_static_runtime_identifiers_remain_distinct() {
    let mut info = MaterialMeshInfo::from_render_mesh_array_indices(0, 31, 0x1234);
    assert_eq!((info.material, info.mesh, info.sub_mesh()), (-1, -32, 0x34));
    assert_eq!(
        (info.material_array_index(), info.mesh_array_index()),
        (0, 31)
    );
    assert!(!info.is_runtime_material() && !info.is_runtime_mesh());
    info.set_mesh_array_index(5);
    assert_eq!(info.mesh_array_index(), 5);
    let mut range = MaterialMeshInfo::from_material_mesh_index_range(65535, 127).expect("range");
    assert_eq!(range.material_mesh_index_range(), Some((65535, 127)));
    assert!(!range.is_runtime_material() && !range.is_runtime_mesh());
    range.set_sub_mesh(1023);
    assert!(!range.has_material_mesh_index_range());
    assert_eq!(range.sub_mesh(), 255);
    assert!(MaterialMeshInfo::from_material_mesh_index_range(0, 128).is_err());
    let runtime = MaterialMeshInfo::new(8, 9, 2);
    assert!(runtime.is_runtime_mesh() && runtime.is_runtime_material());
    assert_eq!(runtime.mesh_array_index(), -1);
    assert!(MaterialMeshInfo::from_bytes(&[0; 11]).is_err());
}
