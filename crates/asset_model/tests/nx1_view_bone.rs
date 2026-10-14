//! The NX1 rig is used exactly as authored. Nothing is recentred on load.
//!
//! Every NX1 clip places the rig itself. `viewmodel_scar_h_idle.cast` says so
//! outright:
//!
//! ```text
//! tag_player  tz = -152.39999
//! tag_weapon  at (33.48, -7.31, -9.68)
//! ```
//!
//! `tag_view` sits at local +152.4, so the clip's own root translation carries it
//! to world origin. The recentring is in the data. It is not this reader's job.
//!
//! ## Why this file exists
//!
//! Four builds shipped a reader that shifted the rig by -152.4 on top of that,
//! once through `base_mat` and again through `root_rest`. The result was a camera
//! 152.4 units (2 ft) off and hands flung out across a 500-unit box. The hands
//! diagnostic found it by printing `max_move=152.400` -- the offset, reappearing
//! inside a pose change.
//!
//! Each earlier removal was judged on `view_trans`, and each was judged wrong:
//!
//! > Dropping `root_rest` put `view_trans` back to 152.4 and the viewpoint high
//! > again, while `admitted` kept climbing.
//!
//! That reasoning does not hold. A uniform shift `T` cancels out of the viewpoint:
//!
//! ```text
//! (T * W_view)^-1 * (T * W_x)  ==  W_view^-1 * W_x
//! ```
//!
//! so no amount of rigid offset changes what the camera sees. `view_trans` moved;
//! the screen did not. Asserting on it validated every wrong version of this fix.
//!
//! These tests therefore assert on the *bind pose as authored*, which is a
//! property of the file and cannot drift with the camera.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

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

/// No mesh gets a `root_rest`. It was the second half of the double shift, and
/// nothing in the NX1 data calls for it -- every clip authors its own root.
#[test]
fn no_mesh_carries_a_root_rest() {
    for cast in ["viewhands_nx_us_army", "viewmodel_scar_h", "viewmodel_scar2"] {
        let skel = read(cast);
        let pose = skel.pose.as_ref().expect("pose");
        assert!(
            pose.root_rest.is_none(),
            "{cast} must not override its root bind; the clip places the rig"
        );
    }
}

/// The hands' bind pose must be reported exactly as the file states it, including
/// `tag_view` sitting back at +152.4. That value is the whole point: it is what the
/// clip's `tag_player` translation cancels.
#[test]
fn the_hands_bind_pose_is_untouched() {
    let skel = read("viewhands_nx_us_army");
    let dobj = dobj(&skel);
    let view = dobj.find("tag_view").expect("hands carry a tag_view");
    let at = dobj.bones[view].bind_world.w_axis.truncate();
    println!("hands tag_view bind_world = {at:?}");

    let scale = asset_model::cast_xmodel::CAST_TO_WORLD;
    let _ = scale;
    assert!(
        (at.z - 152.4).abs() < 0.5,
        "tag_view must stay where the file put it, 152.4 units back; it is at {at:?}"
    );
}

/// The gun is authored around the origin and carries no `tag_view` at all, so it
/// must survive a load completely untouched.
#[test]
fn the_gun_is_left_exactly_as_authored() {
    let skel = read("viewmodel_scar_h");
    let pose = skel.pose.as_ref().expect("gun pose");
    assert!(
        !pose.bone_names.iter().any(|n| n == "tag_view"),
        "the gun is expected to have no tag_view bone"
    );
    let dobj = dobj(&skel);
    let root = dobj.bones[0].bind_world.w_axis.truncate();
    println!("gun root bind_world = {root:?}");
    assert!(
        root.length() < 0.01,
        "the gun is authored around the origin and must stay there, root is at {root:?}"
    );
}

/// The two anchors that pin the whole rig.
///
/// `base_mat` is a per-bone world transform used directly for skinning, so any
/// offset added to it displaces the geometry a second time. The hands are authored
/// with their root on the origin and `tag_view` 152.4 back. Both holding that exact
/// relationship is only possible if nothing was added anywhere in the chain.
#[test]
fn the_root_and_the_view_bone_stay_exactly_where_authored() {
    let skel = read("viewhands_nx_us_army");
    let pose = skel.pose.as_ref().expect("pose");
    let dobj = dobj(&skel);

    let root = dobj.bones[0].bind_world.w_axis.truncate();
    let view = dobj.find("tag_view").expect("hands carry a tag_view");
    let at_view = dobj.bones[view].bind_world.w_axis.truncate();

    println!("hands root = {root:?}  tag_view = {at_view:?}");
    assert!(
        root.length() < 0.01,
        "the hands' root bone is authored on the origin, it is at {root:?}"
    );
    assert!(
        (at_view.z - 152.4).abs() < 0.5 && at_view.x.abs() < 0.01 && at_view.y.abs() < 0.5,
        "tag_view must sit 152.4 back and nowhere else, it is at {at_view:?}"
    );
    assert_eq!(
        pose.base_mat.len(),
        pose.num_bones,
        "base_mat must hold one world transform per bone"
    );
}

/// The animation's translation axes, pinned to the file that authored them.
///
/// `viewmodel_scar_h_idle.cast` states:
/// ```text
/// tag_player  tz = -152.39999
/// tag_weapon  tx = 33.48431, ty = -7.31177, tz = -9.67803
/// ```
///
/// The reader used to assemble those as `[ty, tz, tx]`, rotating every animated
/// translation by one axis. It put tag_player's Z on Y, dropping the hands 152.4
/// units below the gun -- which reads as a raised viewpoint, because the gun was
/// never high, the arms were down.
///
/// tag_weapon is the sharper tell: 33.48 lateral, 7.31 down, 9.68 forward is an
/// ordinary grip offset. Rotated, it reads (-7.31, -9.68, 33.48) -- sideways,
/// sunken, and 33 units toward the camera.
#[test]
fn animation_translation_axes_are_x_y_z() {
    let path = format!("{ROOT}\\animations\\viewmodel_scar_h_idle.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    let clip = asset_model::cast_xmodel::read_cast_xanim(
        &bytes,
        "viewmodel_scar_h_idle",
        &std::collections::HashMap::new(),
    )
    .unwrap_or_else(|e| panic!("parse: {e}"));

    let at = |bone: &str| -> [f32; 3] {
        let track = clip
            .tracks
            .iter()
            .find(|t| t.name == bone)
            .unwrap_or_else(|| panic!("no track for {bone}"));
        let keyed = match &track.translation {
            xmodel_runtime::Translation::FullKeyed(k) => k,
            other => panic!("{bone}: expected a keyed translation, got {other:?}"),
        };
        keyed.values[0]
    };

    let player = at("tag_player");
    println!("tag_player = {player:?}");
    assert!(
        (player[2] + 152.4).abs() < 0.1 && player[0].abs() < 0.1 && player[1].abs() < 0.1,
        "tag_player is authored at (0, 0, -152.4); it read as {player:?}, so the \
         translation axes are rotated"
    );

    let weapon = at("tag_weapon");
    println!("tag_weapon = {weapon:?}");
    assert!(
        (weapon[0] - 33.484).abs() < 0.01
            && (weapon[1] + 7.312).abs() < 0.01
            && (weapon[2] + 9.678).abs() < 0.01,
        "tag_weapon is authored at (33.484, -7.312, -9.678); it read as {weapon:?}, \
         so the translation axes are rotated"
    );
}
