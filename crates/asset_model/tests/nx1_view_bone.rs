//! NX1 rig recentring, asserted the way the draw reads it.
//!
//! Each NX1 viewmodel is authored around its *own* `tag_view`: the hands carry
//! theirs at bind Z = +152.4, with geometry spanning Z = 110..135 in front of it.
//! The gun carries none and is authored around the origin.
//!
//! The FPV assembly resolves one view bone for the whole weapon and places every
//! part against it, so two meshes authored against different view origins cannot
//! both be right. Whichever origin wins, the other part ends up hundreds of units
//! from the camera — admitted, textured, in the draw plan, and invisible.
//!
//! Two earlier attempts fixed the wrong surface and both passed their own tests:
//!
//!  1. shifting `skel.bones` after the skel existed — but `DObj::build` never reads
//!     it;
//!  2. shifting `base_mat[root]` before the pose — but a root bone's bind comes
//!     from `root_rest`, so `base_mat` is ignored for it.
//!
//! Both failed only at runtime, where they read as "arms missing" and "camera two
//! feet too high". These tests build the `DObj` the assembly builds and assert on
//! `bind_world`, so they fail here instead.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";


/// The assembled `DObj` for one cast, as `FpvAssembly::build` assembles it.
fn dobj_of(cast: &str) -> (xmodel_runtime::DObj, String) {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) =
        asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
            .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    let pose = skel.pose.as_ref().expect("cast must carry a pose");
    let dobj = xmodel_runtime::DObj::build(&[(pose, None)]).expect("DObj::build");
    (dobj, cast.to_owned())
}

/// The whole point: after recentring, the hands' view point sits on the camera.
#[test]
fn hands_view_bone_lands_on_the_origin() {
    let (dobj, name) = dobj_of("viewhands_nx_us_army");
    let view = dobj.find("tag_view").expect("hands carry a tag_view");
    let at = dobj.bones[view].bind_world.w_axis.truncate();
    println!("{name}: tag_view bind_world = {at:?}");
    assert!(
        at.length() < 0.01,
        "the hands' tag_view must reach the origin through DObj::build, it is at {at:?}"
    );
}

/// The offset has to be carried by `root_rest`: a root bone's bind is read from
/// there, not from `base_mat`. If this regresses the shift silently stops applying.
#[test]
fn the_offset_travels_in_root_rest() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (skel, _) =
        asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");
    let pose = skel.pose.as_ref().expect("pose");
    let (_, root_rest) = pose.root_rest.expect("the offset must ride in root_rest");
    println!("root_rest = {root_rest:?}");
    assert!(
        root_rest.z.abs() > 100.0,
        "root_rest should carry the ~152 unit view offset, it is {root_rest:?}"
    );
}

/// The gun has no `tag_view`, so it must be left exactly as authored. A fix that
/// moved everything put the gun off screen once already, so this guards it.
#[test]
fn a_mesh_without_a_tag_view_is_untouched() {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).expect("gun cast");
    let (skel, _) =
        asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "gun").expect("gun");
    let pose = skel.pose.as_ref().expect("gun pose");
    assert!(
        !pose.bone_names.iter().any(|n| n == "tag_view"),
        "the gun is expected to have no tag_view bone"
    );
    assert!(
        pose.root_rest.is_none(),
        "the gun has no view offset to apply, it must carry no root_rest"
    );
    let dobj = xmodel_runtime::DObj::build(&[(pose, None)]).expect("gun DObj");
    let root = dobj.bones[0].bind_world.w_axis.truncate();
    println!("gun root bind_world = {root:?}");
    assert!(
        root.length() < 0.01,
        "the gun is authored around the origin and must stay there, root is at {root:?}"
    );
}

/// Names, for the record. NX1's rig carries real attachment tags, which is what
/// makes its own hide words derivable.
#[test]
fn bone_names_are_recorded() {
    for cast in ["viewmodel_scar_h", "viewhands_nx_us_army"] {
        let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
        let bytes = std::fs::read(&path).expect(cast);
        let (skel, _) =
            asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast).expect(cast);
        let tagged: Vec<&String> = skel
            .bone_names
            .iter()
            .filter(|b| b.contains("tag_"))
            .collect();
        println!("{cast}: {} bones, {} tagged", skel.bone_names.len(), tagged.len());
        assert!(!skel.bone_names.is_empty());
    }
}
