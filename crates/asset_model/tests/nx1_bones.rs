//! NX1 bone names, so hide words can be derived from this rig rather than the
//! donor's.
//!
//! A hide word is bone-indexed: `surface_hidden` lifts a surface's part bits into
//! bone-aligned words and intersects them with the hide. The words on the NX1 row
//! are the donor SCAR-H's, so they index the *donor's* bone ordering. Applied to a
//! 35-bone NX1 rig they hide whatever NX1 bones happen to share those indices --
//! which is how 29 of 34 surfaces disappear, and why passing `None` moved the gun
//! off screen instead of simply revealing it.
//!
//! NX1 needs hide words built against its own bone list. That starts by knowing
//! what those bones are called. This prints them.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn bones_of(cast: &str) -> Vec<String> {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel.bone_names.clone()
}

#[test]
fn gun_bone_names() {
    let bones = bones_of("viewmodel_scar_h");
    println!("gun: {} bones", bones.len());
    for (i, b) in bones.iter().enumerate() {
        println!("  {i:>3} {b}");
    }
    assert!(!bones.is_empty());
}

#[test]
fn hands_bone_names() {
    let bones = bones_of("viewhands_nx_us_army");
    println!("hands: {} bones", bones.len());
    for (i, b) in bones.iter().enumerate() {
        println!("  {i:>3} {b}");
    }
    assert!(!bones.is_empty());
}

/// An NX1 hide word can only be built if the rig's bones carry the attachment tags
/// the donor's did. If they do not, the whole approach is wrong and we need the
/// attachment list from somewhere else.
/// The hands admit every surface, sit in the plan, and are textured — and still
/// draw nothing. Their packed geometry is authored around Z = +110 while the gun
/// is around Z = 0, so the hands are ~120 units behind the camera rather than in
/// front of it. Nothing in the log said so; the draw plan reports counts.
///
/// If that offset is the rig's own mount tag, recentring on it is the fix and can
/// be verified here without a build.
#[test]
fn hands_are_authored_away_from_the_view_origin() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");

    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &skel.positions {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    println!("hands bounds lo={lo:?} hi={hi:?}");

    // The gun, for comparison: it is the one that renders.
    let gun_path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let gun_bytes = std::fs::read(&gun_path).expect("gun cast");
    let (gun, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&gun_bytes, "gun").expect("gun");
    let mut gz = [f32::INFINITY; 2];
    let mut ghi = [f32::NEG_INFINITY; 2];
    for p in &gun.positions {
        gz[0] = gz[0].min(p[2]);
        ghi[0] = ghi[0].max(p[2]);
    }
    println!("gun Z range (already recentred): {} -> {}", gz[0], ghi[0]);
}

/// After recentring, both meshes must sit in the same space as the camera, or the
/// FPV assembly's single view bone puts one of them behind the viewer.
///
/// The hands authored 152.4 units back from their own `tag_view`; the gun needed no
/// shift at all. That difference is the whole reason the hands never appeared.
#[test]
fn recentring_puts_both_meshes_in_view_space() {
    for (cast, label) in [
        ("viewhands_nx_us_army", "hands"),
        ("viewmodel_scar_h", "gun"),
    ] {
        let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
        let bytes = std::fs::read(&path).expect(cast);
        let (skel, _) =
            asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, label).expect(cast);

        // Where the rig now believes its view point is, in bind world space.
        let Some(view) = skel.tag_view else {
            // The gun has no `tag_view` bone at all. It is authored around the
            // origin already, which is why it renders and the hands never did.
            println!("{label}: no tag_view bone — authored around the origin, no shift needed");
            continue;
        };
        let mut world = 0.0f32;
        let mut node = view;
        let mut chain = Vec::new();
        for _ in 0..skel.bones.len() {
            chain.push(node);
            match skel.parent_of(node) {
                Some(p) => node = p,
                None => break,
            }
        }
        for &b in chain.iter().rev() {
            world += skel.bones[b].trans[2];
        }
        println!("{label}: tag_view index={:?} now at Z={world:.3}", skel.tag_view);
        assert!(
            world.abs() < 0.01,
            "{label}: tag_view should sit at the origin after recentring, it is at Z={world}"
        );
    }
}

/// The recentring has to reach the *pose*, not just the skel.
///
/// `DObj::build` reads its bind transforms from `ModelPoseSrc::base_mat`, and that
/// array is built before the skel is assembled. An earlier version shifted
/// `skel.bones` after the fact, so the reader's own tests passed — the skel carried
/// the shift — while `base_mat` kept the file's original values and the FPV
/// assembly never saw it. The hands stayed 152 units from the camera with the fix
/// "applied".
///
/// This asserts on the pose, which is what the draw actually reads.
#[test]
fn the_pose_carries_the_shift_not_just_the_skel() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (skel, _) =
        asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");

    let pose = skel.pose.as_ref().expect("the hands must carry a pose");
    let view = pose
        .bone_names
        .iter()
        .position(|n| n == "tag_view")
        .expect("hands carry a tag_view");

    // `base_mat` is parent-relative; `DObj::build` composes it down the chain.
    // So the view bone's *world* position is the sum along its chain, and that sum
    // is what must now be zero.
    let mut world = [0.0f32; 3];
    let mut node = view;
    for _ in 0..pose.bone_names.len() {
        let Some((_, t)) = pose.base_mat.get(node).copied() else {
            break;
        };
        world[0] += t.x;
        world[1] += t.y;
        world[2] += t.z;
        // Parents sit before children and `parent_list` is a step from the child.
        if node < pose.num_root_bones {
            break;
        }
        let child = node - pose.num_root_bones;
        let Some(&step) = pose.parent_list.get(child) else {
            break;
        };
        if step == 0 {
            break;
        }
        node = node - step as usize;
    }
    println!("pose base_mat tag_view world = {world:?}");
    assert!(
        world[2].abs() < 0.01,
        "the pose still composes to the original view offset (Z={}); the assembly reads \
         base_mat, so shifting skel.bones alone does nothing",
        world[2]
    );
}

/// And the root's bind transform the dobj derives everything else from must have
/// absorbed the same offset.
#[test]
fn the_pose_root_absorbs_the_view_offset() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (skel, _) =
        asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");
    let pose = skel.pose.as_ref().expect("pose");
    let root = pose.base_mat.first().copied().expect("base_mat root");
    println!("pose base_mat root = {root:?}");
    assert!(
        root.1.z.abs() > 100.0,
        "the root should have absorbed the ~152 unit view offset, it is {root:?}"
    );
}

/// The gun has no `tag_view`, so it must be left exactly as authored. This is the
/// guard against a fix that moves everything, which would put a working gun off
/// screen.
#[test]
fn a_mesh_without_a_tag_view_is_untouched() {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).expect("gun cast");
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "gun").expect("gun");
    let pose = skel.pose.as_ref().expect("gun pose");
    assert!(
        !pose.bone_names.iter().any(|n| n == "tag_view"),
        "the gun is expected to have no tag_view bone"
    );
    let root = pose.base_mat.first().copied().expect("gun base_mat root");
    println!("gun base_mat root = {root:?} (untouched)");
    assert!(
        root.1.length() < 0.01,
        "the gun has no tag_view and must not be shifted, its root moved to {root:?}"
    );
}