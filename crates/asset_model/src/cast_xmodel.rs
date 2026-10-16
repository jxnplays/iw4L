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

use crate::{
    BoneBind, ModelSkel, VertSkin, pack_tex_coords, pack_unit_vec, unpack_color,
};
use std::collections::HashMap;
use xmodel_runtime::ModelPoseSrc;

const MAGIC: u32 = 0x7473_6163; // "cast"

const NODE_MESH: u32 = 0x6873_656d; // "mesh"
const NODE_BONE: u32 = 0x656e_6f62; // "bone"
const NODE_MATL: u32 = 0x6c74_616d; // "matl"
const NODE_FILE: u32 = 0x656c_6966; // "file"

/// The texture maps one cast material names, as PNG file names.
#[derive(Clone, Debug, Default)]
pub struct CastMaterialTextures {
    pub specular: Option<String>,
    pub normal: Option<String>,
    pub albedo: Option<String>,
    pub detail: Option<String>,
}

/// One surface's material: its CryEngine name plus the PNG maps it declares.
#[derive(Clone, Debug)]
pub struct CastSurfaceMaterial {
    pub name: String,
    pub textures: CastMaterialTextures,
}
const NODE_ANIM: u32 = 0x6d69_6e61; // "anim"
const NODE_CURV: u32 = 0x7672_7563; // "curv"

/// Cast units are centimetres; IW4 world units are inches.
///
/// Measured, not assumed. Parent-to-child offsets in
/// `viewhands_nx_us_army_LOD0.cast`, against textbook adult dimensions:
/// `j_index_le_1` (index proximal phalanx) 4.4717 against 4.5 cm, ratio 1.006;
/// `j_index_le_2` (middle phalanx) 3.0915 against 3.0 cm, ratio 0.970;
/// `j_elbow_le` (upper arm) 30.7145 against 33 cm, ratio 1.074; `j_wrist_le`
/// (forearm) 29.3882 against 26 cm, ratio 0.885; and `tag_view` at 152.4000,
/// which is 1.524 m, exactly five feet, the conventional view height. Every
/// ratio is 1.0 to within the spread of human proportions, so a cast unit is
/// 1.006 cm and the file is in centimetres. Read as inches instead, `j_index_le_1`
/// would be 11 cm of finger.
///
/// The reciprocal is exact by definition, so the constant is the definition of
/// an inch and not another fitted number.
pub const CAST_TO_WORLD: f32 = 1.0 / 2.54;

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

    /// The value of a single 64-bit integer property, if it is one.
    ///
    /// The meshÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¾Ãƒâ€šÃ‚Â¢material link and the material's own texture ids are both
    /// `l` (int64) scalars, and both are optional: a mesh without a material
    /// has no `m`, and a material with no textures names none of them.
    fn int64(&self, name: &str) -> Option<u64> {
        let prop = self.get(name)?;
        if prop.width != Some(8) || prop.data.len() < 8 {
            return None;
        }
        Some(u64::from_le_bytes(prop.data[..8].try_into().ok()?))
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

/// Build the rest pose the renderer poses bones from.
///
/// A cast bone carries its local transform, so the pose is the file's own
/// hierarchy rather than a reconstruction.
///
/// `DObj` reads three of these arrays by *child* index ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â `bone - num_root_bones`
/// ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â and `base_mat` by bone index. Bones are therefore emitted roots first, and
/// `parent_list`, `quats` and `trans` carry one entry per non-root bone.
///
/// `parent_list` is a step, not an absolute index: `DObj` recovers the parent as
/// `bone - step` and refuses a step of zero, so a bone directly under its own
/// parent is a step of 1. Writing the absolute parent there, as an earlier
/// version of this did, is what made `DObj::build` report a parent step escaping
/// the skeleton.
///
/// `quats` is the same packed i16 xyzw the fastfile stores, each component scaled
/// by 32767. The cast rotation is already a unit quaternion in that order, so it
/// quantises rather than converts.
fn build_pose(
    name: &str,
    bone_names: &[String],
    bone_parents: &[i32],
    bones: &[BoneBind],
    // Absolute world transform per bone, straight from the file's wp/wr.
    world_bones: &[([f32; 4], [f32; 3])],
) -> Option<ModelPoseSrc> {
    if bone_names.len() != bones.len() || bones.is_empty() {
        return None;
    }
    let parent_of = |index: usize| -> i32 { *bone_parents.get(index).unwrap_or(&-1) };

    // Roots first, because `DObj` assumes bones 0..num_root_bones are roots and
    // everything after is a child. A cast skeleton already stores its root at
    // index 0; this reorders defensively rather than assuming it.
    let mut order: Vec<usize> = Vec::with_capacity(bones.len());
    let mut placed = vec![false; bones.len()];
    let mut roots: Vec<usize> = (0..bones.len()).filter(|i| parent_of(*i) < 0).collect();
    if roots.is_empty() {
        return None;
    }
    for root in roots.drain(..) {
        order.push(root);
        placed[root] = true;
    }
    // Attach each bone once its parent is placed, repeating until nothing is
    // left; a bone whose parent never arrives leaves the cycle and is refused.
    let mut progressed = true;
    while order.len() < bones.len() && progressed {
        progressed = false;
        for index in 0..bones.len() {
            if placed[index] {
                continue;
            }
            let parent = parent_of(index);
            if parent >= 0 && placed[parent as usize] {
                order.push(index);
                placed[index] = true;
                progressed = true;
            }
        }
    }
    if order.len() != bones.len() {
        return None;
    }

    let num_root_bones = order
        .iter()
        .filter(|index| parent_of(**index) < 0)
        .count();
    let mut parent_list = Vec::with_capacity(bones.len() - num_root_bones);
    let mut quats = Vec::with_capacity(bones.len() - num_root_bones);
    let mut trans = Vec::with_capacity(bones.len() - num_root_bones);
    for (position, index) in order.iter().enumerate() {
        let parent = parent_of(*index);
        if parent < 0 {
            continue;
        }
        // Map the file's absolute parent onto its new position, then take the
        // step between them.
        let parent_position = order
            .iter()
            .position(|candidate| *candidate == parent as usize)?;
        let step = position.checked_sub(parent_position)?;
        if step == 0 || step > u8::MAX as usize {
            return None;
        }
        parent_list.push(step as u8);
        quats.push(quantize_quat(bones[*index].quat));
        trans.push(bones[*index].trans);
    }

    // `base_mat` is a *world* transform per bone, and `DObj::build` uses it as
    // `bind_world` with no parent accumulation -- it feeds the right half of every
    // skin matrix (`world[b] * bind_world.inverse()`).
    //
    // `lp` is PARENT-RELATIVE. It was stored here raw, as if it were already a world
    // position, so `bind_world` was wrong for every bone that is not a direct child
    // of a root sitting on the origin. The error is `cumulative(lp chain) - lp[b]`.
    //
    // On the hands that reaches 145 cm. `j_wrist_le` has cumulative z = 144.3 but a
    // raw lp z of 0, so every hand bone was displaced by up to 57 inches and 61% of
    // hand vertices landed behind the camera. That is why the arms never appeared:
    // they were drawn, and scattered outside the view.
    //
    // Verified against the mesh itself: `j_slider_m203`'s surface spans x 23.0-48.2
    // and its cumulative position is 30.9 -- inside. Its raw lp is 7.8, outside the
    // surface entirely. Same for `j_grenade_m203`: surface x 54.8-62.1, cumulative
    // 58.5, raw lp 35.4. The mesh agrees with the cumulative reading, not the raw one.
    // `base_mat` is an absolute WORLD transform per bone, and `DObj::build` uses it
    // as `bind_world` with no parent accumulation -- it is the right half of every
    // skin matrix (`world[b] * bind_world.inverse()`), so at bind pose it must equal
    // the posed world exactly.
    //
    // The file states that transform: `wp`/`wr` on every Bone node, already
    // composed down the chain. Those are used directly. An earlier version stored
    // the local `lp`/`lr` pair here, which is what broke the weapon:
    //
    //   hands: 63 of 66 bones differ from their world transform by >1 cm, worst
    //          tag_weapon_right at 152.5 cm. 9 of 66 differ in rotation too.
    //   gun:   18 of 35 differ by >1 cm, worst 45.3 cm.
    //
    // A middle version composed the chain by hand but still took the rotation from
    // `lr`, leaving 42 of the 45 bones the hands actually deform wrong by up to
    // 180 degrees -- it cut the hand error from 59 inches to 10 and still did not
    // work. Using both `wp` and `wr` is the whole fix.
    //
    // `trans` and `quats` above stay local: the pose contract is parent-relative
    // and `DObj::compose` accumulates it down the hierarchy itself.
    // `base_mat` is an absolute WORLD transform per bone, and `DObj::build` uses it
    // as `bind_world` with no parent accumulation -- the right half of every skin
    // matrix (`world[b] * bind_world.inverse()`).
    //
    // The file states that transform directly: `wp`/`wr` on every Bone node, already
    // composed down the chain. Those are used verbatim. Storing the local `lp`/`lr`
    // pair here instead is what broke the weapon --
    //
    //   hands: 63 of 66 bones differ from their world transform by >1 cm, worst
    //          tag_weapon_right at 152.5 cm; 9 of 66 also differ in rotation, every
    //          elbow, wrist and finger.
    //   gun:   18 of 35 differ by >1 cm, worst 45.3 cm.
    //
    // A middle version composed the chain by hand but still took rotation from `lr`,
    // leaving 42 of the 45 bones the hands actually deform wrong by up to 180
    // degrees: it cut hand error from 59 inches to 10 and still did not work.
    //
    // `trans` and `quats` above stay local -- the pose contract is parent-relative
    // and `DObj::compose` accumulates that itself.
    let base_mat: Vec<(bevy::math::Quat, bevy::math::Vec3)> = order
        .iter()
        .map(|index| {
            let (rotation, translation) = world_bones[*index];
            (
                bevy::math::Quat::from_xyzw(
                    rotation[0],
                    rotation[1],
                    rotation[2],
                    rotation[3],
                ),
                bevy::math::Vec3::from_array(translation),
            )
        })
        .collect();
    let ordered_names: Vec<String> = order.iter().map(|index| bone_names[*index].clone()).collect();
    Some(ModelPoseSrc {
        name: name.to_owned(),
        num_bones: bones.len(),
        num_root_bones,
        // The pose is what carries the skel's unit scale: bone offsets are left
        // in the file's own centimetres and scaled here, so skinning reduces to
        // one multiply rather than rescaling every vertex row and every packed
        // half-float. `bounds` and `radius` above are pre-multiplied to match.
        scale: CAST_TO_WORLD,
        no_scale_part_bits: [0; 6],
        bone_names: ordered_names,
        parent_list,
        quats,
        trans,
        base_mat,
        // No `root_rest`. A root bone's bind used to be overridden here with the
        // view offset, which is the second half of the double shift described at the
        // call site: `base_mat` displaced the geometry and `root_rest` displaced the
        // posed world the assembly reads for the camera eye.
        //
        // The belief that these two "feed different consumers and are not summed"
        // came from `view_trans` moving when one was dropped. That number cannot
        // move the viewpoint -- see the algebra at the call site -- so it was never
        // evidence, and three rebuilds were spent believing it.
        root_rest: None,
    })
}

/// wrap into the opposite sign.
fn quantize_quat(quat: [f32; 4]) -> [i16; 4] {
    let pack = |value: f32| -> i16 {
        let scaled = (value * 32767.0).round();
        scaled.clamp(-32767.0, 32767.0) as i16
    };
    [pack(quat[0]), pack(quat[1]), pack(quat[2]), pack(quat[3])]
}

/// Build one IW4 packed vertex row.
///
/// The layout is fixed by `asset_iw4::vertex_decl` for `PACKED_VERTEX_TYPE`:
/// stride 32, position as a float4 at offset 0, colour at 16, the two UVs
/// packed as halves at 20, and the normal packed as a unit vector at 24. The
/// normal's scale byte carries the magnitude, so a unit input always reads
/// back as one.
fn pack_iw4_vertex(
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: u32,
) -> [u8; asset_iw4::size::GFX_PACKED_VERTEX] {
    let mut row = [0u8; asset_iw4::size::GFX_PACKED_VERTEX];
    for (offset, value) in [position[0], position[1], position[2], 0.0]
        .iter()
        .enumerate()
    {
        row[offset * 4..offset * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    row[16..20].copy_from_slice(&color.to_le_bytes());
    row[20..24].copy_from_slice(&pack_tex_coords(uv).to_le_bytes());
    row[24..28].copy_from_slice(&pack_unit_vec(normal).to_le_bytes());
    row
}

/// Read one `.cast` file into a single `ModelSkel`: one surface per `mesh`
/// node under the model, in file order, plus the `skel` bone hierarchy.
///
/// Bones and the per-vertex bind are read, not assumed. NX1 weapon skeletons
/// are tag trees whose meshes bind rigidly to `tag_weapon`, and this keeps that
/// a fact of the file rather than a claim about it.
/// Read an NX1 `.cast` mesh and the material each surface names.
///
/// The second value is one entry per surface, in surface order: the CryEngine
/// material path the surface draws with (`mc\mtl_weapon_scar_h` and the rest).
/// These are names, not catalog rows ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â this reader holds no catalog, so the
/// caller registers them and writes the resulting indices onto
/// `skel.surface_materials` itself.
pub fn read_cast_xmodel_materials(
    bytes: &[u8],
    name: &str,
) -> Result<(ModelSkel, Vec<Option<CastSurfaceMaterial>>), String> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.u32()? != MAGIC {
        return Err("cast: magic is not \"cast\"".to_owned());
    }
    let _version = cursor.u32()?;
    let _flag = cursor.u32()?;
    let _reserved = cursor.u32()?;

    // Materials first, in their own pass over the same container.
    //
    // A `mesh` node names its material by id (`m`, an int64), and a `matl` node
    // does not state its own id ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â the id sits one below the lowest of the
    // texture ids the material owns (`specular`/`normal`/`albedo`/`extra0`,
    // each also an int64). Those ids come from one monotonic counter shared by
    // materials and their textures, so a material's own id is recoverable from
    // its textures. This is resolved before the geometry pass because a file is
    // free to place a mesh before the materials it names; resolving lazily at
    // the end would miss any that do.
    let mut material_names: HashMap<u64, String> = HashMap::new();
    // id -> that material's texture map (PNG file names per slot).
    let mut material_maps: HashMap<u64, CastMaterialTextures> = HashMap::new();
    // Walk state: a material's texture paths live on `file` child nodes visited
    // immediately after their `matl`, in the same order the `matl` declares its
    // slots. Track the material being filled and the next slot a `file` lands in.
    let mut pending: Option<(u64, Vec<&'static str>, usize)> = None;
    const SLOTS: [&str; 4] = ["specular", "normal", "albedo", "extra0"];
    let mut material_scan = Cursor { bytes, at: 16 };
    walk(&mut material_scan, 0, &mut |id, node| {
        match id {
            NODE_MATL => {
                let Some(name) = node.get("n").and_then(|prop| prop.text.clone()) else {
                    pending = None;
                    return Ok(());
                };
                let declared: Vec<&'static str> = SLOTS
                    .into_iter()
                    .filter(|slot| node.int64(slot).is_some())
                    .collect();
                // The material's own id is one below the lowest texture id it
                // owns. A material that owns no texture names no slots and has
                // no id to derive, so it is left out; nothing references it.
                let owned = SLOTS.into_iter().filter_map(|slot| node.int64(slot)).min();
                if let Some(base) = owned {
                    let id = base.wrapping_sub(1);
                    material_names.insert(id, name);
                    pending = Some((id, declared, 0));
                } else {
                    pending = None;
                }
            }
            NODE_FILE => {
                if let Some((id, slots, at)) = pending.as_mut() {
                    let path = node
                        .get("p")
                        .and_then(|prop| prop.text.clone())
                        .map(|p| {
                            p.rsplit(['\\', '/']).next().unwrap_or(&p).to_owned()
                        });
                    if let Some(path) = path
                        && let Some(slot) = slots.get(*at)
                    {
                        let entry = material_maps.entry(*id).or_default();
                        match *slot {
                            "specular" => entry.specular = Some(path),
                            "normal" => entry.normal = Some(path),
                            "albedo" => entry.albedo = Some(path),
                            _ => entry.detail = Some(path),
                        }
                        *at += 1;
                    }
                }
            }
            _ => pending = None,
        }
        Ok(())
    })?;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut vert_skin: Vec<VertSkin> = Vec::new();
    let mut packed_vertices: Vec<[u8; asset_iw4::size::GFX_PACKED_VERTEX]> = Vec::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    // The material each surface names, in surface order. Names rather than
    // catalog indices: this reader has no catalog, so it cannot know the row a
    // material will occupy. `match_walk` registers the names and stamps the
    // indices onto `surface_materials` once the catalog exists.
    let mut surface_material_names: Vec<Option<CastSurfaceMaterial>> = Vec::new();
    let mut bone_names: Vec<String> = Vec::new();
    let mut bones: Vec<BoneBind> = Vec::new();
    // Absolute world transform per bone, from wp/wr, parallel to ones.
    let mut world_bones: Vec<([f32; 4], [f32; 3])> = Vec::new();
    let mut bone_parents: Vec<i32> = Vec::new();
    // Tag bones are ordinary bones carrying a reserved name, so the index is
    // found by name rather than by a node type. Absent stays absent: a gun
    // viewmodel carries attachment tags (`tag_clip`, `tag_foregrip`) and none of
    // these two, while a viewhands skeleton carries both. The renderer asks the
    // hands for them, so requiring them here would reject every gun.
    let mut tag_view: Option<usize> = None;
    let mut tag_weapon: Option<usize> = None;

    let mut visit = |id: u32, node: &Node<'_>| -> Result<(), String> {
        if id == NODE_BONE {
            // Every Bone node states its transform twice: `lp`/`lr` are the
            // PARENT-RELATIVE local transform and `wp`/`wr` are the absolute world
            // transform already composed down the chain. `wp`/`wr` are the authored
            // answer and this reader uses them directly -- see the note at the
            // `base_mat` build, where storing the local pair there is what broke
            // the weapon.
            //
            // Measured divergence between the two, per bone:
            //
            //   hands: 63 of 66 bones differ by >1 cm in position (worst
            //          tag_weapon_right, 152.5 cm) and 9 of 66 differ by >1 deg in
            //          rotation -- every elbow, wrist and finger bone.
            //   gun:   18 of 35 differ by >1 cm (worst 45.3 cm); none differ in
            //          rotation.
            //
            // `lr`/`lp` are still read: `trans` is parent-relative by contract and
            // the pose must stay local.
            let position = node.floats("lp", PROP_VECTOR3, 1)?;
            let rotation = node.floats("lr", PROP_VECTOR4, 1)?;
            let world_position = node.floats("wp", PROP_VECTOR3, 1).unwrap_or_default();
            let world_rotation = node.floats("wr", PROP_VECTOR4, 1).unwrap_or_default();
            if world_position.len() < 3 || world_rotation.len() < 4 {
                // Falling back to the local pair would silently reintroduce the bug
                // this whole change exists to fix, so a missing world transform is an
                // error rather than a wrong answer nobody notices.
                return Err("cast: bone carries no `wp`/`wr` world transform".to_owned());
            }
            bone_names.push(node.text("n")?.to_owned());
            // `p` is the parent bone index, -1 for a root.
            let parent = node
                .required("p")?
                .scalar(PROP_INT32, 1)
                .map(|data| i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
                .unwrap_or(-1);
            bone_parents.push(parent);
            let index = bone_names.len() - 1;
            match bone_names[index].as_str() {
                "tag_view" if tag_view.is_none() => tag_view = Some(index),
                "tag_weapon" if tag_weapon.is_none() => tag_weapon = Some(index),
                _ => {}
            }
            bones.push(BoneBind {
                quat: [rotation[0], rotation[1], rotation[2], rotation[3]],
                trans: [position[0], position[1], position[2]],
            });
            // The absolute world transform the file also states on every bone.
            // Kept beside the local pair rather than inside `BoneBind` because
            // `BoneBind` is constructed in four places and only this reader has
            // `wp`/`wr` to offer.
            world_bones.push((
                [
                    world_rotation[0],
                    world_rotation[1],
                    world_rotation[2],
                    world_rotation[3],
                ],
                [world_position[0], world_position[1], world_position[2]],
            ));
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
        // Winding. Cast stores every face counter-clockwise (right-handed), as its
        // spec requires; the exporters reverse IW's own clockwise order to get
        // there. The colour pass draws IW4 xmodels with `FrontFace::Cw` and culls
        // back faces, so the cast order fed through unchanged makes every outward
        // face a back face: the near side of the gun is culled and its inside
        // walls draw instead, which is what reads as a see-through weapon body.
        // Swapping the last two corners restores IW's order. The index *set* per
        // surface is unchanged, so the range tables below still hold.
        for tri in mesh_indices.chunks_exact(3) {
            indices.extend([first + tri[0], first + tri[2], first + tri[1]]);
        }
        // The packed row is the IW4 vertex the GPU reads back: the same four
        // values the decoded arrays carry, in the packed layout. W is unused.
        for vertex in 0..vertex_count {
            packed_vertices.push(pack_iw4_vertex(
                [
                    flat_positions[vertex * 3],
                    flat_positions[vertex * 3 + 1],
                    flat_positions[vertex * 3 + 2],
                ],
                [
                    flat_normals[vertex * 3],
                    flat_normals[vertex * 3 + 1],
                    flat_normals[vertex * 3 + 2],
                ],
                [flat_uvs[vertex * 2], flat_uvs[vertex * 2 + 1]],
                u32::from_le_bytes([
                    packed_colors[vertex * 4],
                    packed_colors[vertex * 4 + 1],
                    packed_colors[vertex * 4 + 2],
                    packed_colors[vertex * 4 + 3],
                ]),
            ));
        }
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
        // `m` is the id of the material this surface draws with, resolved to
        // its name and the PNG maps it names. An id no `matl` claims
        // leaves the surface unmaterialed rather than guessing.
        surface_material_names.push(
            node.int64("m").and_then(|id| {
                let name = material_names.get(&id)?.clone();
                let textures = material_maps.get(&id).cloned().unwrap_or_default();
                Some(CastSurfaceMaterial { name, textures })
            }),
        );
        Ok(())
    };

    walk(&mut cursor, 0, &mut visit)?;
    if surfaces.is_empty() {
        return Err("cast: no mesh node carried geometry".to_owned());
    }
    // NO RECENTRING. The pose is built exactly as authored.
    //
    // Each NX1 clip authors the whole rig's placement itself. In
    // `viewmodel_scar_h_idle.cast`, for instance:
    //
    //     tag_player  tz = -152.39999
    //     tag_weapon  at (33.48, -7.31, -9.68)
    //
    // `tag_view` sits at local +152.4, so the clip's root puts it at world origin --
    // the recentring is already in the data, and it is the *pose* that does it, not
    // the bind.
    //
    // This reader used to add a second -152.4 on top, via `base_mat` and again via
    // `root_rest`, which put the camera 152.4 units (2 ft) off and threw the hands
    // out to a 500-unit box. The hands diagnostic caught it: `max_move=152.400`, the
    // offset appearing in the pose change.
    //
    // It survived three rebuilds because each removal was judged on `view_trans`,
    // which provably cannot move the viewpoint -- a uniform shift T cancels out of
    // `(T*W_view)^-1 * T*W_x = W_view^-1 * W_x`. The number moved; the screen did not.
    // Three earlier versions of this fix were each validated on the wrong surface.
    let pose = build_pose(name, &bone_names, &bone_parents, &bones, &world_bones);

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
        (lo[0] + hi[0]) * 0.5 * CAST_TO_WORLD,
        (lo[1] + hi[1]) * 0.5 * CAST_TO_WORLD,
        (lo[2] + hi[2]) * 0.5 * CAST_TO_WORLD,
    ];
    let half = [
        (hi[0] - lo[0]) * 0.5 * CAST_TO_WORLD,
        (hi[1] - lo[1]) * 0.5 * CAST_TO_WORLD,
        (hi[2] - lo[2]) * 0.5 * CAST_TO_WORLD,
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

    // Per-surface part bits: the bones each surface's vertices are actually
    // weighted to.
    //
    // This must be real per-surface data. A part bit is the "this surface
    // touches this bone" marker; the renderer hides a surface when its part bits
    // intersect the weapon's hide bits (the attachment-hide tags). A blanket
    // "every part" mask therefore hides the *entire* mesh the moment the weapon
    // hides any attachment at all ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â which is exactly what happened to the NX1
    // gun: the donor SCAR row hides its sight attachments, so the hide word is
    // non-zero and all 34 surfaces AND-ed against it and were dropped. Marking
    // only the bones a surface really uses keeps attachment hiding local to the
    // attachment.
    let surface_part_bits: Vec<[u32; 6]> = surfaces
        .iter()
        .map(|surface| {
            let mut bits = [0u32; 6];
            for skin in &vert_skin[surface.vertex_base..surface.vertex_base + surface.vertex_count]
            {
                for (slot, &bone) in skin.bones.iter().enumerate() {
                    if skin.weights[slot] > 0.0 {
                        let bone = bone as usize;
                        if bone < bits.len() * 32 {
                            bits[bone >> 5] |= 0x8000_0000u32 >> (bone & 31);
                        }
                    }
                }
            }
            bits
        })
        .collect();

    let mut skel = ModelSkel {
        name: name.to_owned(),
        bone_collision: vec![None; bones.len()],
        bone_names,
        tag_view,
        tag_weapon,
        bones,
        pose,
        positions,
        normals,
        colors,
        uvs,
        indices,
        // The names are carried alongside so the caller can register them; the
        // indices themselves belong to a catalog this reader does not hold.
        surface_materials: vec![None; surface_count],
        surface_vertex_ranges: surfaces
            .iter()
            .map(|s| (s.vertex_base, s.vertex_count))
            .collect(),
        surface_index_ranges: surfaces
            .iter()
            .map(|s| (s.index_base, s.index_count))
            .collect(),
        // The part bits are the real per-surface bone usage computed above.
        surface_part_bits,
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
        packed_vertices,
        radius: Some(radius * CAST_TO_WORLD),
        bounds: Some((mid, half)),
        contents: None,
        coll_lod: -1,
        coll_surfs: Vec::new(),
        movement_brushes: Vec::new(),
        mount_tag: None,
        // No distance selector. A cast file carries one LOD and no lod_dist
        // thresholds, and writing an Iw4 selector with `lod_dist` all zero makes
        // `dpvs_iw4::xmodel_get_lod_for_dist` reject every distance: it returns a
        // LOD only when `dist < lod_dist[i]`, and a threshold of zero is never
        // greater than a distance. With no selector `smodel_camera_lod` returns
        // LOD 0 unconditionally, which is the truth for this file. The surface
        // span below already puts every surface at LOD 0.
        lod: None,
        lod_smc: None,
        lod_part_bits: None,
        lod_surf_span: [
            (0, u16::try_from(surface_count).unwrap_or(u16::MAX)),
            (0, 0),
            (0, 0),
            (0, 0),
        ],
    };
    Ok((skel, surface_material_names))
}

/// Bind-world position of `tag_view`, so it can be moved onto the origin.
///
/// Each NX1 viewmodel is authored around *its own* `tag_view`: the hands carry
/// theirs at bind Z = +152.4 with the geometry spanning Z = 110..135 in front of
/// it. The gun carries none and is authored around the origin already.
///
/// The FPV assembly resolves one view bone for the whole weapon and places every
/// part against it, so two meshes authored against different view origins cannot
/// both be right. Whichever origin wins, the other part ends up hundreds of units
/// from the camera: admitted, textured, in the draw plan, and invisible.
/// Read an NX1 `.cast` mesh, discarding the per-surface material names.
pub fn read_cast_xmodel(bytes: &[u8], name: &str) -> Result<ModelSkel, String> {
    read_cast_xmodel_materials(bytes, name).map(|(skel, _)| skel)
}

/// A rig's bind translation per bone name.
///
/// A `relative` curve's values are an offset *from* the bone's authored position, so
/// resolving one needs the bind pose it is relative to. The `.cast` animation file
/// does not carry it -- it lives in the mesh file -- so the caller supplies it.
pub type BindTranslations = std::collections::HashMap<String, [f32; 3]>;

/// One bone's channel in an NX1 `.cast` animation.
///
/// Read from a `curv` node. `nn` names the bone, `kp` names the channel, `kb`
/// holds one frame index per key, and `kv` holds the matching values: four
/// floats for `rq`, one for each of `tx`/`ty`/`tz`.
#[derive(Debug, Clone)]
struct CastCurve {
    bone: String,
    /// `rq` (quaternion xyzw), `tx`, `ty`, or `tz`.
    channel: String,
    /// One entry per key. Quaternion channels hold 4 floats, translation 1.
    values: Vec<f32>,
    /// Frame index per key, as the file states them.
    frames: Vec<u16>,
    /// The curve's `m` property: `"absolute"` or `"relative"`.
    ///
    /// This reader used to treat every curve as absolute, which is what it is for
    /// 628,967 of the 632,510 curves in the NX1 tree. The other 3,543 are
    /// `relative`, and they are the ADS transitions -- `tag_ads` on
    /// `viewmodel_scar_h_ads_down.cast` and its siblings is the bulk of them.
    /// Applied absolutely, a relative curve replaces the bone's authored position
    /// with an offset from it, so the sight jumps to the origin instead of leaning
    /// in. They are translation-only; no relative `rq` curve exists.
    mode: CurveMode,
}

/// How a `curv` node's values combine with the bone's bind pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveMode {
    /// Replaces the bind transform. The overwhelming majority of curves.
    Absolute,
    /// An offset *added* to the bind transform.
    Relative,
}

/// Read an NX1 `.cast` animation into an [`AnimClip`].
///
/// The container is the same one `read_cast_xmodel` walks; this reads the
/// `anim` node's `curv` children instead of its `mesh` nodes. An `anim` node
/// carries `fr` (frames per second, 30 in every file checked) and `lo` (loop
/// flag), and each `curv` child is one bone channel. There is no mesh node, which
/// is why `read_cast_xmodel` rejects these files outright.
///
/// Bones are matched to tracks by name, exactly as the XAnim path does, so a
/// bone the mesh does not have simply has no track and holds its bind pose.
/// Tracks are emitted only for bones the file animates; bones absent from the
/// file are not invented, so the rig keeps their bind transform.
pub fn read_cast_xanim(
    bytes: &[u8],
    name: &str,
    bind: &BindTranslations,
) -> Result<xmodel_runtime::AnimClip, String> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.u32()? != MAGIC {
        return Err("cast: magic is not \"cast\"".to_owned());
    }
    let _version = cursor.u32()?;
    let _flag = cursor.u32()?;
    let _reserved = cursor.u32()?;

    let mut framerate = 30.0f32;
    let mut looping = false;
    let mut curves: Vec<CastCurve> = Vec::new();

    walk(&mut cursor, 0, &mut |id, node| {
        if id == NODE_ANIM {
            if node.get("fr").is_some() {
                framerate = node
                    .floats("fr", PROP_FLOAT, 1)
                    .map(|v| v[0])
                    .unwrap_or(framerate);
            }
            if node.get("lo").is_some() {
                looping = node
                    .all_floats("lo", PROP_BYTE)
                    .is_ok_and(|v| v.first().copied().unwrap_or(0.0) != 0.0);
            }
            return Ok(());
        }
        if id != NODE_CURV {
            return Ok(());
        }
        let bone = node.text("nn")?.to_owned();
        let channel = node.text("kp")?.to_owned();
        // `m` states how the values combine with the bone's bind pose. It is absent
        // on some curves and means absolute there; only `"relative"` means an offset.
        let mode = match node.text("m") {
            Ok(other) if other == "relative" => CurveMode::Relative,
            _ => CurveMode::Absolute,
        };
        // `kb` is one byte per key when the frame count fits, else two. Read the
        // declared width rather than assuming, so a long clip is not misread.
        let kb = node.required("kb")?;
        let key_width = match kb.width {
            Some(w) if w == 1 || w == 2 => w,
            _ => return Err("cast: `kb` is neither a byte nor a short array".to_owned()),
        };
        let key_count = kb.count;
        let frames: Vec<u16> = kb
            .data
            .chunks_exact(key_width)
            .map(|c| {
                if key_width == 1 {
                    u16::from(c[0])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        let floats_per_key = if channel == "rq" { 4 } else { 1 };
        let expect = key_count.saturating_mul(floats_per_key);
        // `kv` holds floats. A quaternion key is xyzw and a translation key is
        // one float, but the file declares the payload either as a scalar-float
        // array or as a 4-wide vector array depending on the channel, so accept
        // whichever width the container states and read every float present.
        let values = node
            .all_floats("kv", PROP_FLOAT)
            .or_else(|_| node.all_floats("kv", PROP_VECTOR4))?;
        if values.len() < expect {
            return Err(format!(
                "cast: bone `{bone}` channel `{channel}` states {key_count} keys but holds {} values",
                values.len()
            ));
        }
        curves.push(CastCurve {
            bone,
            channel,
            values: values[..expect].to_vec(),
            frames,
            mode,
        });
        Ok(())
    })?;

    if curves.is_empty() {
        return Err("cast: no curv node carried a bone channel".to_owned());
    }

    // Group channels by bone. A bone's rotation and its three translation axes
    // are separate `curv` nodes and must be joined, not emitted separately.
    let mut order: Vec<String> = Vec::new();
    for curve in &curves {
        if !order.contains(&curve.bone) {
            order.push(curve.bone.clone());
        }
    }

    let mut tracks = Vec::with_capacity(order.len());
    let mut max_frame = 0u32;
    for bone in &order {
        let own = |channel: &str| curves.iter().find(|c| &c.bone == bone && c.channel == channel);
        let rotation_curve = own("rq");
        let tx = own("tx");
        let ty = own("ty");
        let tz = own("tz");

        let rotation = match rotation_curve {
            Some(curve) => {
                let quats: Vec<[f32; 4]> = curve
                    .values
                    .chunks_exact(4)
                    .map(|c| [c[0], c[1], c[2], c[3]])
                    .collect();
                xmodel_runtime::Rotation::FullKeyed(xmodel_runtime::Keyed {
                    values: quats,
                    frames: frame_indices(&curve.frames),
                })
            }
            None => xmodel_runtime::Rotation::Default,
        };

        let translation = match (tx, ty, tz) {
            (Some(tx), Some(ty), Some(tz)) => {
                let frames = frame_indices(&tx.frames);
                // A relative curve's values are an *offset* from the bone's authored
                // position, so the bind translation has to be folded in here or the
                // bone is read as sitting at the offset itself. `base` is the bind
                // translation this clip's rig carries for `bone`; it is zero for an
                // absolute curve, and for a bone the caller has no bind for.
                let relative = tx.mode == CurveMode::Relative;
                // Visible, not silent. A relative curve resolved against an empty
                // bind is still wrong; it just fails quietly unless it says so. This
                // is the ADS case: the caller has no rig at this point and should be
                // told rather than left to look right in a log that never mentions it.
                let base = if relative && !bind.contains_key(bone) {
                    eprintln!(
                        "nx1 anim: {name} carries a `relative` curve on `{bone}` but no \
                         bind translation was supplied; reading it as absolute"
                    );
                    [0.0; 3]
                } else {
                    bind.get(bone).copied().unwrap_or([0.0; 3])
                };
                let values: Vec<[f32; 3]> = (0..tx.values.len())
                    .map(|i| {
                        // tx is X, ty is Y, tz is Z.
                        //
                        // This used to assemble [ty, tz, tx], which rotates every
                        // animated translation by one axis. It was invisible for most
                        // of the tree and catastrophic for the view bones:
                        // viewmodel_scar_h_idle authors tag_player tz = -152.4 to push
                        // the rig back along the view axis, and the rotation delivered
                        // it on Y, dropping the hands 152.4 units below the gun. That
                        // is the raised viewpoint -- the gun was never high, the arms
                        // were down.
                        //
                        // tag_weapon shows the same rotation: authored tx=33.48,
                        // ty=-7.31, tz=-9.68 -- 33 lateral, 7 down, 9 forward, an
                        // ordinary grip offset -- and read as (-7.31, -9.68, 33.48),
                        // sideways, sunken, and 33 units toward the camera.
                        let mut v = [
                            tx.values[i],
                            *ty.values.get(i).unwrap_or(&0.0),
                            *tz.values.get(i).unwrap_or(&0.0),
                        ];
                        if relative {
                            for axis in 0..3 {
                                v[axis] += base[axis];
                            }
                        }
                        v
                    })
                    .collect();
                xmodel_runtime::Translation::FullKeyed(xmodel_runtime::Keyed { values, frames })
            }
            _ => xmodel_runtime::Translation::Default,
        };

        for curve in [&rotation_curve, &tx, &ty, &tz].into_iter().flatten() {
            if let Some(&last) = curve.frames.last() {
                max_frame = max_frame.max(u32::from(last));
            }
        }
        tracks.push(xmodel_runtime::Track {
            name: bone.clone(),
            rotation,
            translation,
        });
    }

    let numframes = u16::try_from(max_frame.saturating_add(1)).unwrap_or(u16::MAX);
    Ok(xmodel_runtime::AnimClip {
        name: name.to_owned(),
        framerate,
        numframes,
        looping,
        tracks,
        notifies: Vec::new(),
        delta_translation: xmodel_runtime::Translation::Default,
    })
}

/// Frame indices as the runtime wants them: dense when they run 0..n with no
/// gaps, which every NX1 clip so far checked does, and sparse otherwise.
fn frame_indices(frames: &[u16]) -> xmodel_runtime::FrameIndices {
    if frames.windows(2).all(|w| w[1] == w[0] + 1) {
        xmodel_runtime::FrameIndices::Dense
    } else {
        xmodel_runtime::FrameIndices::Sparse(frames.to_vec())
    }
}








