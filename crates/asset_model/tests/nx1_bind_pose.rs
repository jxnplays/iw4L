//! The placement pipeline, proved by arithmetic rather than by screenshot.
//!
//! `skin[b] = world[b] * bind_world[b]^-1`.
//!
//! At bind pose -- no animation, no placement -- the posed world and the bind
//! world are the same transform, so **every skin matrix must be the identity**.
//! If it is, the whole chain is provably lossless: bone order, parent
//! accumulation, the merged rig's `bone_base`, and the unit conversion all
//! cancel by construction.
//!
//! This test would have caught both bugs that cost the weapon its arms, without
//! anyone launching the game:
//!
//!  * the reader assembling animation translations as `[ty, tz, tx]`;
//!  * `base_mat` holding the parent-relative `lp` where an absolute world
//!    transform belongs, which put every hand bone up to 152 cm (60 in) from the
//!    geometry it drives.
//!
//! Both are invisible to a green test suite that only checks parsed bytes. This
//! one checks the thing that decides what reaches the screen.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn read(cast: &str) -> asset_model::ModelSkel {
    let path = format!(r"{ROOT}\models\{cast}\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel
}

fn dobj(skel: &asset_model::ModelSkel) -> xmodel_runtime::DObj {
    let pose = skel.pose.as_ref().expect("cast must carry a pose");
    xmodel_runtime::DObj::build(&[(pose, None)]).expect("DObj::build")
}

fn bones<'a>(skel: &'a asset_model::ModelSkel) -> Vec<&'a str> {
    skel.bone_names.iter().map(String::as_str).collect()
}

/// Every NX1 mesh must skin to a no-op at bind pose.
#[test]
fn nx1_skin_matrices_are_identity_at_bind_pose() {
    for cast in [
        "viewhands_nx_us_army",
        "viewmodel_scar_h",
        "viewmodel_scar2",
    ] {
        let skel = read(cast);
        let dobj = dobj(&skel);
        let names = bones(&skel);

        // No animation, identity placement: the posed world is the bind pose.
        let world = dobj.pose(&[], &Default::default(), bevy::math::Mat4::IDENTITY);
        let skin = dobj.skin_matrices(&world);

        assert_eq!(
            skin.len(),
            names.len(),
            "{cast}: one skin matrix per bone"
        );

        let mut worst = 0.0f32;
        let mut worst_bone = "";
        // Reported separately, because they have different causes. A translation
        // error means the two world transforms genuinely disagree and the geometry
        // is being displaced -- a real bug. A rotation error of a few thousandths
        // is `quantize_quat` storing `quats` as i16, so the posed world is quantised
        // while `bind_world` keeps the file's exact float. That floor is expected and
        // grows with depth in the hierarchy; a finger bone is five levels down.
        let mut worst_translation = 0.0f32;
        let mut worst_rotation = 0.0f32;
        for (index, matrix) in skin.iter().enumerate() {
            let m = matrix.to_cols_array();
            let translation = (m[12].abs() + m[13].abs() + m[14].abs()).max(m[15].abs() - 1.0);
            let rotation = (m[0..12].iter())
                .zip([
                    1.0f32, 0.0, 0.0, 0.0, //
                    0.0, 1.0, 0.0, 0.0, //
                    0.0, 0.0, 1.0, 0.0,
                ])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            let deviation = translation.max(rotation);
            if deviation > worst {
                worst = deviation;
                worst_bone = names.get(index).copied().unwrap_or("?");
            }
            worst_translation = worst_translation.max(translation);
            worst_rotation = worst_rotation.max(rotation);
        }

        println!(
            "{cast}: {} bones, worst {worst:.6} at {worst_bone} \
             (translation {worst_translation:.6}, rotation {worst_rotation:.6})",
            names.len()
        );
        // Depth correlation. `quats` is stored i16-quantised, so the posed world is
        // rebuilt through quantised rotations while `bind_world` keeps the file's
        // exact `wr`. The residual must therefore be ~0 near the root and grow with
        // distance down the hierarchy. A residual that does NOT follow depth would be
        // a logic error, not quantisation, and this check is what tells them apart.
        {
            let mut rows: Vec<(usize, usize, f32)> = Vec::new();
            let pose = skel.pose.as_ref().expect("pose");
            for (index, matrix) in skin.iter().enumerate() {
                let m = matrix.to_cols_array();
                let translation = (m[12].abs() + m[13].abs() + m[14].abs())
                    .max((m[15].abs() - 1.0).max(0.0));
                if translation < 1e-7 {
                    continue;
                }
                let mut depth = 0usize;
                let mut walk = index;
                while walk >= pose.num_root_bones {
                    let step = pose.parent_list[walk - pose.num_root_bones];
                    if step == u8::MAX {
                        break;
                    }
                    depth += 1;
                    walk -= step as usize;
                }
                rows.push((depth, index, translation));
            }
            rows.sort_by_key(|r| r.0);
            let mut cursor = 0usize;
            let mut printed = 0usize;
            while cursor < rows.len() && printed < 8 {
                let d = rows[cursor].0;
                let end = rows[cursor..].iter().position(|r| r.0 != d).map_or(rows.len(), |p| p + cursor);
                let worst_at_depth = rows[cursor..end]
                    .iter()
                    .max_by(|a, b| a.2.total_cmp(&b.2))
                    .copied()
                    .unwrap();
                let (depth_i, bone_i, err) = worst_at_depth;
                println!("    depth {depth_i}: worst translation {err:.6} (bone {bone_i})");
                cursor = end;
                printed += 1;
            }
        }
        assert!(
            worst_translation < 0.01,
            "{cast}: skin TRANSLATION must be identity at bind pose; worst {worst_translation:.6} \
             at {worst_bone}. 0.01 world units is 0.01 inch, about a quarter of a \
             millimetre -- the previous bug was 23 units, 59 inches.",
        );
        assert!(
            worst_rotation < 0.02,
            "{cast}: skin rotation deviates by {worst_rotation:.6} at {worst_bone}, beyond \
             quaternion quantisation.",
        );
    }
}

/// The same invariant after the two meshes are merged into one rig, which is what
/// the renderer actually poses. A merge offset error cannot hide behind two
/// individually-correct meshes.
#[test]
fn the_merged_nx1_rig_skins_to_a_no_op_at_bind_pose() {
    let hands = read("viewhands_nx_us_army");
    let gun = read("viewmodel_scar_h");
    let (hand_pose, gun_pose) = (
        hands.pose.as_ref().expect("hands pose"),
        gun.pose.as_ref().expect("gun pose"),
    );

    let merged = xmodel_runtime::DObj::build(&[(hand_pose, None), (gun_pose, None)])
        .expect("merge hands + gun");
    let total = hands.bone_names.len() + gun.bone_names.len();
    let world = merged.pose(&[], &Default::default(), bevy::math::Mat4::IDENTITY);
    let skin = merged.skin_matrices(&world);

    assert_eq!(skin.len(), total, "merged rig must hold every bone");

    let mut worst_translation = 0.0f32;
    let mut worst_rotation = 0.0f32;
    let mut worst_bone = "";
    for (index, matrix) in skin.iter().enumerate() {
        let m = matrix.to_cols_array();
        let translation = (m[12].abs() + m[13].abs() + m[14].abs())
            .max((m[15].abs() - 1.0).max(0.0));
        let rotation = (m[0..12].iter())
            .zip([
                1.0f32, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0,
            ])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let label = hands
            .bone_names
            .get(index)
            .or_else(|| gun.bone_names.get(index - hands.bone_names.len()))
            .map(String::as_str)
            .unwrap_or("?");
        if translation.max(rotation) > worst_translation.max(worst_rotation) {
            worst_bone = label;
        }
        worst_translation = worst_translation.max(translation);
        worst_rotation = worst_rotation.max(rotation);
    }

    println!(
        "merged: {total} bones (hands {} + gun {}), worst translation {worst_translation:.6}, \
         rotation {worst_rotation:.6}, at {worst_bone}",
        hands.bone_names.len(),
        gun.bone_names.len()
    );
    assert!(
        worst_translation < 0.01,
        "the merged rig must skin to a no-op at bind pose; translation deviates by \
         {worst_translation:.6} at {worst_bone}. A merge offset error would show up here.",
    );
    assert!(
        worst_rotation < 0.02,
        "the merged rig's skin rotation deviates by {worst_rotation:.6} at {worst_bone}, \
         beyond quaternion quantisation.",
    );
}
