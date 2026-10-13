//! NX1 rig recentring, asserted the way the draw reads it.
//!
//! Each NX1 viewmodel is authored around its *own* `tag_view`: the hands carry
//! theirs at bind Z = +152.4, with geometry spanning Z = 110..135 in front of it.
//! The gun carries none and is authored around the origin.
//!
//! The FPV assembly resolves one view bone for the whole weapon and places every
//! part against it, so two meshes authored against different view origins cannot
//! both be right. The owner confirmed the raised viewpoint appears *only* with this
//! weapon, which pins the cause to this path rather than a general camera bug.
//!
//! Three earlier versions of this fix passed their own tests and were still wrong
//! at runtime, each because the assertion was on the wrong surface:
//!
//!  1. shifting `skel.bones` after the skel existed -- `DObj::build` never reads it;
//!  2. shifting `base_mat[root]` -- a root's bind comes from `root_rest`, and every
//!     other bone carries its own absolute world translation, so one entry is wrong
//!     twice over;
//!  3. setting *both* `base_mat` and `root_rest` -- the root is then displaced twice
//!     while every other bone is displaced once, so the rig and its geometry
//!     disagree by exactly the view offset.
//!
//! These build the `DObj` the assembly builds and assert `bind_world`.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn hands() -> asset_model::ModelSkel {
    read("viewhands_nx_us_army")
}


fn gun() -> asset_model::ModelSkel {
    read("viewmodel_scar_h")
}

fn read(cast: &str) -> asset_model::ModelSkel {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel
}

fn dobj(skel: &asset_model::ModelSkel) -> xmodel_runtime::DObj {
    let pose = skel.pose.as_ref().expect("cast must carry a pose");
    xmodel_runtime::DObj::build(&[(pose, None)]).expect("DObj::build")
}

/// The whole point: after recentring, the hands' view point sits on the camera.
#[test]
fn hands_view_bone_lands_on_the_origin() {
    let skel = hands();
    let dobj = dobj(&skel);
    let view = dobj.find("tag_view").expect("hands carry a tag_view");
    let at = dobj.bones[view].bind_world.w_axis.truncate();
    println!("hands tag_view bind_world = {at:?}");
    assert!(
        at.length() < 0.01,
        "the hands' tag_view must reach the origin through DObj::build, it is at {at:?}"
    );
}
/// The offset belongs in *both* channels, because they feed different consumers.
///
/// `bind_world` (from `base_mat`) drives skinning. `bind_translation` (from
/// `root_rest`) drives the posed world the FPV assembly reads for the camera eye.
/// They are not summed.
///
/// Removing `root_rest` put `view_trans` back to 152.4 and the viewpoint high again
/// while surface admission kept improving -- which is what gave the change away.
#[test]
fn the_offset_rides_in_both_channels() {
    let skel = hands();
    let pose = skel.pose.as_ref().expect("pose");
    let root_rest = pose.root_rest.expect("the posed world needs the offset too");
    println!("root_rest = {root_rest:?}");
    assert!(
        root_rest.1.z.abs() > 100.0,
        "root_rest carries the view offset for the posed world, it is {root_rest:?}"
    );

    // And the skinned geometry needs it independently.
    let dobj = dobj(&skel);
    let view = dobj.find("tag_view").expect("hands carry a tag_view");
    let at = dobj.bones[view].bind_world.w_axis.truncate();
    assert!(
        at.length() < 0.01,
        "base_mat must carry the same offset for skinning, tag_view is at {at:?}"
    );
}

/// Every bone must move by the offset, not just the root: `base_mat` holds a world
/// transform per bone and `DObj` uses it directly.
#[test]
fn every_bone_carries_the_offset() {
    let skel = hands();
    let dobj = dobj(&skel);
    let z = skel.bone_names.len();
    let moved = dobj
        .bones
        .iter()
        .filter(|b| b.bind_world.w_axis.truncate().z.abs() > 100.0)
        .count();
    println!("{moved} of {z} bones sit more than 100 units back");
    assert!(
        moved > 0 && moved < z,
        "expected the hands' bones to have moved off the origin, {moved} of {z} did"
    );
}

/// The gun has no `tag_view`, so it must be left exactly as authored. A fix that
/// moved everything put the gun off screen once already.
#[test]
fn a_mesh_without_a_tag_view_is_untouched() {
    let skel = gun();
    let pose = skel.pose.as_ref().expect("gun pose");
    assert!(
        !pose.bone_names.iter().any(|n| n == "tag_view"),
        "the gun is expected to have no tag_view bone"
    );
    assert!(
        pose.root_rest.is_none(),
        "the gun has no view offset to apply, so it must carry no root_rest"
    );
    let dobj = dobj(&skel);
    let root = dobj.bones[0].bind_world.w_axis.truncate();
    println!("gun root bind_world = {root:?}");
    assert!(
        root.length() < 0.01,
        "the gun is authored around the origin and must stay there, root is at {root:?}"
    );
}

