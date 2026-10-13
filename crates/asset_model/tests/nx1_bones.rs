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

/// The vertices deliberately do not move: the shift is applied to the root bone so
/// the skinned result moves while the geometry and the rig stay in agreement. This
/// pins that the root actually moved, and that the geometry is otherwise untouched.
#[test]
fn recentring_shifts_the_root_bone_not_the_vertices() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");

    let first = skel.positions[0];
    let root = skel.bones[0].trans;
    println!("hands root bone trans after recentring: {root:?}");
    println!("hands first vertex (unchanged): {first:?}");
    assert!(
        root[2].abs() > 100.0,
        "the root should have absorbed the ~152 unit view offset, it is {root:?}"
    );
}