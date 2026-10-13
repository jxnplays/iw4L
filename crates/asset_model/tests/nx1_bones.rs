//! NX1 bone names and geometry, recorded from the real dump.
//!
//! The recentring behaviour itself is covered in `nx1_view_bone.rs`, which asserts
//! through `DObj::build` — the thing the draw actually reads. Asserting on `base_mat`
//! or on `skel.bones` proved nothing: two earlier versions of the fix passed those
//! and were still wrong at runtime.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn read(cast: &str) -> asset_model::ModelSkel {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel
}

/// The gun has no `tag_view` bone; the hands do, and theirs is 152.4 units back.
/// That difference is the whole reason one rendered and the other did not.
#[test]
fn the_two_meshes_use_different_view_origins() {
    let gun = read("viewmodel_scar_h");
    let hands = read("viewhands_nx_us_army");

    assert!(
        !gun.bone_names.iter().any(|b| b == "tag_view"),
        "the gun is authored around the origin: it has no tag_view"
    );

    let view = hands
        .bone_names
        .iter()
        .position(|b| b == "tag_view")
        .expect("the hands carry a tag_view");
    let mut world = 0.0f32;
    let mut node = view;
    for _ in 0..hands.bones.len() {
        world += hands.bones[node].trans[2];
        match hands.parent_of(node) {
            Some(p) => node = p,
            None => break,
        }
    }
    println!("hands tag_view sits {world} units back before recentring");
    assert!(
        world > 100.0,
        "the hands are expected to be authored well behind the origin, found {world}"
    );
}

/// NX1's rig carries real attachment tags. That is what makes NX1's own hide words
/// derivable instead of inherited from the donor, which had no such luck.
#[test]
fn nx1_carries_attachment_tags() {
    for (cast, tagged_min) in [("viewmodel_scar_h", 20usize), ("viewhands_nx_us_army", 10)] {
        let skel = read(cast);
        let tagged = skel
            .bone_names
            .iter()
            .filter(|b| b.contains("tag_"))
            .count();
        println!("{cast}: {} bones, {tagged} tagged", skel.bone_names.len());
        assert!(
            tagged >= tagged_min,
            "{cast}: expected at least {tagged_min} tagged bones, found {tagged}"
        );
    }
}

/// Geometry sizes, recorded so a parse regression is visible without a build.
#[test]
fn geometry_is_read_in_full() {
    let gun = read("viewmodel_scar_h");
    assert_eq!(gun.bone_names.len(), 35);
    assert_eq!(gun.surface_index_ranges.len(), 34);
    assert!(!gun.positions.is_empty());
    assert!(gun.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));

    let hands = read("viewhands_nx_us_army");
    assert_eq!(hands.bone_names.len(), 66);
    assert_eq!(hands.surface_index_ranges.len(), 2);
    assert!(hands.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
}