//! Bounded reader for the candidate male prefab's serialized face collection.
//! This indexes assets; it does not select or attach a gameplay face.
use anyhow::{ensure, Context, Result};
use unity_formats::{
    player::{ObjRef, PPtr},
    scene::Scene,
};

use crate::GameContent;

/// The final word's meaning is not yet established; preserve it without interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureEntry {
    pub prefab: PPtr,
    pub unknown: u32,
}

fn pointer(bytes: &[u8]) -> Result<PPtr> {
    ensure!(bytes.len() == 12, "invalid serialized pointer width");
    Ok(PPtr {
        file_id: i32::from_le_bytes(bytes[0..4].try_into()?),
        path_id: i64::from_le_bytes(bytes[4..12].try_into()?),
    })
}

/// Parse the observed 4-byte count and 16-byte records, rejecting extra or missing data.
/// The shape is established only for the pinned male face collection.
pub fn parse_face_entries(bytes: &[u8]) -> Result<Vec<FeatureEntry>> {
    let count = i32::from_le_bytes(bytes.get(..4).context("missing count")?.try_into()?);
    let count = usize::try_from(count).context("negative entry count")?;
    let expected = count
        .checked_mul(16)
        .and_then(|n| n.checked_add(4))
        .context("entry size overflow")?;
    ensure!(expected == bytes.len(), "face collection length mismatch");
    bytes[4..]
        .as_chunks::<16>()
        .0
        .iter()
        .map(|entry| {
            Ok(FeatureEntry {
                prefab: pointer(&entry[..12])?,
                unknown: u32::from_le_bytes(entry[12..16].try_into()?),
            })
        })
        .collect()
}

/// Follow the observed pinned prefab's HybridEquipmentUser to its face collection.
/// Offset 80 is a build-specific observation and is checked against script and asset identity.
pub fn candidate_male_faces(content: &GameContent) -> Result<Vec<(ObjRef, u32)>> {
    const FILE: &str = "ae19a255904fd765632aa97a2d7c670c";
    let player = content.player();
    let scene = Scene::read(player, FILE)?;
    let roots: Vec<_> = scene
        .game_objects
        .values()
        .filter(|o| o.name == "HYB_VampireMale_Prefab")
        .collect();
    ensure!(roots.len() == 1, "candidate player root is ambiguous");
    let mut components = Vec::new();
    for &path_id in &roots[0].components {
        let r = ObjRef {
            file: FILE.into(),
            path_id,
        };
        if player.class_name(&r) == Some("MonoBehaviour")
            && player.script_class(&r)?.as_deref() == Some("ProjectM.HybridEquipmentUser")
        {
            components.push(r);
        }
    }
    ensure!(
        components.len() == 1,
        "candidate equipment component is ambiguous"
    );
    let (_, payload) = player.behaviour(&components[0])?;
    let face_ptr = pointer(
        payload
            .get(80..92)
            .context("missing face collection pointer")?,
    )?;
    let collection = player
        .resolve_pptr(FILE, face_ptr)?
        .context("null face collection")?;
    ensure!(
        player.script_class(&collection)?.as_deref() == Some("ProjectM.FeaturesCollection"),
        "unexpected face collection script"
    );
    let (base, payload) = player.behaviour(&collection)?;
    ensure!(
        base.get("m_Name").and_then(|v| v.as_str()) == Some("VampireMale_FaceCollection"),
        "unexpected face collection name"
    );
    parse_face_entries(&payload)?
        .into_iter()
        .map(|entry| {
            let prefab = player
                .resolve_pptr(&collection.file, entry.prefab)?
                .context("null face prefab")?;
            ensure!(
                player.class_name(&prefab) == Some("GameObject"),
                "face entry is not a GameObject"
            );
            Ok((prefab, entry.unknown))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_face_entries;

    #[test]
    fn checks_framing_and_preserves_pointer_and_unknown_word() {
        let mut data = 1i32.to_le_bytes().to_vec();
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&(-3i64).to_le_bytes());
        data.extend_from_slice(&0x11223344u32.to_le_bytes());
        let rows = parse_face_entries(&data).unwrap();
        assert_eq!(
            (
                rows[0].prefab.file_id,
                rows[0].prefab.path_id,
                rows[0].unknown
            ),
            (2, -3, 0x11223344)
        );
        assert!(parse_face_entries(&data[..19]).is_err());
        data.push(0);
        assert!(parse_face_entries(&data).is_err());
        assert!(parse_face_entries(&(-1i32).to_le_bytes()).is_err());
    }
}
