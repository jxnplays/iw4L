//! Cast faces are CCW; IW4 draws `FrontFace::Cw` with back faces culled.
//!
//! The reader must hand the renderer IW order, or every outward face of an NX1
//! mesh is culled and its inside draws instead -- a see-through gun. Culling
//! happens on the GPU, so no draw count can show this; this test does, from a
//! one-triangle cast built in memory (no game files, no machine paths).

fn prop(out: &mut Vec<u8>, id: u16, name: &str, elements: u32, data: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&elements.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(data);
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn node(id: &[u8; 4], props: &[u8], prop_n: u32, children: &[Vec<u8>]) -> Vec<u8> {
    let body: usize = children.iter().map(Vec::len).sum();
    let size = 24 + props.len() + body;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(id);
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&prop_n.to_le_bytes());
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    out.extend_from_slice(props);
    for child in children {
        out.extend_from_slice(child);
    }
    out
}

/// One bone, one triangle wound CCW about +Z with +Z normals: cast's own order.
fn one_triangle_cast() -> Vec<u8> {
    let mut bone = Vec::new();
    prop(&mut bone, 0x73, "n", 1, b"tag_origin\0");
    prop(&mut bone, 0x69, "p", 1, &(-1i32).to_le_bytes());
    prop(&mut bone, 0x7633, "lp", 1, &floats(&[0.0, 0.0, 0.0]));
    prop(&mut bone, 0x7634, "lr", 1, &floats(&[0.0, 0.0, 0.0, 1.0]));
    prop(&mut bone, 0x7633, "wp", 1, &floats(&[0.0, 0.0, 0.0]));
    prop(&mut bone, 0x7634, "wr", 1, &floats(&[0.0, 0.0, 0.0, 1.0]));
    let bone = node(b"bone", &bone, 6, &[]);
    let skel = node(b"skel", &[], 0, &[bone]);

    let mut mesh = Vec::new();
    prop(
        &mut mesh,
        0x7633,
        "vp",
        3,
        &floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
    );
    prop(
        &mut mesh,
        0x7633,
        "vn",
        3,
        &floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
    );
    prop(&mut mesh, 0x7632, "u0", 3, &floats(&[0.0; 6]));
    prop(&mut mesh, 0x69, "c0", 3, &[0xff; 12]);
    prop(&mut mesh, 0x68, "f", 3, &[0, 0, 1, 0, 2, 0]);
    prop(&mut mesh, 0x62, "mi", 1, &[1]);
    prop(&mut mesh, 0x62, "wb", 3, &[0, 0, 0]);
    prop(&mut mesh, 0x66, "wv", 3, &floats(&[1.0, 1.0, 1.0]));
    let mesh = node(b"mesh", &mesh, 8, &[]);

    let model = node(b"modl", &[], 0, &[skel, mesh]);
    let root = node(b"root", &[], 0, &[model]);
    let mut out = Vec::new();
    out.extend_from_slice(b"cast");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&root);
    out
}

#[test]
fn cast_faces_reach_the_renderer_in_iw_order() {
    let skel = asset_model::cast_xmodel::read_cast_xmodel(&one_triangle_cast(), "winding")
        .unwrap_or_else(|e| panic!("parse: {e}"));
    assert_eq!(skel.indices.len(), 3);
    let p = |i: u32| skel.positions[i as usize];
    let (a, b, c) = (p(skel.indices[0]), p(skel.indices[1]), p(skel.indices[2]));
    let z = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    // The normal is +Z. IW order is clockwise seen from the normal side, so the
    // geometric normal must point the other way.
    assert!(
        z < 0.0,
        "indices {:?} are still in cast (CCW) order: outward faces would be culled",
        skel.indices
    );
    // Same corners, only reordered.
    let mut sorted = skel.indices.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2]);
}
