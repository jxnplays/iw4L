//! Checks the NX1 `.cast` reader against the shipped SCAR LOD.
//!
//! The source file is an owned copy outside this repository, so the test skips
//! when it is absent rather than failing a checkout that does not have it.
//! Nothing here fabricates a value: every assertion is a property of the file.

use asset_model::cast_xmodel::read_cast_xmodel;

const CAST: &str = concat!(
    r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki",
    r"\exported_files\nx1\models\weapon_scar2\weapon_scar2_LOD0.cast"
);

#[test]
fn reads_the_scar_lod() {
    let Ok(bytes) = std::fs::read(CAST) else {
        eprintln!("skipping: {CAST} is not present");
        return;
    };
    let skel = read_cast_xmodel(&bytes, "weapon_scar2_LOD0").expect("cast parses");

    // Seven `mesh` nodes, each naming one of the seven materials.
    assert_eq!(skel.surface_vertex_ranges.len(), 7);
    assert_eq!(skel.surface_index_ranges.len(), 7);
    assert_eq!(skel.surface_materials.len(), 7);

    // 17 tag bones, `tag_weapon` first and at the origin.
    assert_eq!(skel.bones.len(), 17);
    assert_eq!(skel.bone_names[0], "tag_weapon");
    assert_eq!(skel.bone_names[1], "tag_brass");

    // Every array is the length of the single concatenated vertex block.
    assert_eq!(skel.positions.len(), 2400);
    assert_eq!(skel.normals.len(), 2400);
    assert_eq!(skel.colors.len(), 2400);
    assert_eq!(skel.uvs.len(), 2400);
    assert_eq!(skel.vert_skin.len(), 2400);
    assert_eq!(skel.indices.len(), 1786 * 3);

    // The gun is rigid: every vertex binds to bone 0 at full weight, so there
    // is no blending and no collapse into a single unbound surface.
    assert_eq!(skel.rigid_verts, 2400);
    assert_eq!(skel.blend_verts, 0);
    assert!(
        skel.vert_skin
            .iter()
            .all(|skin| skin.bones[0] == 0 && skin.weights[0] == 1.0 && skin.weights[1] == 0.0)
    );

    // Surfaces partition the vertex and index blocks without gaps or overlap.
    let mut vertex_cursor = 0usize;
    let mut index_cursor = 0usize;
    for (surface, ((base, count), (ibase, icount))) in skel
        .surface_vertex_ranges
        .iter()
        .zip(&skel.surface_index_ranges)
        .enumerate()
    {
        assert_eq!(*base, vertex_cursor, "surface {surface} vertex base");
        assert_eq!(*ibase, index_cursor, "surface {surface} index base");
        assert_eq!(icount % 3, 0, "surface {surface} index count");
        vertex_cursor += count;
        index_cursor += icount;
    }
    assert_eq!(vertex_cursor, skel.positions.len());
    assert_eq!(index_cursor, skel.indices.len());

    // Every index addresses a vertex that exists.
    assert!(
        skel.indices
            .iter()
            .all(|&index| usize::try_from(index).is_ok_and(|i| i < skel.positions.len()))
    );

    // Normals are unit length, so the mesh needs no renormalising to light.
    assert!(skel.normals.iter().all(|normal| {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        (length - 1.0).abs() < 0.01
    }));

    // The barrel runs down -X from the muzzle: the receiver spans
    // x[-61.463, 39.510], so the model is 101 units long end to end.
    let (mid, half) = skel.bounds.expect("bounds");
    assert!((mid[0] + 10.977).abs() < 0.01, "bounds mid {mid:?}");
    assert!((half[0] - 50.487).abs() < 0.01, "bounds half {half:?}");
    // Y and Z are the thin axes of a rifle held level.
    assert!(half[1] < 4.0, "bounds half {half:?}");
    assert!(half[2] < 16.0, "bounds half {half:?}");
    assert!(skel.radius.unwrap_or(0.0) > half[0]);

    // NX1 materials are CryEngine paths with no IW4 catalog entry, so no
    // surface binds one. That is the file's state, not a parse failure.
    assert!(skel.surface_materials.iter().all(Option::is_none));
    assert!(skel.packed_vertices.is_empty());
}

#[test]
fn refuses_a_file_that_is_not_a_cast() {
    let error = read_cast_xmodel(b"not a cast file at all", "x").expect_err("magic is checked");
    assert!(error.contains("magic"), "{error}");
}
