//! NX1 `.cast` meshes into one `ModelSkel`.
//!
//! A `.cast` file is the CryEngine chunked container, not a fastfile: magic
//! `cast`, then a `root` node holding `meta` and `modl` children. A node is a
//! flat record of named properties and child nodes, and a `mesh` node holds its
//! geometry as property arrays (`vp`, `vn`, `u0`, `c0`, `f`) rather than as a
//! vertex block, so the IW4 `GFX_PACKED_VERTEX` path never runs.
//!
//! NX1 is an Xbox 360 prototype tree with no fastfile to walk, so this is the
//! only way its weapon geometry reaches a map. It is a converter, not a runtime
//! format: nothing in the client loads `.cast`, and nothing here makes one.
//!
//! The container shape was confirmed against Greyhound's `CastExport.cpp`, a
//! writer for the same format. Nothing of it is copied or linked.

use crate::{BoneBind, ModelLodSelector, ModelSkel, VertSkin, unpack_color};

const MAGIC: u32 = 0x7473_6163; // "cast"

const NODE_MESH: u32 = 0x6873_656d; // "mesh"
const NODE_BONE: u32 = 0x656e_6f62; // "bone"

/// Property ids. Scalars are one character; vectors are the multi-character
/// constants `'v2'`, `'v3'`, `'v4'`, which land little-endian as `2v`, `3v`.
const PROP_BYTE: u16 = b'b' as u16;
const PROP_SHORT: u16 = b'h' as u16;
const PROP_INT32: u16 = b'i' as u16;
const PROP_INT64: u16 = b'l' as u16;
const PROP_FLOAT: u16 = b'f' as u16;
const PROP_DOUBLE: u16 = b'd' as u16;
const PROP_STRING: u16 = b's' as u16;
const PROP_VECTOR2: u16 = u16::from_le_bytes(*b"2v");
const PROP_VECTOR3: u16 = u16::from_le_bytes(*b"3v");
const PROP_VECTOR4: u16 = u16::from_le_bytes(*b"4v");

/// Bytes per element for a property id, or `None` when the id is not one this
/// converter reads. An unknown id cannot be sized, so its payload cannot be
/// skipped and the walk stops rather than losing sync.
fn element_width(id: u16) -> Option<usize> {
    if id == PROP_BYTE {
        return Some(1);
    }
    if id == PROP_SHORT {
        return Some(2);
    }
    if id == PROP_INT32 {
        return Some(4);
    }
    if id == PROP_INT64 {
        return Some(8);
    }
    if id == PROP_FLOAT {
        return Some(4);
    }
    if id == PROP_DOUBLE {
        return Some(8);
    }
    if id == PROP_VECTOR2 {
        return Some(8);
    }
    if id == PROP_VECTOR3 {
        return Some(12);
    }
    if id == PROP_VECTOR4 {
        return Some(16);
    }
    None
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .ok_or_else(|| "cast: read offset overflow".to_owned())?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("cast: read of {count} bytes past end of file"))?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        let mut word = [0u8; 8];
        word.copy_from_slice(b);
        Ok(u64::from_le_bytes(word))
    }

    /// A string property is a NUL-terminated run whose element count is 1, so
    /// the terminator is the only end marker.
    fn cstr(&mut self) -> Result<String, String> {
        let mut out = Vec::new();
        loop {
            let byte = self.u8()?;
            if byte == 0 {
                break;
            }
            out.push(byte);
        }
        String::from_utf8(out).map_err(|error| format!("cast: string is not UTF-8: {error}"))
    }
}

struct Node<'a> {
    props: Vec<(String, Property<'a>)>,
    children: usize,
}

struct Property<'a> {
    /// Elements the container states, used to check a requested count against.
    count: usize,
    width: Option<usize>,
    data: &'a [u8],
    text: Option<String>,
}

impl Property<'_> {
    /// The element count this property states, for a payload of known width.
    fn elements(&self, id: u16) -> Result<usize, String> {
        if self.width != element_width(id) {
            return Err(format!("cast: property is not a {id:#06x} value"));
        }
        Ok(self.count)
    }

    fn scalar(&self, id: u16, count: usize) -> Result<&[u8], String> {
        if self.width != element_width(id) {
            return Err(format!("cast: property is not a {id:#06x} value"));
        }
        let need = count
            .checked_mul(self.width.unwrap_or(0))
            .ok_or_else(|| "cast: property length overflow".to_owned())?;
        self.data.get(..need).ok_or_else(|| {
            format!(
                "cast: property holds {} bytes, needs {need}",
                self.data.len()
            )
        })
    }
}

/// Read a node's property list. The header carries the property and child
/// counts, both before the properties; the children follow them.
fn read_node<'a>(cursor: &mut Cursor<'a>) -> Result<Node<'a>, String> {
    let count = cursor.u32()? as usize;
    let children = cursor.u32()? as usize;
    let mut props = Vec::with_capacity(count.min(32));
    for _ in 0..count {
        let id = cursor.u16()?;
        let name_len = cursor.u16()? as usize;
        let elements = cursor.u32()? as usize;
        let name = String::from_utf8_lossy(cursor.take(name_len)?).into_owned();
        let (width, data, text) = if id == PROP_STRING {
            (None, &[][..], Some(cursor.cstr()?))
        } else {
            let width = element_width(id);
            let length = elements
                .checked_mul(width.unwrap_or(0))
                .ok_or_else(|| "cast: property length overflow".to_owned())?;
            (width, cursor.take(length)?, None)
        };
        props.push((
            name,
            Property {
                count: elements,
                width,
                data,
                text,
            },
        ));
    }
    Ok(Node { props, children })
}

impl Node<'_> {
    fn get(&self, name: &str) -> Option<&Property<'_>> {
        self.props
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn required(&self, name: &str) -> Result<&Property<'_>, String> {
        self.get(name)
            .ok_or_else(|| format!("cast: node has no `{name}` property"))
    }

    /// Every float in a property, whatever element count it states.
    fn all_floats(&self, name: &str, id: u16) -> Result<Vec<f32>, String> {
        let prop = self.required(name)?;
        if prop.width != element_width(id) {
            return Err(format!("cast: `{name}` is not a {id:#06x} value"));
        }
        let (words, _) = prop.data.as_chunks::<4>();
        words
            .iter()
            .map(|word| Ok(f32::from_bits(u32::from_le_bytes(*word))))
            .collect()
    }

    fn floats(&self, name: &str, id: u16, expect: usize) -> Result<Vec<f32>, String> {
        let data = self.required(name)?.scalar(id, expect)?;
        let (words, _) = data.as_chunks::<4>();
        words
            .iter()
            .map(|word| Ok(f32::from_bits(u32::from_le_bytes(*word))))
            .collect()
    }

    fn text(&self, name: &str) -> Result<&str, String> {
        self.required(name)?
            .text
            .as_deref()
            .ok_or_else(|| format!("cast: `{name}` is not a string property"))
    }
}

/// Walk every node, handing each to `visit`. The walk always descends: a node's
/// stated size covers its children, so skipping a subtree would leave the
/// cursor short of the next node. The node's size also bounds it, and a child
/// that runs past means the file is not what it claims, so the walk refuses
/// rather than reading into the next node.
fn walk(
    cursor: &mut Cursor<'_>,
    depth: usize,
    visit: &mut impl FnMut(u32, &Node<'_>) -> Result<(), String>,
) -> Result<(), String> {
    if depth > 64 {
        return Err("cast: node nesting is implausibly deep".to_owned());
    }
    let start = cursor.at;
    let id = cursor.u32()?;
    let size = cursor.u32()? as usize;
    let _hash = cursor.u64()?;
    let node = read_node(cursor)?;
    let end = start
        .checked_add(size)
        .ok_or_else(|| "cast: node size overflow".to_owned())?;
    if end > cursor.bytes.len() {
        return Err(format!(
            "cast: node at {start:#x} claims {size} bytes, past end of file"
        ));
    }
    visit(id, &node)?;
    for _ in 0..node.children {
        walk(cursor, depth + 1, visit)?;
    }
    if cursor.at != end {
        return Err(format!(
            "cast: node at {start:#x} ends at {:#x}, states {end:#x}",
            cursor.at
        ));
    }
    Ok(())
}

fn indices_of(node: &Node<'_>) -> Result<Vec<u32>, String> {
    let prop = node.required("f")?;
    let width = prop
        .width
        .ok_or_else(|| "cast: `f` is not a scalar property".to_owned())?;
    let indices = match width {
        1 => prop.data.iter().map(|&value| u32::from(value)).collect(),
        2 => prop
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u32::from(u16::from_le_bytes(*pair)))
            .collect(),
        4 => prop
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect(),
        other => return Err(format!("cast: index width {other} is not 1, 2 or 4")),
    };
    Ok(indices)
}

struct Surface {
    vertex_base: usize,
    vertex_count: usize,
    index_base: usize,
    index_count: usize,
    rigid: bool,
}

/// Read one `.cast` file into a single `ModelSkel`: one surface per `mesh`
/// node under the model, in file order, plus the `skel` bone hierarchy.
///
/// Bones and the per-vertex bind are read, not assumed. NX1 weapon skeletons
/// are tag trees whose meshes bind rigidly to `tag_weapon`, and this keeps that
/// a fact of the file rather than a claim about it.
pub fn read_cast_xmodel(bytes: &[u8], name: &str) -> Result<ModelSkel, String> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.u32()? != MAGIC {
        return Err("cast: magic is not \"cast\"".to_owned());
    }
    let _version = cursor.u32()?;
    let _flag = cursor.u32()?;
    let _reserved = cursor.u32()?;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut vert_skin: Vec<VertSkin> = Vec::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut bone_names: Vec<String> = Vec::new();
    let mut bones: Vec<BoneBind> = Vec::new();

    let mut visit = |id: u32, node: &Node<'_>| -> Result<(), String> {
        if id == NODE_BONE {
            let position = node.floats("lp", PROP_VECTOR3, 1)?;
            let rotation = node.floats("lr", PROP_VECTOR4, 1)?;
            bone_names.push(node.text("n")?.to_owned());
            bones.push(BoneBind {
                quat: [rotation[0], rotation[1], rotation[2], rotation[3]],
                trans: [position[0], position[1], position[2]],
            });
            return Ok(());
        }
        if id != NODE_MESH || node.get("vp").is_none() {
            return Ok(());
        }

        let vertex_count = node.required("vp")?.elements(PROP_VECTOR3)?;
        if vertex_count == 0 {
            return Err("cast: mesh has no vertices".to_owned());
        }
        let vertex_base = positions.len();
        let flat_positions = node.floats("vp", PROP_VECTOR3, vertex_count)?;
        let flat_normals = node.floats("vn", PROP_VECTOR3, vertex_count)?;
        let flat_uvs = node.floats("u0", PROP_VECTOR2, vertex_count)?;
        let packed_colors = node.required("c0")?.scalar(PROP_INT32, vertex_count)?;
        let mesh_indices = indices_of(node)?;
        if mesh_indices.len() % 3 != 0 {
            return Err(format!(
                "cast: `f` holds {} indices, not a multiple of 3",
                mesh_indices.len()
            ));
        }
        if mesh_indices
            .iter()
            .any(|&index| usize::try_from(index).map_or(true, |i| i >= vertex_count))
        {
            return Err("cast: `f` indexes a vertex the mesh does not have".to_owned());
        }

        // `mi` is the max influences per vertex; `wb`/`wv` carry that many
        // entries per vertex, vertex-major.
        let influences = node
            .get("mi")
            .and_then(|prop| prop.data.first().copied())
            .map_or(1, |count| usize::from(count).clamp(1, 4));
        let bone_bytes = node.get("wb").map_or(&[][..], |prop| prop.data);
        let bone_weights = node.all_floats("wv", PROP_FLOAT)?;

        for vertex in 0..vertex_count {
            positions.push([
                flat_positions[vertex * 3],
                flat_positions[vertex * 3 + 1],
                flat_positions[vertex * 3 + 2],
            ]);
            normals.push([
                flat_normals[vertex * 3],
                flat_normals[vertex * 3 + 1],
                flat_normals[vertex * 3 + 2],
            ]);
            colors.push(unpack_color(u32::from_le_bytes([
                packed_colors[vertex * 4],
                packed_colors[vertex * 4 + 1],
                packed_colors[vertex * 4 + 2],
                packed_colors[vertex * 4 + 3],
            ])));
            uvs.push([flat_uvs[vertex * 2], flat_uvs[vertex * 2 + 1]]);

            let mut bind = VertSkin::default();
            for slot in 0..influences {
                let at = vertex * influences + slot;
                if let Some(raw) = bone_bytes.get(at) {
                    bind.bones[slot] = u16::from(*raw);
                }
                if let Some(weight) = bone_weights.get(at) {
                    bind.weights[slot] = *weight;
                }
            }
            vert_skin.push(bind);
        }

        let index_base = indices.len();
        let first = u32::try_from(vertex_base).unwrap_or(0);
        indices.extend(mesh_indices.iter().map(|&index| first + index));
        let rigid = vert_skin[vertex_base..vertex_base + vertex_count]
            .iter()
            .all(|skin| skin.weights[1] == 0.0 && skin.weights[0] == 1.0);
        surfaces.push(Surface {
            vertex_base,
            vertex_count,
            index_base,
            index_count: mesh_indices.len(),
            rigid,
        });
        Ok(())
    };

    walk(&mut cursor, 0, &mut visit)?;
    if surfaces.is_empty() {
        return Err("cast: no mesh node carried geometry".to_owned());
    }

    let rigid_verts = vert_skin
        .iter()
        .filter(|skin| skin.weights[1] == 0.0 && skin.weights[0] == 1.0)
        .count();
    let blend_verts = vert_skin.len().saturating_sub(rigid_verts);
    let surface_count = surfaces.len();

    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for position in &positions {
        for (axis, value) in position.iter().enumerate() {
            lo[axis] = lo[axis].min(*value);
            hi[axis] = hi[axis].max(*value);
        }
    }
    let mid = [
        (lo[0] + hi[0]) * 0.5,
        (lo[1] + hi[1]) * 0.5,
        (lo[2] + hi[2]) * 0.5,
    ];
    let half = [
        (hi[0] - lo[0]) * 0.5,
        (hi[1] - lo[1]) * 0.5,
        (hi[2] - lo[2]) * 0.5,
    ];
    let radius = positions
        .iter()
        .map(|position| {
            let d = [
                position[0] - mid[0],
                position[1] - mid[1],
                position[2] - mid[2],
            ];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(0.0f32, f32::max);

    Ok(ModelSkel {
        name: name.to_owned(),
        bone_collision: vec![None; bones.len()],
        bone_names,
        tag_view: None,
        tag_weapon: None,
        bones,
        pose: None,
        positions,
        normals,
        colors,
        uvs,
        indices,
        // NX1 material names are CryEngine paths (`mc\mtl_nx_weapon_...`) with
        // no counterpart in the IW4 catalog, so no surface binds a material.
        surface_materials: vec![None; surface_count],
        surface_vertex_ranges: surfaces
            .iter()
            .map(|s| (s.vertex_base, s.vertex_count))
            .collect(),
        surface_index_ranges: surfaces
            .iter()
            .map(|s| (s.index_base, s.index_count))
            .collect(),
        // NX1 carries no per-surface part-bit mask, so every axis counts as used.
        surface_part_bits: vec![[u32::MAX, u32::MAX, 0, 0, 0, 0]; surface_count],
        surface_deformed: surfaces
            .iter()
            .map(|surface| Some(!surface.rigid))
            .collect(),
        surface_vert_list_count: surfaces
            .iter()
            .map(|surface| u32::try_from(surface.vertex_count).ok())
            .collect(),
        vert_skin,
        rigid_verts,
        blend_verts,
        packed_vertices: Vec::new(),
        radius: Some(radius),
        bounds: Some((mid, half)),
        contents: None,
        coll_lod: -1,
        coll_surfs: Vec::new(),
        movement_brushes: Vec::new(),
        mount_tag: None,
        lod: Some(ModelLodSelector::Iw4 {
            lod_start: 0,
            num_lods: 1,
            lod_dist: [0.0; 4],
        }),
        lod_smc: None,
        lod_part_bits: None,
        lod_surf_span: [
            (0, u16::try_from(surface_count).unwrap_or(u16::MAX)),
            (0, 0),
            (0, 0),
            (0, 0),
        ],
    })
}
