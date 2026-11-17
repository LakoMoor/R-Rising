use rrising_content::persistence::{
    PersistenceHeader, PersistentSave, SavedSnapshot, SerializationJob, SerializationJobRegistry,
};
use rrising_ecs::Entity;
use serde_json::Value;

fn number(row: &Value, field: &str) -> usize {
    usize::try_from(row[field].as_u64().unwrap_or_else(|| panic!("{field}"))).expect(field)
}

fn name<'a>(row: &'a Value, field: &str) -> &'a str {
    row[field].as_str().unwrap_or_else(|| panic!("{field}"))
}

fn assert_entity(actual: Entity, expected: &Value) {
    assert_eq!(Some(i64::from(actual.index)), expected[0].as_i64());
    assert_eq!(Some(i64::from(actual.version)), expected[1].as_i64());
}

fn assert_fingerprint(bytes: &[u8], expected: &str) {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(format!("{hash:016x}"), expected);
}

fn assert_sample_bytes(member: &[u8], actual: &[u8], offset: usize, bytes: usize) {
    assert_eq!(actual.len(), bytes);
    assert!(std::ptr::eq(actual, &member[offset..offset + bytes]));
}

fn check_scalar_sample(
    job: &SerializationJob<'_>,
    member: &[u8],
    header: &PersistenceHeader,
    sample: &Value,
) {
    let component = header
        .persistent_component_named(name(sample, "component"))
        .expect("sampled component metadata");
    let archetype = job
        .archetypes
        .iter()
        .find(|archetype| Some(archetype.offset_bits) == sample["archetype_offset_bits"].as_u64())
        .expect("sampled archetype offset");
    let chunk = archetype
        .chunks
        .iter()
        .find(|chunk| chunk.entity_array_offset == number(sample, "entity_array_offset"))
        .expect("sampled entity-array offset");
    assert_entity(
        chunk.entities().next().expect("first entity"),
        &sample["entity"],
    );
    let column = chunk
        .columns
        .iter()
        .find(|column| column.saved_type_index == component.saved_type_index)
        .expect("sampled scalar column");
    assert_eq!(column.stable_type_hash, component.stable_type_hash);
    assert_eq!(column.offset, number(sample, "offset"));
    let bytes = column.scalar(0).expect("first scalar value");
    assert_sample_bytes(member, bytes, column.offset, number(sample, "bytes"));
    assert_fingerprint(bytes, name(sample, "fnv1a64"));
}

fn check_buffer_sample(
    job: &SerializationJob<'_>,
    member: &[u8],
    header: &PersistenceHeader,
    sample: &Value,
) {
    let archetype = &job.archetypes[number(sample, "archetype_ordinal")];
    assert_eq!(
        usize::from(archetype.omission_flags),
        number(sample, "omission_flags")
    );
    let chunk = &archetype.chunks[number(sample, "chunk_ordinal")];
    let component = header
        .persistent_component_named(name(sample, "component"))
        .expect("sampled buffer metadata");
    let column = chunk
        .columns
        .iter()
        .find(|column| column.saved_type_index == component.saved_type_index)
        .expect("sampled buffer column");
    assert_eq!(column.stable_type_hash, component.stable_type_hash);
    assert!(column.scalar(0).is_none());
    let element_bytes = usize::try_from(component.component_size).expect("buffer element size");
    let buffers: Vec<_> = column.buffers().expect("buffers").collect();
    assert_eq!(buffers.len(), number(sample, "buffers"));
    assert_eq!(buffers.len(), chunk.entities().len());
    assert_eq!(
        buffers.iter().filter(|bytes| !bytes.is_empty()).count(),
        number(sample, "nonempty_buffers")
    );
    assert_eq!(
        buffers.iter().map(|bytes| bytes.len()).sum::<usize>(),
        number(sample, "total_elements") * element_bytes
    );
    for kind in ["empty", "nonempty"] {
        let ordinal = number(sample, &format!("{kind}_entity_ordinal"));
        assert_entity(
            chunk
                .entities()
                .nth(ordinal)
                .expect("sampled buffer entity"),
            &sample[format!("{kind}_entity")],
        );
        let bytes = buffers[ordinal];
        if kind == "empty" {
            assert!(bytes.is_empty());
        } else {
            assert_sample_bytes(
                member,
                bytes,
                number(sample, "nonempty_offset"),
                number(sample, "nonempty_elements") * element_bytes,
            );
            assert_fingerprint(bytes, name(sample, "nonempty_fnv1a64"));
        }
    }
}

#[test]
#[ignore = "requires two captured original server saves and RRISING_SAVE_DIR"]
fn original_save_jobs_consumed_to_eof_and_column_samples_match() {
    let root =
        std::path::PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").expect("RRISING_SAVE_DIR"));
    let golden: Value =
        serde_json::from_str(include_str!("../../../evidence/oracles/save_jobs.json"))
            .expect("captured serialization job evidence");
    let samples = golden["scalar_samples"].as_array().expect("scalar samples");
    let buffer_sample = &golden["buffer_sample"];
    for capture in golden["saves"].as_array().expect("save captures") {
        let file = name(capture, "file");
        let save = PersistentSave::open(&root.join(file)).expect("original autosave");
        let snapshot = SavedSnapshot::read(&save).expect("original saved entity index");
        let debug_slots: std::collections::HashSet<_> = snapshot
            .world_data()
            .nonpersistent_archetypes
            .as_ref()
            .unwrap()
            .iter()
            .flat_map(|archetype| &archetype.chunks)
            .flat_map(|chunk| chunk.entity_indices())
            .collect();
        assert_eq!(debug_slots.len(), 284158);
        assert!(snapshot
            .entities()
            .all(|entity| !debug_slots.contains(&(entity.index as usize))));
        assert!(snapshot.header().prefabs.iter().all(|prefab| {
            std::iter::once(&prefab.entity)
                .chain(&prefab.linked_entities)
                .all(|entity| debug_slots.contains(&(entity.index as usize)))
        }));
        let header = save.header().expect("original persistence header");
        let registry = SerializationJobRegistry::new(&header).expect("job registry");
        let steering = header
            .persistent_component_named("ProjectM.MoveTowardsPositionBuff")
            .expect("steering component in the registry");
        let jobs = capture["jobs"].as_object().expect("expected jobs");
        let entity_count: u64 = jobs.values().map(|entry| entry[3].as_u64().unwrap()).sum();
        assert_eq!(snapshot.entity_count() as u64, entity_count);
        assert_eq!(
            snapshot.entities().map(|entity| entity.index).min(),
            Some(1270)
        );
        assert_eq!(
            snapshot.entities().map(|entity| entity.index).max(),
            Some(if file == "AutoSave_4.save.gz" {
                559509
            } else {
                559503
            })
        );
        let view = snapshot
            .entity(Entity::new(152830, 4))
            .expect("sampled entity");
        assert!(snapshot.entity(Entity::new(152830, 3)).is_none());
        assert!(snapshot.entity(Entity::NULL).is_none());
        for sample in samples {
            let component = header
                .persistent_component_named(name(sample, "component"))
                .unwrap();
            let bytes = view.scalar(component.saved_type_index).unwrap();
            assert_fingerprint(bytes, name(sample, "fnv1a64"));
        }
        let mut observed_jobs = 0;
        for (index, entry) in save.members().iter().enumerate() {
            if !entry.name.starts_with("SerializationJob_") {
                continue;
            }
            let expected = jobs[&entry.name]
                .as_array()
                .expect("captured job statistics");
            let bytes = save.member_data(index).expect("original job bytes");
            let job = SerializationJob::read(bytes, &registry)
                .unwrap_or_else(|err| panic!("{file}/{}: {err:#}", entry.name));
            let stats = job.stats();
            let actual = [
                bytes.len(),
                stats.archetypes,
                stats.chunks,
                stats.entities,
                stats.columns,
                stats.buffer_entities,
            ]
            .map(|value| Value::from(value as u64));
            assert_eq!(actual.as_slice(), expected, "{file}/{}", entry.name);
            assert!(
                job.archetypes
                    .iter()
                    .flat_map(|archetype| &archetype.chunks)
                    .flat_map(|chunk| &chunk.columns)
                    .all(|column| column.saved_type_index != steering.saved_type_index),
                "{file}/{} unexpectedly contained active steering state",
                entry.name
            );
            for sample in samples
                .iter()
                .filter(|sample| name(sample, "job") == entry.name)
            {
                check_scalar_sample(&job, bytes, &header, sample);
            }
            if name(buffer_sample, "job") == entry.name {
                check_buffer_sample(&job, bytes, &header, buffer_sample);
            }
            println!("{file}/{}: {stats:?}", entry.name);
            observed_jobs += 1;
        }
        assert_eq!(observed_jobs, jobs.len(), "{file}");
    }
}
