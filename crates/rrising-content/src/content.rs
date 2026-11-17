//! Loads V Rising's own content folder with the original's own formats.
//!
//! The game keeps its art in `StreamingAssets/ContentArchives/<hash>`: 4411 UnityFS bundles, each a
//! `SerializedFile` that stores its type trees, holding named `Mesh`, `Texture2D`, `Material`,
//! `Avatar`, `ParticleSystem` and `MonoBehaviour` objects. This module opens them as one addressable
//! store and indexes them by class and name, which is how the port finds the art it has to draw.
//!
//! The type trees a *player* build strips are supplied by `assets/typetrees.tpk`, generated from the
//! AssetRipper dump of the exact engine version the build reports (`2022.3.58f1`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use unity_formats::material::UnityMaterialData;
use unity_formats::mesh::UnityMeshData;
use unity_formats::player::{BundleSpec, NodeSelection, ObjRef, Player};
use unity_formats::texture::UnityTexture;
use unity_formats::TypeTreePackage;

/// Classes the renderer and the gameplay layer need from the archives.
pub const TRACKED_CLASSES: &[&str] = &[
    "Mesh",
    "Texture2D",
    "Material",
    "Shader",
    "Avatar",
    "Animator",
    "AnimatorController",
    "AnimationClip",
    "Sprite",
    "SkinnedMeshRenderer",
    "MeshFilter",
    "MeshRenderer",
    "ParticleSystem",
    "GameObject",
];

/// One entry of the content index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentEntry {
    /// Unity's class id from the object table (1 GameObject, 4 Transform, 28 Texture2D, ...).
    pub class_id: i32,
    /// The class name the type tree package gives the object (`Mesh`, `Texture2D`, ...).
    pub class: String,
    /// The object's `m_Name`, empty when its class stores none.
    pub name: String,
    /// Where the object lives: which file of the store, and its path id.
    pub object: ObjRef,
}

/// The opened content of a V Rising install.
pub struct GameContent {
    player: Player,
    /// `(class, lower-cased name)` -> every object with that key. Several archives hold same-named
    /// objects (a model and its LOD, or the same texture in two variants), so the index keeps them all.
    by_key: BTreeMap<(String, String), Vec<ContentEntry>>,
    /// Every indexed object, in the order the store lists them.
    entries: Vec<ContentEntry>,
    /// The data folder the content was opened from.
    pub data_dir: PathBuf,
}

impl GameContent {
    /// Open a `VRising_Data` folder plus its `StreamingAssets/ContentArchives` bundles.
    ///
    /// `archives` may be a subset; passing the whole folder is what the game does at startup, and it
    /// costs parsing 4411 file headers.
    pub fn open(data_dir: &Path, archives: &[PathBuf], tpk_path: &Path) -> Result<Self> {
        let tpk = std::fs::read(tpk_path)
            .with_context(|| format!("reading type tree package {}", tpk_path.display()))?;
        let package = TypeTreePackage::parse(&tpk)
            .map_err(|e| anyhow::anyhow!("type tree package {}: {e}", tpk_path.display()))?;

        let specs: Vec<BundleSpec> = archives
            .iter()
            .map(|p| BundleSpec {
                path: p.as_path(),
                nodes: NodeSelection::All,
            })
            .collect();
        let player = Player::open_with_bundles(data_dir, package, &specs)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", data_dir.display()))?;

        let mut content = GameContent {
            player,
            by_key: BTreeMap::new(),
            entries: Vec::new(),
            data_dir: data_dir.to_path_buf(),
        };
        content.build_index()?;
        Ok(content)
    }

    /// List every object of the tracked classes and index it by `(class, name)`.
    fn build_index(&mut self) -> Result<()> {
        let listing = unity_formats::listing::list(&self.player, Some(TRACKED_CLASSES))
            .map_err(|e| anyhow::anyhow!("listing objects: {e}"))?;
        for e in listing.entries {
            // A class the package has no name for cannot be asked for by class, so it is not indexed;
            // the store still holds it and `player()` reaches it by path id.
            let Some(class) = e.class else { continue };
            let entry = ContentEntry {
                class_id: e.class_id,
                class,
                name: e.name.unwrap_or_default(),
                object: e.object,
            };
            let key = (entry.class.clone(), entry.name.to_lowercase());
            self.by_key.entry(key).or_default().push(entry.clone());
            self.entries.push(entry);
        }
        Ok(())
    }

    /// Every archive file the store knows about.
    pub fn file_count(&self) -> usize {
        self.player.files.len()
    }

    /// Indexed objects, in store order.
    pub fn entries(&self) -> &[ContentEntry] {
        &self.entries
    }

    /// Count of indexed objects per class, in class-name order.
    pub fn class_counts(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for e in &self.entries {
            *m.entry(e.class.clone()).or_default() += 1;
        }
        m
    }

    /// Every object of `class` named exactly `name` (case-insensitive).
    pub fn find(&self, class: &str, name: &str) -> &[ContentEntry] {
        self.by_key
            .get(&(class.to_owned(), name.to_lowercase()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The single object of `class` named `name`, when the name is unambiguous.
    pub fn find_unique(&self, class: &str, name: &str) -> Result<ObjRef> {
        match self.find(class, name) {
            [] => bail!("no {class} named {name:?} in the content"),
            [one] => Ok(one.object.clone()),
            many => bail!(
                "{class} {name:?} is ambiguous: {} objects, first in {}",
                many.len(),
                many[0].object.file
            ),
        }
    }

    /// Read a `Mesh` object: vertex channels, indices, submeshes and skin.
    pub fn read_mesh(&self, r: &ObjRef) -> Result<UnityMeshData> {
        UnityMeshData::read(&self.player, r).map_err(|e| anyhow::anyhow!("mesh {}: {e}", r.file))
    }

    /// Read a `Texture2D` object with its mip chain still packed in `data`.
    pub fn read_texture(&self, r: &ObjRef) -> Result<UnityTexture> {
        UnityTexture::read(&self.player, r).map_err(|e| anyhow::anyhow!("texture {}: {e}", r.file))
    }

    /// Read a `Material`: its shader, saved properties and texture table.
    pub fn read_material(&self, r: &ObjRef) -> Result<UnityMaterialData> {
        UnityMaterialData::read(&self.player, r)
            .map_err(|e| anyhow::anyhow!("material {}: {e}", r.file))
    }

    /// The `Player` behind the store, for readers this module does not wrap.
    pub fn player(&self) -> &Player {
        &self.player
    }
}

/// The UnityFS bundle signature a content archive starts with.
const UNITYFS: &[u8] = b"UnityFS";

/// List the archive bundles of a `StreamingAssets/ContentArchives` folder in a stable order.
///
/// Two things this pins down. The order matters: the store's iteration order decides which
/// same-named object wins, so the port fixes it instead of relying on a directory read. And the
/// folder is not only bundles — it also holds `archive_dependencies.bin`, the serialized
/// `RuntimeContentCatalog` the game uses to resolve archive dependencies. That file is not a UnityFS
/// bundle and is skipped here; parsing it is tracked in `docs/PORT.md`.
pub fn list_archives(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out.retain(|p| {
        let Ok(mut f) = std::fs::File::open(p) else {
            return false;
        };
        let mut head = [0u8; 7];
        match std::io::Read::read_exact(&mut f, &mut head) {
            Ok(()) => head == UNITYFS,
            Err(_) => false,
        }
    });
    Ok(out)
}
