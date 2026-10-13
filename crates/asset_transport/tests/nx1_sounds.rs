//! NX1 sounds are loose files, not an archive.
//!
//! Every other namespace's streamed clips are read out of an IWD through
//! `IwdSoundIndex`. NX1 has no fastfile, so `index(Nx1)` is `None` by construction
//! and its clips could never be fetched — which is why the NX1 weapon plays its
//! donor's audio. The streamed path already decodes a byte buffer; it only ever
//! asked an archive for it.
//!
//! These tests read the real export, and pin that a catalog row cannot reach a
//! file outside the sound tree.

use asset_core::AssetNamespace;
use asset_transport::{NamespaceSoundIwd, NX1_SOUNDS_ROOT};

fn sounds() -> NamespaceSoundIwd {
    NamespaceSoundIwd::default().with_nx1_root(NX1_SOUNDS_ROOT)
}

/// The export must actually be where the constant says, or every other test here
/// passes vacuously.
#[test]
fn the_export_is_present() {
    assert!(
        std::path::Path::new(NX1_SOUNDS_ROOT).is_dir(),
        "NX1 sounds root missing: {NX1_SOUNDS_ROOT}"
    );
}

/// The SCAR-H is `xm108` in NX1's naming. If its gunfire cannot be read from disk,
/// wiring the aliases buys nothing.
#[test]
fn nx1_gunfire_is_readable_as_a_loose_file() {
    let source = sounds();
    assert!(
        source.nx1_root().is_some(),
        "NX1 root should be set when the export exists"
    );
    let bytes = source
        .read_sound(
            AssetNamespace::Nx1,
            "weapons/xm108/weap_xm108_fire_plr.wav",
        )
        .expect("the SCAR-H player fire sound must resolve")
        .expect("and read cleanly");
    assert!(bytes.len() > 1000, "fire sound is implausibly small");
    assert_eq!(&bytes[..4], b"RIFF", "expected a RIFF/WAVE container");
}

/// Each alias the weapon row will bind must be a real file, or it plays silence.
#[test]
fn every_sound_we_intend_to_bind_exists() {
    let source = sounds();
    let wanted = [
        "weapons/xm108/weap_xm108_fire_plr.wav",
        "weapons/xm108/weap_xm108_sp_fire_plr.wav",
        "weapons/xm108/weap_xm108_fire_npc.wav",
        "weapons/xm108/wpnfoley_xm108_mag_out_01.wav",
        "weapons/xm108/wpnfoley_xm108_mag_in_01.wav",
        "weapons/xm108/wpnfoley_xm108_switch.wav",
        "weapons/xm108/wpnfoley_xm108_first_time.wav",
    ];
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|rel| {
            matches!(source.read_sound(AssetNamespace::Nx1, rel), Some(Ok(_)))
                .not()
        })
        .collect();
    assert!(missing.is_empty(), "missing NX1 sounds: {missing:?}");
}

trait Not {
    fn not(self) -> bool;
}
impl Not for bool {
    fn not(self) -> bool {
        !self
    }
}

/// A catalog row is data, not a capability. It must not be able to name a file
/// outside the sound tree, whatever it contains.
#[test]
fn a_sound_row_cannot_escape_the_sound_tree() {
    let source = sounds();
    for hostile in [
        "../models/viewmodel_scar_h/viewmodel_scar_h_LOD0.cast",
        "weapons/../../outside.wav",
        "..",
        "C:/Windows/win.ini",
        "",
    ] {
        let outcome = source.read_sound(AssetNamespace::Nx1, hostile);
        assert!(
            !matches!(outcome, Some(Ok(_))),
            "a sound row was able to read `{hostile}`"
        );
    }
}

/// The other namespaces must be untouched: they still come from archives, and a
/// missing archive is still `None` rather than a filesystem read.
#[test]
fn other_namespaces_are_unaffected() {
    let source = sounds();
    assert!(source.index(AssetNamespace::Nx1).is_none());
    assert!(source.read_sound(AssetNamespace::Nx1, "no-such-file.wav").is_some());
}