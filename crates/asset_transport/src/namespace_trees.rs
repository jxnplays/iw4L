use std::path::{Path, PathBuf};

use asset_core::AssetNamespace;

use crate::ZoneGame;
use crate::discover::{GamesRoot, find_zone_file_version, zone_game_for_path, zone_version};
use crate::iwd::game_main_for_zone;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceTree {
    pub anchor: PathBuf,

    pub main: Option<PathBuf>,
}

impl NamespaceTree {
    fn from_anchor(anchor: PathBuf) -> Self {
        let main = game_main_for_zone(&anchor).ok();
        Self { anchor, main }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceTrees {
    iw4: Option<NamespaceTree>,
    t5: Option<NamespaceTree>,
    iw5: Option<NamespaceTree>,
    t6: Option<NamespaceTree>,
}

impl NamespaceTrees {
    /// Namespaces that are backed by a fastfile. NX1 is absent by design: this
    /// tree synthesises it from the Saluki dump, so it has no `common_mp` to
    /// discover and no zone to adopt.
    const ZONED: [AssetNamespace; 4] = [
        AssetNamespace::Iw4,
        AssetNamespace::T5,
        AssetNamespace::Iw5,
        AssetNamespace::T6,
    ];

    pub fn discover(root: &GamesRoot) -> Self {
        let mut trees = Self::default();
        for ns in Self::ZONED {
            let version = zone_version(zone_game_of(ns));
            if let Ok(found) = find_zone_file_version(root, "common_mp", version) {
                if let Some(slot) = trees.slot_mut(ns) {
                    *slot = Some(NamespaceTree::from_anchor(found.path));
                }
            }
        }
        trees
    }

    pub fn adopt_zone(&mut self, zone_ff: &Path) {
        let Some(game) = zone_game_for_path(zone_ff) else {
            return;
        };
        let ns = AssetNamespace::from_zone_game(game);
        if let Some(slot) = self.slot_mut(ns) {
            *slot = Some(NamespaceTree::from_anchor(zone_ff.to_path_buf()));
        }
    }

    pub fn get(&self, ns: AssetNamespace) -> Option<&NamespaceTree> {
        match ns {
            AssetNamespace::Iw4 => self.iw4.as_ref(),
            AssetNamespace::T5 => self.t5.as_ref(),
            AssetNamespace::Iw5 => self.iw5.as_ref(),
            AssetNamespace::T6 => self.t6.as_ref(),
            // NX1 is synthesised from the Saluki dump, not a fastfile, so it has
            // no zone tree. Its assets are read directly by the match walk.
            AssetNamespace::Nx1 => None,
        }
    }

    pub fn main_for(&self, ns: AssetNamespace) -> Option<&Path> {
        self.get(ns).and_then(|tree| tree.main.as_deref())
    }

    pub fn is_empty(&self) -> bool {
        self.iw4.is_none() && self.t5.is_none() && self.iw5.is_none() && self.t6.is_none()
    }

    pub fn present(&self) -> impl Iterator<Item = (AssetNamespace, &NamespaceTree)> {
        Self::ZONED
            .into_iter()
            .filter_map(|ns| self.get(ns).map(|tree| (ns, tree)))
    }

    pub fn report_lines(&self) -> Vec<String> {
        Self::ZONED
            .into_iter()
            .map(|ns| match self.get(ns) {
                Some(tree) => format!(
                    "namespace tree {}: main={} anchor={}",
                    ns.as_str(),
                    tree.main
                        .as_deref()
                        .map_or_else(|| "-".to_owned(), |m| m.display().to_string()),
                    tree.anchor.display(),
                ),
                None => format!("namespace tree {}: not installed", ns.as_str()),
            })
            .collect()
    }

    /// `None` for a namespace with no tree slot, which is NX1.
    fn slot_mut(&mut self, ns: AssetNamespace) -> Option<&mut Option<NamespaceTree>> {
        match ns {
            AssetNamespace::Iw4 => Some(&mut self.iw4),
            AssetNamespace::T5 => Some(&mut self.t5),
            AssetNamespace::Iw5 => Some(&mut self.iw5),
            AssetNamespace::T6 => Some(&mut self.t6),
            AssetNamespace::Nx1 => None,
        }
    }
}

const fn zone_game_of(ns: AssetNamespace) -> ZoneGame {
    match ns {
        AssetNamespace::Iw4 => ZoneGame::Iw4,
        AssetNamespace::T5 => ZoneGame::T5,
        AssetNamespace::Iw5 => ZoneGame::Iw5,
        AssetNamespace::T6 => ZoneGame::T6,
        AssetNamespace::Nx1 => ZoneGame::Nx1,
    }
}

#[derive(Debug, Default)]
pub struct NamespaceSoundIwd {
    iw4: Option<crate::iwd::IwdSoundIndex>,
    t5: Option<crate::iwd::IwdSoundIndex>,
    iw5: Option<crate::iwd::IwdSoundIndex>,
    /// Where NX1's loose sound files live.
    ///
    /// NX1 is not a zone game: it has no fastfile, so no `IwdSoundIndex` can be
    /// opened for it and `index(Nx1)` is `None` by construction. Its sounds ship as
    /// plain files in the export's `sounds` tree, which the streamed-clip path
    /// already knows how to decode -- it only ever asked an archive. So NX1 is
    /// served from a directory instead, and `read_sound` is the single seam.
    nx1_root: Option<std::path::PathBuf>,
}

/// NX1 sound files, as they sit in the Saluki export.
pub const NX1_SOUNDS_ROOT: &str =
    "E:\\Call of Duty Future Warfare\\NX1\\output\\one-gun\\tools\\saluki\\exported_files\\nx1\\sounds";

impl NamespaceSoundIwd {
    /// Point NX1 at a directory of loose sound files.
    pub fn with_nx1_root(mut self, root: impl Into<std::path::PathBuf>) -> Self {
        let root = root.into();
        if root.is_dir() {
            self.nx1_root = Some(root);
        }
        self
    }

    pub fn nx1_root(&self) -> Option<&std::path::Path> {
        self.nx1_root.as_deref()
    }
    pub fn open(trees: &NamespaceTrees) -> (Self, Vec<String>) {
        let mut opened = Self::default();
        let mut lines = Vec::new();
        for (ns, tree) in trees.present() {
            let Some(main) = tree.main.as_deref() else {
                lines.push(format!(
                    "audio: IWD sounds {}: tree {} has no archive directory",
                    ns.as_str(),
                    tree.anchor.display()
                ));
                continue;
            };
            match crate::iwd::IwdSoundIndex::open(main) {
                Ok(index) => {
                    lines.push(format!(
                        "audio: IWD sounds {} from {} — {} paths",
                        ns.as_str(),
                        index.indexed_dir().display(),
                        index.sound_count()
                    ));
                    if let Some(slot) = opened.slot_mut(ns) {
                        *slot = Some(index);
                    }
                }
                Err(e) => lines.push(format!(
                    "audio: IWD sounds {} unavailable ({e})",
                    ns.as_str()
                )),
            }
        }
        (opened, lines)
    }

    pub fn index(&self, ns: AssetNamespace) -> Option<&crate::iwd::IwdSoundIndex> {
        match ns {
            AssetNamespace::Iw4 => self.iw4.as_ref(),
            AssetNamespace::T5 => self.t5.as_ref(),
            AssetNamespace::Iw5 => self.iw5.as_ref(),
            AssetNamespace::T6 => None,
            AssetNamespace::Nx1 => None,
        }
    }

    pub fn read_sound(
        &self,
        ns: AssetNamespace,
        relative: &str,
    ) -> Option<Result<Vec<u8>, String>> {
        if ns == AssetNamespace::Nx1 {
            return self.read_nx1_file(relative);
        }
        self.index(ns)?.read_sound(relative)
    }

    /// Read one NX1 sound from the export directory.
    ///
    /// The relative path comes from a catalog row, but a catalog row should never
    /// be able to name a file outside the tree, so `..` and rooted paths are refused
    /// rather than normalised.
    fn read_nx1_file(&self, relative: &str) -> Option<Result<Vec<u8>, String>> {
        let root = self.nx1_root.as_deref()?;
        let trimmed = relative.trim_start_matches(['/', '\\']);
        if trimmed.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = Vec::new();
        for part in trimmed.split(['/', '\\']) {
            match part {
                "" | "." => continue,
                ".." => return None,
                other => {
                    if other.contains(':') {
                        return None;
                    }
                    parts.push(other);
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        let mut path = root.to_path_buf();
        for part in parts {
            path.push(part);
        }
        Some(std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display())))
    }

    pub fn is_empty(&self) -> bool {
        self.iw4.is_none() && self.t5.is_none() && self.iw5.is_none() && self.nx1_root.is_none()
    }

    fn slot_mut(&mut self, ns: AssetNamespace) -> Option<&mut Option<crate::iwd::IwdSoundIndex>> {
        match ns {
            AssetNamespace::Iw4 => Some(&mut self.iw4),
            AssetNamespace::T5 => Some(&mut self.t5),
            AssetNamespace::Iw5 => Some(&mut self.iw5),
            AssetNamespace::T6 => None,
            AssetNamespace::Nx1 => None,
        }
    }
}
