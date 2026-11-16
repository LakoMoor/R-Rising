//! Parses every `DOTSBIN!` scene file of the real install.
//!
//! Skipped when the game folder is absent, like the other real-content tests. The counts and the
//! fingerprint it checks are recorded in `evidence/entity_scenes_fingerprint.txt`.

use std::path::{Path, PathBuf};

use rrising_content::entity_scenes::{
    ARCHETYPE_TYPE_INDEX_MASK, BLOB_ASSET_ALIGNMENT, FILE_VERSION, FileHeader, MAGIC,
    NODE_HEADER_SIZE, NODE_TYPE_FOLDER, NodeKind, SceneFile, SceneStream, id_bytes,
};
use rrising_ecs::Hash128;

fn scenes_dir() -> PathBuf {
    PathBuf::from("/Users/lakomoor/ported/VRising/VRising_Data/StreamingAssets/EntityScenes")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Collect the `.entities` files, sorted so the fingerprint does not depend on readdir order.
fn scene_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("entities"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[test]
fn every_scene_file_is_a_dots_serialization_v77_container() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }

    let mut parsed = 0usize;
    let mut unreadable = Vec::new();
    let mut bad = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut data_bytes = 0u64;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            unreadable.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            continue;
        };
        match SceneFile::parse(bytes) {
            Ok(f) => {
                parsed += 1;
                let h = &f.header;
                if h.magic != MAGIC || h.file_version != FILE_VERSION {
                    bad.push(format!("{}: header {:#x} v{}", path.display(), h.magic, h.file_version));
                }
                // The sections have to tile the file exactly, in the order data, metadata, nodes.
                if h.total_size() as u64 != f.bytes().len() as u64 {
                    bad.push(format!(
                        "{}: header + {} + {} + {} = {}, file is {}",
                        path.display(),
                        h.data_section_size,
                        h.metadata_section_size,
                        h.nodes_section_size,
                        h.total_size(),
                        f.bytes().len()
                    ));
                }
                let mut blob_sizes: Vec<i64> = f.blobs().map(|n| n.header.data_size).collect();
                blob_sizes.sort();
                let covered: i64 = blob_sizes.iter().sum();
                data_bytes += covered as u64;
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                lines.push(format!(
                    "{name}\tnodes={}\troots={}\tblobs={}\tblob_sizes={:?}\tcovered={}\tdata={}\tfnv={:016x}",
                    f.nodes.len(),
                    f.roots.len(),
                    blob_sizes.len(),
                    blob_sizes,
                    covered,
                    h.data_section_size,
                    fnv1a64(&f.bytes()[h.data_section_offset as usize
                        ..h.data_section_offset as usize + h.data_section_size as usize]),
                ));
            }
            Err(e) => bad.push(format!("{}: {e}", path.display())),
        }
    }

    println!(
        "scene files: {} found, {} parsed, {} unreadable, {} rejected",
        files.len(),
        parsed,
        unreadable.len(),
        bad.len()
    );
    println!("data bytes covered by node blobs: {data_bytes}");
    if !unreadable.is_empty() {
        println!("unreadable (first 5): {:?}", &unreadable[..unreadable.len().min(5)]);
    }
    for b in bad.iter().take(5) {
        println!("rejected: {b}");
    }
    assert_eq!(bad, Vec::<String>::new(), "scene files this reader could not accept");
    assert_eq!(
        parsed,
        files.len() - unreadable.len(),
        "every readable scene file has to parse"
    );

    let joined = lines.join("\n");
    let got = format!(
        "files={}\nparsed={}\nnode_blobs_bytes={}\nfnv1a64={:016x}\n",
        files.len(),
        parsed,
        data_bytes,
        fnv1a64(joined.as_bytes())
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_scenes_fingerprint.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    // The record file carries its own recipe as `#` comments; only the value lines are compared.
    let want: String = std::fs::read_to_string(&record)
        .expect("read the fingerprint record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.trim(), want.trim(), "the scene index moved away from the recorded fingerprint");
}

#[test]
fn node_tree_framing_holds_on_the_largest_and_smallest_scene_files() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.len() < 2 {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }
    let mut sized: Vec<(u64, PathBuf)> = files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| (m.len(), p.clone())))
        .collect();
    sized.sort();
    let picks = [sized[0].1.clone(), sized[sized.len() / 2].1.clone(), sized[sized.len() - 1].1.clone()];

    for path in picks {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("{} unreadable this run, skipping", path.display());
            continue;
        };
        let len = bytes.len();
        let f = SceneFile::parse(bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let h = &f.header;
        let nodes_start = h.nodes_section_offset as usize;

        // The root of every scene file is the 72-byte folder node the dump describes.
        assert_eq!(f.roots.len(), 1, "{}: one first-level node", path.display());
        let root = &f.nodes[f.roots[0]];
        assert_eq!(root.header.node_type_hash, NODE_TYPE_FOLDER, "{}: root is a folder", path.display());
        assert_eq!(root.header.size, NODE_HEADER_SIZE as i32, "{}: root is a bare header", path.display());
        assert_eq!(root.header.data_size, 0, "{}: the root carries no data", path.display());

        // Children start right after the parent and chain to the end of the nodes section.
        let mut expect = nodes_start + root.header.size as usize;
        for (i, idx) in root.children.iter().enumerate() {
            let c = &f.nodes[*idx];
            assert_eq!(c.offset, expect, "{}: child {i} is not where the previous one ends", path.display());
            expect = c.offset + c.header.size as usize;
        }
        assert_eq!(
            expect,
            nodes_start + h.nodes_section_size as usize,
            "{}: the child chain does not fill the nodes section",
            path.display()
        );

        // Data blobs are addressed by file offset and start at the data section.
        let blobs: Vec<&_> = f.blobs().collect();
        assert_eq!(
            blobs[0].header.data_starting_offset,
            h.data_section_offset,
            "{}: the first blob does not start the data section",
            path.display()
        );
        let covered: i64 = blobs.iter().map(|b| b.header.data_size).sum();
        assert!(
            covered <= h.data_section_size,
            "{}: blobs cover {covered} bytes of a {}-byte data section",
            path.display(),
            h.data_section_size
        );
        println!(
            "{}: {len} bytes, {} nodes, {} blobs, {} of {} data bytes covered",
            path.file_name().unwrap_or_default().to_string_lossy(),
            f.nodes.len(),
            blobs.len(),
            covered,
            h.data_section_size
        );
    }
}

/// The twelve `SerializeUtility` stream tags, as the literals `SerializeUtility..cctor` @ 0x8fef290
/// actually holds, paired with the field they are assigned to.
const STREAM_TAGS: &[(&str, &str)] = &[
    ("WorldFileType", "7F090F7311DF4BA5BA3094AA33D0FAA7"),
    ("WorldNodeType", "E9FDFDEB4775443ABB236D4858232894"),
    ("DebugSectionNodeType", "582BAFE96EC44A72ACD9CCBC850FDBC4"),
    ("TypesNameStringTableNodeType", "4847374145504072849AAC7B1921E7FD"),
    ("TypesNameNodeType", "828510FAD38F4EA78CD908D4D780388B"),
    ("ArchetypesNodeType", "F5364E1CCB62466A9883F6F9554D4F0C"),
    ("SharedAndManagedComponentsNodeType", "D565355C5CF34C0DBBD4A06ADDA948B1"),
    ("EnabledBitsNodeType", "5846E500EA614C1DA94AFB85AFD8F4F4"),
    ("BlobAssetsNodeType", "9A26954FF1ED4CC5A64E8AAD4F64773A"),
    ("ChunksNodeType", "2EA7CE3325F04D7A84CEAB46790C628A"),
    ("BufferDataNodeType", "E33124BFAC2649D792DE36E4611DDD70"),
    ("PrefabNodeType", "2A84A183583A4FAD8CB7105AE3C47598"),
];

/// `Hash128.StringToHash` @ 0x8fcb550 reads each group of eight hex characters back to front, so
/// the bytes it leaves behind are the literal's bytes with the two nibbles of each byte swapped.
/// That is the only reason the shipped scene files' node ids mean anything at all, so both halves
/// of it are checked here: the ported parser against the rule, and the rule against the files.
#[test]
fn stream_tags_survive_the_ported_hash128_parser() {
    for (field, guid) in STREAM_TAGS {
        let parsed = Hash128::from_guid(guid);
        assert!(parsed.is_valid(), "{field}: {guid} parsed to the zero hash");
        assert_eq!(
            parsed.to_bytes(),
            id_bytes(guid),
            "{field}: the parser and the nibble-swap rule disagree on {guid}"
        );
    }

    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }

    // Every tag the files actually use has to be one the ported parser reproduces.
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut named = 0usize;
    let mut unnamed = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(f) = SceneFile::parse(bytes) else { continue };
        for n in f.blobs() {
            let key = hex(&n.header.id);
            *seen.entry(key).or_insert(0) += 1;
            match n.kind {
                NodeKind::RevisionedRawData { stream: Some(s), .. } => {
                    assert_eq!(
                        n.header.id,
                        Hash128::from_guid(tag_guid(s)).to_bytes(),
                        "{}: stream {s:?} id does not come from its literal",
                        path.display()
                    );
                    named += 1;
                }
                other => unnamed.push(format!("{}: {other:?}", path.display())),
            }
        }
    }
    println!("stream ids seen across {} files: {seen:?}", files.len());
    assert_eq!(unnamed, Vec::<String>::new(), "a scene node this reader could not name");
    assert_eq!(named, 6 * files.len(), "every scene file carries six named streams");
}

/// The literal `SerializeUtility..cctor` assigns to the field a `SceneStream` names after.
fn tag_guid(stream: SceneStream) -> &'static str {
    match stream {
        SceneStream::Archetypes => "F5364E1CCB62466A9883F6F9554D4F0C",
        SceneStream::BlobAssets => "9A26954FF1ED4CC5A64E8AAD4F64773A",
        SceneStream::SharedAndManagedComponents => "D565355C5CF34C0DBBD4A06ADDA948B1",
        SceneStream::EnabledBits => "5846E500EA614C1DA94AFB85AFD8F4F4",
        SceneStream::Chunks => "2EA7CE3325F04D7A84CEAB46790C628A",
        SceneStream::BufferData => "E33124BFAC2649D792DE36E4611DDD70",
        SceneStream::Prefab => "2A84A183583A4FAD8CB7105AE3C47598",
        SceneStream::TypeNames => "828510FAD38F4EA78CD908D4D780388B",
        SceneStream::TypeNamesStringTable => "4847374145504072849AAC7B1921E7FD",
        SceneStream::DebugSection => "582BAFE96EC44A72ACD9CCBC850FDBC4",
    }
}

fn hex(b: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}
/// The header of a file that is not a scene container has to be refused, not mis-parsed.
#[test]
fn a_foreign_header_is_refused() {
    let mut buf = vec![0u8; 152];
    buf[0..8].copy_from_slice(b"NOTMAGIC");
    assert!(FileHeader::read(&buf).is_err());
    assert!(FileHeader::read(&buf[..100]).is_err());
}

/// Decodes the `Archetypes` stream out of every scene file of the install.
///
/// The stream is the archetype list the game rebuilds on load, so this is the first piece of a
/// `.entities` file whose *meaning* — not just whose framing — is checked against the real
/// content. Two independent halves of the file have to agree: the `EntityCount` the stream
/// records per archetype, and the `CountForSerialization` of that file's own `Chunks` stream.
/// The recorded totals are in `evidence/entity_archetypes.txt`.
#[test]
fn archetypes_stream_decodes_from_every_scene_file() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut distinct_hashes: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    let mut rejected = Vec::new();
    let mut with_stream = 0usize;
    let mut archetypes_total = 0usize;
    let mut unique_total = 0usize;
    let mut chunk_component_total = 0usize;
    let mut entities_total = 0i64;
    let mut chunk_entities_total = 0i64;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(file) = SceneFile::parse(bytes) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let arch = match file.archetypes() {
            Ok(a) => a,
            Err(e) => {
                rejected.push(format!("{name}: {e}"));
                continue;
            }
        };
        if file.stream(SceneStream::Archetypes).is_none() {
            rejected.push(format!("{name}: no Archetypes node"));
            continue;
        }
        with_stream += 1;

        let blob_hash = file
            .stream(SceneStream::Archetypes)
            .map(|n| fnv1a64(&n.data))
            .unwrap_or(0);

        let mut chunk_component = 0usize;
        let mut max_types = 0usize;
        for a in &arch.archetypes {
            max_types = max_types.max(a.types.len());
            for t in &a.types {
                if t.chunk_component {
                    chunk_component += 1;
                }
                assert!(
                    usize::from(t.unique_index) < arch.unique_type_hashes.len(),
                    "{name}: unique type index {} is past the {} hashes the stream lists",
                    t.unique_index,
                    arch.unique_type_hashes.len()
                );
                assert_eq!(t.to_raw() & ARCHETYPE_TYPE_INDEX_MASK, u32::from(t.unique_index));
            }
        }
        distinct_hashes.extend(arch.unique_type_hashes.iter().copied());

        let chunk_entities: i64 =
            file.chunks().map(|c| c.iter().map(|ch| i64::from(ch.header.count)).sum()).unwrap_or(0);
        let entities = arch.total_entity_count();

        archetypes_total += arch.archetypes.len();
        unique_total += arch.unique_type_hashes.len();
        chunk_component_total += chunk_component;
        entities_total += entities;
        chunk_entities_total += chunk_entities;

        lines.push(format!(
            "{name}\ttypes={}\tarch={}\tents={}\tchunk_ents={}\ten={}\tmaxtc={}\thash={blob_hash:016x}",
            arch.unique_type_hashes.len(),
            arch.archetypes.len(),
            entities,
            chunk_entities,
            chunk_component,
            max_types
        ));
    }

    println!(
        "archetypes stream: {} files carry it, {} archetypes, {} unique type entries, \
         {} distinct stable hashes, {} chunk-component slots, {} entities",
        with_stream,
        archetypes_total,
        unique_total,
        distinct_hashes.len(),
        chunk_component_total,
        entities_total
    );
    for r in rejected.iter().take(5) {
        println!("rejected: {r}");
    }
    assert_eq!(rejected, Vec::<String>::new(), "archetypes streams this reader could not accept");
    assert_eq!(with_stream, files.len(), "every scene file has to carry an Archetypes node");
    assert_eq!(
        entities_total, chunk_entities_total,
        "the archetype list and the chunk blocks have to describe the same number of entities"
    );

    let joined = lines.join("\n");
    let hash_list: Vec<String> = distinct_hashes.iter().map(|h| format!("{h:016x}")).collect();
    let got = format!(
        "files={with_stream}\n\
         archetypes={archetypes_total}\n\
         unique_type_entries={unique_total}\n\
         distinct_stable_hashes={}\n\
         chunk_component_slots={chunk_component_total}\n\
         entities={entities_total}\n\
         chunk_entities={chunk_entities_total}\n\
         fnv1a64_lines={:016x}\n\
         fnv1a64_hashes={:016x}\n",
        distinct_hashes.len(),
        fnv1a64(joined.as_bytes()),
        fnv1a64(hash_list.join(",").as_bytes())
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_archetypes.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded archetypes fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    let want: String = std::fs::read_to_string(&record)
        .expect("read the archetypes record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.trim(), want.trim(), "the archetypes stream moved away from the record");
}

/// The `BufferData` stream, checked against the chunk blocks it exists to patch.
///
/// A `DynamicBuffer`'s chunk data is only a pointer, and
/// `SerializeUtility.FillBufferPatchRecordsAndClearBufferPointer` @ 0x8fee550 zeroes that pointer
/// before `WriteChunks` @ 0x8fe65e0 writes the block out — so the contents have to travel
/// separately, and this stream is where they travel. The check that the records really address
/// those blocks is that every `ChunkOffset` lands on eight zero bytes at
/// `block + Chunk.kBufferOffset + ChunkOffset`: the cleared pointer. The recorded totals are in
/// `evidence/entity_buffer_data.txt`.
#[test]
fn buffer_data_stream_decodes_from_every_scene_file() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut rejected = Vec::new();
    let mut with_stream = 0usize;
    let mut groups_total = 0usize;
    let mut patches_total = 0usize;
    let mut bytes_total = 0usize;
    let mut files_with_buffers = 0usize;
    let mut cleared_slots = 0usize;
    let mut max_offset = 0i32;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(file) = SceneFile::parse(bytes) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Some(node) = file.stream(SceneStream::BufferData) else {
            rejected.push(format!("{name}: no BufferData node"));
            continue;
        };
        let blob_hash = fnv1a64(&node.data);
        let chunks = match file.chunks() {
            Ok(c) => c,
            Err(e) => {
                rejected.push(format!("{name}: chunks: {e}"));
                continue;
            }
        };
        let bd = match file.buffer_data() {
            Ok(b) => b,
            Err(e) => {
                rejected.push(format!("{name}: {e}"));
                continue;
            }
        };
        with_stream += 1;
        assert_eq!(
            bd.total_chunk_count as usize,
            chunks.len(),
            "{name}: the stream counts {} chunks, the chunks stream holds {}",
            bd.total_chunk_count,
            chunks.len()
        );

        let mut npatches = 0usize;
        let mut nbytes = 0usize;
        let mut prev_archetype = -1i32;
        for (g, group) in bd.per_chunk.iter().enumerate() {
            let chunk = &chunks[g];
            assert_eq!(group.chunk_index, g, "{name}: patch group {g} is not for chunk {}", chunk.index);
            assert!(
                chunk.header.archetype_index >= prev_archetype,
                "{name}: chunk {} of archetype {} came after archetype {}, so the stream is not \
                 grouped the way WriteChunks @ 0x8fe65e0 writes it",
                chunk.index,
                chunk.header.archetype_index,
                prev_archetype
            );
            prev_archetype = chunk.header.archetype_index;
            let mut prev_offset = -1i32;
            for p in &group.patches {
                let claimed = usize::try_from(p.alloc_size_bytes).unwrap_or(usize::MAX);
                assert_eq!(
                    claimed,
                    p.data.len(),
                    "{name}: chunk {} patch at {} claims {} bytes and the stream holds {}",
                    chunk.index,
                    p.chunk_offset,
                    p.alloc_size_bytes,
                    p.data.len()
                );
                assert!(
                    p.chunk_offset > prev_offset,
                    "{name}: chunk {} patches do not ascend: {} follows {}",
                    chunk.index,
                    p.chunk_offset,
                    prev_offset
                );
                prev_offset = p.chunk_offset;
                let at = usize::try_from(p.chunk_offset).unwrap_or(usize::MAX);
                assert!(
                    at + 8 <= chunk.buffer.len(),
                    "{name}: chunk {} patch at {at} does not fit in the {} bytes of chunk buffer",
                    chunk.index,
                    chunk.buffer.len()
                );
                assert_eq!(
                    &chunk.buffer[at..at + 8],
                    &[0u8; 8],
                    "{name}: chunk {} patch at {} does not land on a cleared buffer pointer",
                    chunk.index,
                    p.chunk_offset
                );
                cleared_slots += 1;
                max_offset = max_offset.max(p.chunk_offset);
                npatches += 1;
                nbytes += p.data.len();
            }
        }
        assert_eq!(bd.patch_count(), npatches);
        assert_eq!(bd.total_data_bytes(), nbytes);

        groups_total += bd.per_chunk.len();
        patches_total += npatches;
        bytes_total += nbytes;
        if npatches > 0 {
            files_with_buffers += 1;
        }
        lines.push(format!(
            "{name}\tbd={}\tchunks={}\tpatches={npatches}\tbytes={nbytes}\thash={blob_hash:016x}",
            node.header.data_size,
            bd.total_chunk_count
        ));
    }

    println!(
        "buffer data stream: {} files carry it, {} chunk groups, {} patches, {} bytes of buffer \
         contents, {} cleared pointer slots accounted for",
        with_stream, groups_total, patches_total, bytes_total, cleared_slots
    );
    for r in rejected.iter().take(5) {
        println!("rejected: {r}");
    }
    assert_eq!(rejected, Vec::<String>::new(), "buffer data streams this reader could not accept");
    assert_eq!(with_stream, files.len(), "every scene file has to carry a BufferData node");
    assert_eq!(
        cleared_slots, patches_total,
        "every patch has to account for one cleared buffer pointer"
    );

    let joined = lines.join("\n");
    let got = format!(
        "files={with_stream}\n\
         files_with_buffer_data={files_with_buffers}\n\
         chunk_groups={groups_total}\n\
         patches={patches_total}\n\
         buffer_bytes={bytes_total}\n\
         cleared_pointer_slots_verified={cleared_slots}\n\
         max_chunk_offset={max_offset}\n\
         fnv1a64_lines={:016x}\n",
        fnv1a64(joined.as_bytes())
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_buffer_data.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded buffer data fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    let want: String = std::fs::read_to_string(&record)
        .expect("read the buffer data record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.trim(), want.trim(), "the buffer data stream moved away from the record");
}

/// The `EnabledBits` stream, checked against the archetypes and chunk blocks it describes.
///
/// src: `SerializeUtility.WriteEnabledBits` @ 0x8fe7230 sizes two regions by walking the archetype
/// array — `Archetype.Chunks.ComponentCount * Archetype.Chunks.Count * 16`, then the same product
/// times `4` (`ArchetypeChunkData` keeps `Count` at `+0xc` and `ComponentCount` at `+0x14`) —
/// writes each total as an `int` and then emits that many bytes, one archetype after another. The
/// 16-byte unit is `ArchetypeChunkData.ComponentEnabledBits` for one (chunk, component) pair, an
/// `UnsafeBitArray` of the chunk's entity count over it (`GetEnabledArrayForTypeInChunk`
/// @ 0x911d6a0), and the `int` is the matching `ComponentEnabledBitsHierarchicalData` entry.
///
/// `ComponentCount` is the archetype's whole `TypesCount`, so the walk has to add back the
/// `SharedComponentData` entry at `Types[0]` the `Archetypes` stream leaves out — the recorded
/// identity `DataSize == 8 + 20 × units` over all 1183 files is what pins that `+1`. The recorded
/// totals are in `evidence/entity_enabled_bits.txt`.
#[test]
fn enabled_bits_stream_decodes_from_every_scene_file() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        println!("no scene files under {}, skipping", dir.display());
        return;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut rejected = Vec::new();
    let mut with_stream = 0usize;
    let mut units_total = 0usize;
    let mut mask_total = 0usize;
    let mut hier_total = 0usize;
    let mut within = 0usize;
    let mut all_enabled = 0usize;
    let mut max_count = 0i32;
    let mut both_empty = 0usize;
    let mut disabled_nonzero = 0usize;
    let mut disabled_consistent = 0usize;
    let mut chunk_component_units = 0usize;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(file) = SceneFile::parse(bytes) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Some(node) = file.stream(SceneStream::EnabledBits) else {
            rejected.push(format!("{name}: no EnabledBits node"));
            continue;
        };
        let blob_hash = fnv1a64(&node.data);
        let eb = match file.enabled_bits() {
            Ok(e) => e,
            Err(e) => {
                rejected.push(format!("{name}: {e}"));
                continue;
            }
        };
        let arch = match file.archetypes() {
            Ok(a) => a,
            Err(e) => {
                rejected.push(format!("{name}: archetypes: {e}"));
                continue;
            }
        };
        let chunks = match file.chunks() {
            Ok(c) => c,
            Err(e) => {
                rejected.push(format!("{name}: chunks: {e}"));
                continue;
            }
        };
        with_stream += 1;

        let units = eb.units();
        let implied: usize = eb
            .chunk_counts
            .iter()
            .zip(eb.component_counts.iter())
            .map(|(c, m)| c * m)
            .sum();
        assert_eq!(units, implied, "{name}: {} masks for a walk that implies {implied}", eb.masks.len());
        assert_eq!(
            8 + units * 16 + units * 4,
            node.header.data_size as usize,
            "{name}: {} units make {} bytes, the node holds {}",
            units,
            8 + units * 20,
            node.header.data_size
        );
        assert_eq!(eb.hierarchical.len(), units, "{name}: hierarchical data out of step with masks");

        // The writer walks the archetype array, and inside an archetype that archetype's chunks in
        // the order WriteChunks @ 0x8fe65e0 put them, so collecting the blocks per archetype in
        // stream order reproduces the unit order.
        let mut counts: Vec<Vec<i32>> = vec![Vec::new(); arch.archetypes.len()];
        for c in &chunks {
            let Some(v) = counts.get_mut(usize::try_from(c.header.archetype_index).unwrap_or(usize::MAX)) else {
                rejected.push(format!(
                    "{name}: chunk {} names archetype {}, of which there are {}",
                    c.index,
                    c.header.archetype_index,
                    counts.len()
                ));
                continue;
            };
            v.push(c.header.count);
        }

        let mut unit = 0usize;
        for (ai, components) in eb.component_counts.iter().enumerate() {
            assert_eq!(
                counts[ai].len(),
                eb.chunk_counts[ai],
                "{name}: archetype {ai} has {} chunk blocks, the stream counted {}",
                counts[ai].len(),
                eb.chunk_counts[ai]
            );
            assert_eq!(
                *components,
                arch.archetypes[ai].types.len() + 1,
                "{name}: archetype {ai} has {} component slots, its archetype lists {} plus Types[0]",
                components,
                arch.archetypes[ai].types.len()
            );
            for (ordinal, count) in counts[ai].iter().enumerate() {
                max_count = max_count.max(*count);
                let width = u32::try_from(*count).unwrap_or(u32::MAX);
                for slot in 0..*components {
                    let m = eb.masks[unit];
                    unit += 1;
                    let v: u128 = ((m.high as u128) << 64) | (m.low as u128);
                    if width >= 128 {
                        within += 1;
                        if v == u128::MAX {
                            all_enabled += 1;
                        }
                    } else {
                        let live: u128 = (1u128 << width) - 1;
                        if v & !live == 0 {
                            within += 1;
                        }
                        if v == live {
                            all_enabled += 1;
                        }
                    }
                    // The `int` beside the mask is how many of the chunk's entities the component
                    // is disabled for (`GetChunkDisabledCountForType` @ 0x911db10,
                    // `AddExistingChunk` @ 0x8fdebc0's `perComponentDisabledBitCount`), so the two
                    // can never add up to more than the chunk holds, and where the store keeps the
                    // pair live they add up to exactly the entity count.
                    let index = unit - 1;
                    let set = u64::from(m.enabled_count());
                    let disabled = eb.hierarchical[index];
                    assert!(
                        disabled >= 0 && set + u64::try_from(disabled).unwrap_or(u64::MAX)
                            <= u64::from(width),
                        "{name}: unit {index} reports {set} enabled and {disabled} disabled for a                          chunk holding {count} entities"
                    );
                    if disabled > 0 {
                        disabled_nonzero += 1;
                        assert_eq!(
                            set + u64::try_from(disabled).unwrap_or(u64::MAX),
                            u64::from(width),
                            "{name}: unit {index} carries a disabled count without the matching                              enabled bits"
                        );
                        disabled_consistent += 1;
                    }
                    if set == 0 && disabled == 0 {
                        both_empty += 1;
                    }
                    // A chunk component has no per-entity storage at all, so the store never
                    // touches its mask or its disabled count.
                    if slot > 0 && arch.archetypes[ai].types[slot - 1].chunk_component {
                        chunk_component_units += 1;
                        assert_eq!(
                            m.low | m.high,
                            0,
                            "{name}: unit {index} is a chunk component with enabled bits"
                        );
                        assert_eq!(
                            disabled, 0,
                            "{name}: unit {index} is a chunk component with a disabled count"
                        );
                    }
                    assert_eq!(
                        eb.mask_for(ai, ordinal, slot),
                        Some(m),
                        "{name}: mask_for disagrees with the flat walk at unit {unit}"
                    );
                }
            }
        }
        assert_eq!(unit, units, "{name}: the walk covered {unit} units, the stream holds {units}");

        units_total += units;
        mask_total += units * 16;
        hier_total += units * 4;
        lines.push(format!(
            "{name}\ted={}\tunits={units}\tmask_bytes={}\thier_bytes={}\thash={blob_hash:016x}",
            node.header.data_size,
            units * 16,
            units * 4
        ));
    }

    println!(
        "enabled bits stream: {} files carry it, {units_total} (chunk, component) units, \
         {mask_total} bytes of masks and {hier_total} bytes of hierarchical data, {within} masks \
         inside their chunk's entity count, {all_enabled} with every entity enabled, {both_empty} \
         with neither, {disabled_nonzero} carrying a disabled count (all {disabled_consistent} of \
         them adding up with the mask to the chunk's entity count), {chunk_component_units} units \
         of chunk components all zero, the fullest chunk holding {max_count} entities",
        with_stream
    );
    for r in rejected.iter().take(5) {
        println!("rejected: {r}");
    }
    assert_eq!(rejected, Vec::<String>::new(), "enabled bits streams this reader could not accept");
    assert_eq!(with_stream, files.len(), "every scene file has to carry an EnabledBits node");
    assert_eq!(within, units_total, "every mask has to stay inside its chunk's entity count");
    assert_eq!(
        disabled_consistent, disabled_nonzero,
        "a disabled count has to be matched by the entities the mask leaves enabled"
    );

    let joined = lines.join("\n");
    let got = format!(
        "files={with_stream}\n\
         units={units_total}\n\
         mask_bytes={mask_total}\n\
         hierarchical_bytes={hier_total}\n\
         masks_within_entity_count={within}\n\
         masks_all_enabled={all_enabled}\n\
         masks_and_counts_both_empty={both_empty}\n\
         disabled_counts_nonzero={disabled_nonzero}\n\
         disabled_counts_match_masks={disabled_consistent}\n\
         chunk_component_units={chunk_component_units}\n\
         max_entities_in_a_chunk={max_count}\n\
         fnv1a64_lines={:016x}\n",
        fnv1a64(joined.as_bytes())
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_enabled_bits.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded enabled bits fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    let want: String = std::fs::read_to_string(&record)
        .expect("read the enabled bits record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.trim(), want.trim(), "the enabled bits stream moved away from the record");
}

/// The `SharedAndManagedComponents` stream, checked against the archetype type table it names.
///
/// Shared component values and managed objects are the two things a chunk block cannot carry
/// inline: a shared component travels as a value the chunks only index, and a managed component
/// travels as an object the chunks reference by a number `WriteChunks` @ 0x8fe65e0 assigns. Every
/// record in this stream names its component by stable type hash, so the check that the records
/// really belong to the file is that each hash also appears in that file's `Archetypes` unique type
/// list — a list built from a completely different node. The recorded totals are in
/// `evidence/entity_shared_and_managed_components.txt`.
#[test]
fn shared_and_managed_components_stream_decodes_from_every_scene_file() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    if files.is_empty() {
        eprintln!("no scene files under {}", dir.display());
        return;
    }

    let mut with_stream = 0usize;
    let mut node_bytes = 0u64;
    let mut remap_entries = 0usize;
    let mut remap_nonzero = 0usize;
    let mut remap_max = 0i32;
    let mut unmanaged_total = 0usize;
    let mut unmanaged_bytes = 0u64;
    let mut managed_shared_total = 0usize;
    let mut managed_blob_bytes = 0u64;
    let mut managed_components = 0i64;
    let mut hashes_in_archetype_table = 0usize;
    let mut first_component_hash_checked = 0usize;
    let mut remap_surjective = 0usize;

    let mut lines: Vec<String> = Vec::new();
    let mut distinct_hashes: Vec<u64> = Vec::new();

    for path in &files {
        let file = SceneFile::open(path).expect("open the scene file");
        let Some(node) = file.stream(SceneStream::SharedAndManagedComponents) else { continue };
        with_stream += 1;
        node_bytes += node.header.data_size as u64;

        let arch = file.archetypes().expect("decode the archetypes stream");
        let table: std::collections::HashSet<u64> =
            arch.unique_type_hashes.iter().copied().collect();

        let smc = file
            .shared_and_managed_components()
            .expect("decode the shared and managed components stream");

        // Every record has to name a component type this file's archetypes actually list.
        for c in &smc.unmanaged {
            assert!(
                table.contains(&c.stable_type_hash),
                "{}: unmanaged shared component {:016x} is not in the archetype type table",
                path.display(),
                c.stable_type_hash
            );
            hashes_in_archetype_table += 1;
            if !distinct_hashes.contains(&c.stable_type_hash) {
                distinct_hashes.push(c.stable_type_hash);
            }
        }
        for r in &smc.managed_shared_records {
            assert!(
                table.contains(&r.stable_type_hash),
                "{}: managed shared component {:016x} is not in the archetype type table",
                path.display(),
                r.stable_type_hash
            );
            assert_eq!(
                r.component_size,
                rrising_content::entity_scenes::SHARED_COMPONENT_MANAGED_SIZE,
                "{}: managed shared component {:016x} carries a byte size, which the writer only \
                 stores for unmanaged values",
                path.display(),
                r.stable_type_hash
            );
            hashes_in_archetype_table += 1;
            if !distinct_hashes.contains(&r.stable_type_hash) {
                distinct_hashes.push(r.stable_type_hash);
            }
        }

        // The blob's first managed component, when there is no managed shared object ahead of it.
        if smc.managed_shared_records.is_empty() && smc.managed_component_count > 0 {
            let hash = u64::from_le_bytes(
                smc.managed_blob
                    .get(..8)
                    .expect("a managed component needs its type hash")
                    .try_into()
                    .expect("eight bytes"),
            );
            assert!(
                table.contains(&hash),
                "{}: the managed blob opens with type hash {:016x}, which the archetype type table \
                 does not list",
                path.display(),
                hash
            );
            first_component_hash_checked += 1;
            if !distinct_hashes.contains(&hash) {
                distinct_hashes.push(hash);
            }
        }

        // The remap table is a surjection onto the serialized list: every serialized shared component
        // is referenced, and no entry points past it.
        let serialized = smc.serialized_shared_component_count();
        let reached: std::collections::BTreeSet<usize> = smc
            .shared_component_remap
            .iter()
            .filter(|&&v| v != 0)
            .map(|&v| usize::try_from(v - 1).expect("a positive index"))
            .collect();
        let expected: std::collections::BTreeSet<usize> = (0..serialized).collect();
        assert_eq!(
            reached, expected,
            "{}: the remap table reaches {:?}, the stream serializes {serialized} shared components",
            path.display(),
            reached
        );
        remap_surjective += 1;

        remap_entries += smc.shared_component_remap.len();
        remap_nonzero += smc.shared_component_remap.iter().filter(|&&v| v != 0).count();
        remap_max = remap_max.max(smc.shared_component_remap.iter().copied().max().unwrap_or(0));
        unmanaged_total += smc.unmanaged.len();
        unmanaged_bytes += smc.total_unmanaged_bytes() as u64;
        managed_shared_total += smc.managed_shared_records.len();
        managed_blob_bytes += smc.managed_blob.len() as u64;
        managed_components += i64::from(smc.managed_component_count);

        // The six length fields, the two record tables and the two value regions are the whole
        // stream: nothing else is in it, and the decoder already refused any leftover.
        let framed = 24
            + 4 * smc.shared_component_remap.len() as i64
            + 16 * smc.unmanaged.len() as i64
            + smc.total_unmanaged_bytes() as i64
            + 16 * smc.managed_shared_records.len() as i64
            + smc.managed_blob.len() as i64;
        assert_eq!(
            framed,
            node.header.data_size,
            "{}: the regions of the stream add up to {framed}, its DataSize is {}",
            path.display(),
            node.header.data_size
        );

        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            smc.shared_component_remap.len(),
            smc.unmanaged.len(),
            smc.total_unmanaged_bytes(),
            smc.managed_shared_records.len(),
            smc.managed_blob.len(),
            smc.managed_component_count,
            node.header.data_size
        ));
    }

    distinct_hashes.sort_unstable();
    let joined = lines.join("\n");
    let got = format!(
        "files={with_stream}\n\
         node_bytes={node_bytes}\n\
         remap_entries={remap_entries}\n\
         remap_nonzero={remap_nonzero}\n\
         remap_max={remap_max}\n\
         remap_surjective={remap_surjective}\n\
         unmanaged_shared_components={unmanaged_total}\n\
         unmanaged_value_bytes={unmanaged_bytes}\n\
         managed_shared_components={managed_shared_total}\n\
         managed_blob_bytes={managed_blob_bytes}\n\
         managed_components={managed_components}\n\
         record_hashes_in_archetype_table={hashes_in_archetype_table}\n\
         first_component_hash_checked={first_component_hash_checked}\n\
         distinct_shared_component_hashes={}\n\
         fnv1a64_lines={:016x}\n\
         fnv1a64_hashes={:016x}\n",
        distinct_hashes.len(),
        fnv1a64(joined.as_bytes()),
        fnv1a64(
            distinct_hashes
                .iter()
                .flat_map(|h| h.to_le_bytes())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_shared_and_managed_components.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded shared and managed components fingerprint at {}; the block above is what \
             this run computed",
            record.display()
        );
    }
    let want: String = std::fs::read_to_string(&record)
        .expect("read the shared and managed components record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        got.trim(),
        want.trim(),
        "the shared and managed components stream moved away from the record"
    );
}

/// The `BlobAssets` stream, checked against the batch the scene loader builds from it.
///
/// The loader never copies or indexes this stream: `EndDeserializeWorld` @ 0x8fe34f0 hands the
/// node's whole data blob to `BlobAssetBatch.CreateFromMemory` @ 0x90704b0, which validates it in
/// place, and `ReadUnmanagedSharedComponents` @ 0x8febba0 turns the `int` a serialized shared
/// component stores in each of its blob reference slots into a pointer by adding the base of that
/// same buffer. So the batch's own arithmetic — `16 + sum(32 + align16(Length))`, with each blob's
/// payload recorded at `48 + sum(32 + align16(Length))` over the earlier blobs — is the thing that
/// has to be right, and that is what this checks against every shipped scene.
#[test]
fn blob_assets_stream_decodes_from_every_scene_file() {
    let dir = scenes_dir();
    let files = scene_files(&dir);
    assert!(!files.is_empty(), "no scene files under {}", dir.display());

    let mut with_stream = 0usize;
    let mut empty_batches = 0usize;
    let mut blobs_total = 0usize;
    let mut batch_bytes_total = 0usize;
    let mut payload_bytes_total = 0usize;
    let mut size_fields_ok = 0usize;
    let mut offset_recurrence_ok = 0usize;
    let mut lookup_roundtrip_ok = 0usize;
    let mut max_blobs_in_a_batch = 0usize;
    let mut largest_blob = 0usize;
    let mut rejected = Vec::new();
    let mut lines = Vec::new();
    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut reused: std::collections::HashSet<u64> = std::collections::HashSet::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(file) = SceneFile::parse(bytes) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Some(node) = file.stream(SceneStream::BlobAssets) else {
            rejected.push(format!("{name}: no BlobAssets node"));
            continue;
        };
        with_stream += 1;
        let blob_hash = fnv1a64(&node.data);
        let batch = match file.blob_assets() {
            Ok(b) => b,
            Err(e) => {
                rejected.push(format!("{name}: {e}"));
                continue;
            }
        };

        if batch.header.total_data_size as usize != node.data.len() {
            rejected.push(format!(
                "{name}: batch declares {} bytes, the node holds {}",
                batch.header.total_data_size,
                node.data.len()
            ));
            continue;
        }
        size_fields_ok += 1;
        if batch.blobs.is_empty() {
            empty_batches += 1;
            assert_eq!(node.data.len(), 16, "{name}: an empty batch is the 16-byte header alone");
        }

        let mut cursor = 16usize;
        let mut recurrence_ok = true;
        for b in &batch.blobs {
            cursor += 32;
            if b.offset != cursor || b.length % BLOB_ASSET_ALIGNMENT != 0 {
                recurrence_ok = false;
                break;
            }
            cursor += b.data.len();
        }
        if recurrence_ok && cursor == node.data.len() {
            offset_recurrence_ok += 1;
        } else {
            rejected.push(format!(
                "{name}: the offset recurrence ends at {cursor}, the node holds {}",
                node.data.len()
            ));
        }

        let mut roundtrip_ok = batch.find_by_offset(-1).is_none();
        for b in &batch.blobs {
            if batch.find_by_offset(b.offset as i32).map(|f| f.index) != Some(b.index) {
                roundtrip_ok = false;
            }
        }
        if roundtrip_ok {
            lookup_roundtrip_ok += 1;
        } else {
            rejected.push(format!("{name}: a blob is not findable by its own recorded offset"));
        }

        for b in &batch.blobs {
            blobs_total += 1;
            if !seen.insert(b.hash) {
                reused.insert(b.hash);
            }
            largest_blob = largest_blob.max(b.data.len());
        }
        max_blobs_in_a_batch = max_blobs_in_a_batch.max(batch.blobs.len());
        batch_bytes_total += node.data.len();
        payload_bytes_total += batch.total_blob_bytes();

        lines.push(format!(
            "{name}\tbs={}\tblobs={}\tpayload={}\thash={blob_hash:016x}",
            node.data.len(),
            batch.blobs.len(),
            batch.total_blob_bytes()
        ));
    }

    for r in rejected.iter().take(5) {
        println!("rejected: {r}");
    }
    assert_eq!(rejected, Vec::<String>::new(), "blob asset batches this reader could not accept");
    assert_eq!(with_stream, files.len(), "every scene file has to carry a BlobAssets node");
    assert_eq!(size_fields_ok, with_stream, "every batch has to declare the size it was handed");
    assert_eq!(
        offset_recurrence_ok, with_stream,
        "every batch has to tile the way `CalculateBlobAssetBatchTotalSize` sizes it"
    );
    assert_eq!(
        lookup_roundtrip_ok, with_stream,
        "every blob has to be findable by the offset a shared component stores for it"
    );
    assert_eq!(
        batch_bytes_total,
        16 * with_stream + 32 * blobs_total + payload_bytes_total,
        "the batches have to be exactly their headers plus their payloads"
    );

    let joined = lines.join("\n");
    let got = format!(
        "files={with_stream}\n\
         empty_batches={empty_batches}\n\
         blobs={blobs_total}\n\
         batch_bytes={batch_bytes_total}\n\
         payload_bytes={payload_bytes_total}\n\
         distinct_hashes={}\n\
         hashes_seen_more_than_once={}\n\
         max_blobs_in_a_batch={max_blobs_in_a_batch}\n\
         largest_blob_bytes={largest_blob}\n\
         size_fields_ok={size_fields_ok}\n\
         offset_recurrence_ok={offset_recurrence_ok}\n\
         lookup_roundtrip_ok={lookup_roundtrip_ok}\n\
         fnv1a64_lines={:016x}\n",
        seen.len(),
        reused.len(),
        fnv1a64(joined.as_bytes())
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = root.join("evidence/entity_blob_assets.txt");
    if !record.exists() {
        print!("{got}");
        panic!(
            "no recorded blob assets fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    let want: String = std::fs::read_to_string(&record)
        .expect("read the blob assets record")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.trim(), want.trim(), "the blob assets stream moved away from the record");
}
