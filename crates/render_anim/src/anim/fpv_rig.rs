//! The prepared first-person rig.
//!

use std::collections::HashMap;
use std::sync::Arc;

use bevy::math::{Mat4, Vec3};
use render_scene::SmodelPassMaterial;

use crate::anim::fpv_pose::{
    FpvBoltFrame, FpvBoltTags, PosedClip, tag_camera_lens_local, tag_view_to_bevy_camera,
};
use crate::anim::xmodel_pose::{FpvSurfOwner, SkinLayout, build_skin_layout, skin_packed_into};
use crate::draw::FpvSurfaceDraw;
use anim_iw4::PartBits;
use anim_iw4::surface_hidden;
use asset_game::{FpvAssembly, FpvClipTracks, FpvPartRole};
use asset_model::FpvMeshCatalog;
use xmodel_runtime::{AnimInstance, DObj};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FpvSurfaceVerdict {
    Admitted(u32),
    Inapplicable(&'static str),
    Refused {
        material: String,
        cause: &'static str,
    },
}

pub(super) struct FpvMaterialAdmission {
    meshes: Arc<FpvMeshCatalog>,
    material_catalog: Arc<render_material::RuntimeMaterialCatalog>,
    pub(super) materials: Vec<SmodelPassMaterial>,
    pub(super) by_authored: HashMap<usize, u32>,
    verdicts: HashMap<usize, Vec<(usize, FpvSurfaceVerdict)>>,
}

impl FpvMaterialAdmission {
    pub(super) fn new(
        meshes: Arc<FpvMeshCatalog>,
        material_catalog: Arc<render_material::RuntimeMaterialCatalog>,
    ) -> Self {
        Self {
            meshes,
            material_catalog,
            materials: Vec::new(),
            by_authored: HashMap::new(),
            verdicts: HashMap::new(),
        }
    }

    pub(super) fn owns_materials(
        &self,
        materials: &render_material::RuntimeMaterialCatalog,
    ) -> bool {
        std::ptr::eq(self.material_catalog.as_ref(), materials)
    }

    pub fn record(&mut self, catalog_entry: usize, verdicts: Vec<(usize, FpvSurfaceVerdict)>) {
        self.verdicts.insert(catalog_entry, verdicts);
    }

    pub fn verdict(&self, catalog_entry: usize, surface: usize) -> Option<&FpvSurfaceVerdict> {
        self.verdicts
            .get(&catalog_entry)?
            .iter()
            .find(|(index, _)| *index == surface)
            .map(|(_, verdict)| verdict)
    }

    pub fn verdicts_of(&self, catalog_entry: usize) -> &[(usize, FpvSurfaceVerdict)] {
        self.verdicts
            .get(&catalog_entry)
            .map_or(&[], |verdicts| verdicts.as_slice())
    }
}

/// Hands clip-versus-draw diagnostic, pose side. `pose_clips` fills this in for
/// hand 0 every time it runs, including on each early return, and
/// `hands_draw_report` reads it after skinning and prints one verdict line.
/// It answers one question: were the hands not animated, or not drawn?
#[derive(Clone, Debug, Default)]
pub struct FpvHandsClipDiag {
    /// "posed", or why `pose_clips` returned `None`.
    pub exit: &'static str,
    /// Per right-hand clip: (node, time, weight, tracks landing on hands bones).
    pub clips: Vec<(usize, f32, f32, usize)>,
    pub tracks_total: usize,
    pub tracks_to_hands: usize,
    pub hands_bones: usize,
    /// Hands bones the hand-part mask lets through (None = no mask). Full-body
    /// clips skip the mask, so this only matters for partial clips.
    pub mask_hands_bones: Option<usize>,
    /// Hands bones whose parent-relative transform differs from bind.
    pub moved_bones: usize,
    pub max_move: f32,
    pub max_turn_deg: f32,
    pub max_turn_bone: String,
}

static HANDS_CLIP_DIAG: std::sync::Mutex<Option<FpvHandsClipDiag>> = std::sync::Mutex::new(None);

fn record_hands_clip(diag: FpvHandsClipDiag) {
    if let Ok(mut slot) = HANDS_CLIP_DIAG.lock() {
        *slot = Some(diag);
    }
}

#[derive(Clone, Copy, Debug)]
struct PreparedFpvSurface {
    index_start: u32,
    index_count: u32,
    material: u32,
    authored: usize,
    lens_named: bool,
}

pub struct PreparedFpvModel {
    hide: Option<[u32; 6]>,
    mesh_identity: u64,
    catalog_entry: usize,
    posed_surface_n: usize,
    packed_ok: bool,
    layout: SkinLayout,
    surfaces: Vec<PreparedFpvSurface>,
    refusal: Option<FpvMandatoryRefusal>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FpvMandatoryRefusal {
    pub model: String,
    pub surface: usize,
    pub material: String,
    pub cause: &'static str,
}

impl core::fmt::Display for FpvMandatoryRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} surface {} `{}`: {}",
            self.model, self.surface, self.material, self.cause
        )
    }
}

#[derive(Debug)]
pub enum FpvRigError {
    Catalog(&'static str),
    Layout(&'static str),
}

impl core::fmt::Display for FpvRigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Catalog(what) => write!(f, "{what} missing from the first-person catalog"),
            Self::Layout(what) => write!(f, "{what}"),
        }
    }
}

pub fn leftover_scope_surf_is_lens(name: &str) -> bool {
    name.contains("lens")
}

impl PreparedFpvModel {
    pub(super) fn build(
        catalog: &FpvMeshCatalog,
        catalog_entry: usize,
        hide: Option<&[u32; 6]>,
        admission: &FpvMaterialAdmission,
    ) -> Result<Self, FpvRigError> {
        if catalog.identity() != admission.meshes.identity() {
            return Err(FpvRigError::Layout(
                "material admission belongs to another mesh publication",
            ));
        }
        if catalog.identity() == 0 {
            return Err(FpvRigError::Catalog("published mesh owner"));
        }
        let entry = catalog
            .get_at(catalog_entry)
            .ok_or(FpvRigError::Catalog("model"))?;
        let skel = &entry.skel;
        let lod_range = skel.surfaces_for_lod(0);
        let first_surface = lod_range.start;
        let hidden = |surface: usize| {
            hide.is_some_and(|words| {
                skel.surface_part_bits
                    .get(surface)
                    .is_some_and(|bits| surface_hidden(bits, words, 0))
            })
        };
        let admitted = |surface: usize| match admission.verdict(catalog_entry, surface) {
            Some(FpvSurfaceVerdict::Admitted(row)) => Some(*row),
            _ => None,
        };
        let layout = build_skin_layout(skel, 0, |lod_local| {
            let surface = first_surface + lod_local;
            !hidden(surface) && admitted(surface).is_some()
        })
        .ok_or(FpvRigError::Layout("skin layout refused the model"))?;
        if skel.name.starts_with("nx1_") {
            // Which surfaces actually draw, and how big are they. The gun body
            // is one large surface; if it is hidden while only tiny attachments
            // draw, the weapon reads as "no gun" even though surfaces admitted.
            let mut drawn = String::new();
            for surface in &layout.surfaces {
                if !surface.visible || surface.index_count == 0 {
                    continue;
                }
                let _ = surface;
            }
            for surface in lod_range.clone() {
                let verdict = admission.verdict(catalog_entry, surface);
                let state = match verdict {
                    Some(FpvSurfaceVerdict::Admitted(_)) => "OK",
                    Some(FpvSurfaceVerdict::Refused { cause, .. }) => {
                        diag::info!(Fpv, "fpv nx1 {} surface {surface} refused: {cause}", skel.name);
                        "REFUSED"
                    }
                    Some(FpvSurfaceVerdict::Inapplicable(why)) => {
                        diag::info!(Fpv, "fpv nx1 {} surface {surface} inapplicable: {why}", skel.name);
                        "INAPP"
                    }
                    None => "NONE",
                };
                let idx = skel
                    .surface_index_ranges
                    .get(surface)
                    .map(|&(_, count)| count)
                    .unwrap_or(0);
                if state == "OK" {
                    drawn.push_str(&format!(" {surface}:{idx}"));
                }
            }
            diag::info!(
                Fpv,
                "fpv nx1 rig {}: admitted surfaces (idx:count) ={drawn}",
                skel.name
            );
        }

        let mut refusal = None;
        for surface in lod_range.clone() {
            if hidden(surface) {
                continue;
            }
            if let Some(FpvSurfaceVerdict::Refused { material, cause }) =
                admission.verdict(catalog_entry, surface)
            {
                refusal = Some(FpvMandatoryRefusal {
                    model: skel.name.clone(),
                    surface,
                    material: material.clone(),
                    cause: *cause,
                });
                break;
            }
        }

        let mut surfaces = Vec::new();
        for surface in &layout.surfaces {
            if !surface.visible || surface.index_count == 0 {
                continue;
            }
            let Some(authored) = entry
                .material_edges
                .get(surface.surface_index)
                .and_then(|edge| edge.bound_index())
            else {
                continue;
            };
            let Some(material) = admitted(surface.surface_index) else {
                continue;
            };
            surfaces.push(PreparedFpvSurface {
                index_start: surface.index_start,
                index_count: surface.index_count,
                material,
                authored,
                lens_named: entry
                    .material_keys
                    .get(surface.surface_index)
                    .and_then(|key| Some(key.as_ref()?.name.as_str()))
                    .is_some_and(leftover_scope_surf_is_lens),
            });
        }
        // Diagnostic: for an NX1 mesh, report exactly why surfaces are admitted or
// dropped at the layout stage. The rig hides any surface that is not admitted
// and drops the model when nothing is left, all without a log line, so this is
// the only place that can say whether the gun reached the draw.
if skel.name.starts_with("nx1_") {
    let mut admitted = 0usize;
    let mut refused = 0usize;
    let mut hidden_n = 0usize;
    for surface in lod_range.clone() {
        if hidden(surface) {
            hidden_n += 1;
            continue;
        }
        match admission.verdict(catalog_entry, surface) {
            Some(FpvSurfaceVerdict::Admitted(_)) => admitted += 1,
            Some(FpvSurfaceVerdict::Refused { material, cause }) => {
                refused += 1;
                if refused <= 3 {
                    diag::info!(
                        Fpv,
                        "fpv nx1 rig {}: surface {surface} refused, material={material}, cause={cause}",
                        skel.name
                    );
                }
            }
            Some(FpvSurfaceVerdict::Inapplicable(why)) => {
                refused += 1;
                if refused <= 3 {
                    diag::info!(
                        Fpv,
                        "fpv nx1 rig {}: surface {surface} inapplicable, why={why}",
                        skel.name
                    );
                }
            }
            None => refused += 1,
        }
    }
    diag::info!(
        Fpv,
        "fpv nx1 rig {}: surfaces={} admitted={admitted} not_admitted={refused} hidden={hidden_n} packed_ok={} hide_words={:?} part_bits0={:?}",
        skel.name,
        lod_range.len(),
        skel.packed_vertices.len() == skel.positions.len(),
        hide.copied(),
        skel.surface_part_bits.first()
    );
}
Ok(Self {
            hide: hide.copied(),
            mesh_identity: catalog.identity(),
            catalog_entry,
            posed_surface_n: lod_range.len(),
            packed_ok: skel.packed_vertices.len() == skel.positions.len(),
            layout,
            surfaces,
            refusal,
        })
    }

    pub fn refusal(&self) -> Option<&FpvMandatoryRefusal> {
        self.refusal.as_ref()
    }
}

struct ComposedPart {
    model: Arc<PreparedFpvModel>,
    owner: FpvSurfOwner,
    bone_base: usize,
}

pub struct PreparedFpvComposition {
    assembly: Arc<FpvAssembly>,
    parts: Vec<ComposedPart>,
    refusal: Option<FpvMandatoryRefusal>,
}

fn surf_owner(role: FpvPartRole) -> FpvSurfOwner {
    match role {
        FpvPartRole::Hands => FpvSurfOwner::Hands,
        FpvPartRole::Gun | FpvPartRole::Knife => FpvSurfOwner::Gun,
        FpvPartRole::Attachment => FpvSurfOwner::Scope,
        FpvPartRole::Rocket => FpvSurfOwner::Rocket,
    }
}

impl PreparedFpvComposition {
    pub fn compose(
        assembly: Arc<FpvAssembly>,
        mut model_of: impl FnMut(usize, Option<[u32; 6]>) -> Result<Arc<PreparedFpvModel>, String>,
    ) -> Result<Self, String> {
        let mut parts = Vec::with_capacity(assembly.parts().len());
        let mut refusal = None;
        for part in assembly.parts() {
            let model = model_of(part.model.order(), part.hide)?;
            if model.mesh_identity != assembly.mesh_identity()
                || model.catalog_entry != part.model.order()
                || model.hide != part.hide
            {
                return Err("FPV model or hide layout differs from the prepared assembly".into());
            }
            if refusal.is_none() {
                refusal = model.refusal().cloned();
            }
            parts.push(ComposedPart {
                model,
                owner: surf_owner(part.role),
                bone_base: part.bone_base,
            });
        }
        Ok(Self {
            assembly,
            parts,
            refusal,
        })
    }

    pub fn assembly(&self) -> &Arc<FpvAssembly> {
        &self.assembly
    }

    pub fn refusal(&self) -> Option<&FpvMandatoryRefusal> {
        self.refusal.as_ref()
    }
}

pub fn compose_clip_tracks(
    assembly: &FpvAssembly,
    clip_index: usize,
    clip: &asset_anim::AnimClip,
    tracks: &FpvClipTracks,
) -> Option<Arc<[u16]>> {
    let composed = assembly.compose_tracks(clip_index, clip, tracks)?;
    Some(
        composed
            .into_iter()
            .map(|bone| {
                bone.and_then(|bone| u16::try_from(bone).ok())
                    .unwrap_or(FpvClipTracks::NONE)
            })
            .collect(),
    )
}

/// One model drawn for one hand. The same prepared model appears twice in a
/// dual-wield rig — once per hand — with a destination range of its own.
struct PlanSlot {
    hand: usize,
    part: usize,
    dest_base: usize,
}

pub struct PreparedFpvGeometry {
    segments: Vec<(Arc<PreparedFpvModel>, u32)>,
    pub index_n: usize,
    pub surface_ranges: Vec<(u32, u32)>,
    pub materials: Vec<SmodelPassMaterial>,
    pub material_authored: Vec<usize>,
    pub draws: Vec<FpvSurfaceDraw>,
    pub dest_n: usize,
    pub packed_ok: bool,

    pub hands_plan_n: u32,
    pub gun_plan_n: u32,
    pub scope_plan_n: u32,
    pub scope_house_plan_n: u32,
    pub scope_lens_plan_n: u32,
    pub plan_draw_n: u32,
    pub plan_skip_n: u32,
}

impl PreparedFpvGeometry {
    pub fn write_indices(&self, out: &mut Vec<u32>) {
        out.clear();
        out.reserve(self.index_n);
        for (model, rebase) in &self.segments {
            out.extend(model.layout.indices.iter().map(|index| index + rebase));
        }
    }
}

pub struct FpvHandPose {
    generation: u64,
    skin: Vec<Mat4>,
    eye_from_world: Mat4,
    offset: Vec3,
    pub lens: Mat4,
    pub bolt: FpvBoltFrame,
}

pub struct PreparedFpvRig {
    meshes: Arc<FpvMeshCatalog>,
    clips: [Vec<Option<Arc<asset_anim::AnimClip>>>; 2],
    composition: Arc<PreparedFpvComposition>,
    dual: bool,
    generation: u64,
    parts: PartBits,
    tags: FpvBoltTags,
    secondary_tags: Option<FpvBoltTags>,
    slots: Vec<PlanSlot>,
    tracks: [Vec<Option<Arc<[u16]>>>; 2],
    hand_parts: Option<[PartBits; 2]>,
    pub geometry: PreparedFpvGeometry,
}

impl PreparedFpvRig {
    /// A plan holding this generation is a plan whose indices, ranges,
    /// materials and draws are this rig's.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Name of the gun mesh `skin_into` reads. This is the catalog entry the
    /// composition was built from, not the weapon row's `gun_xmodel`.
    pub fn gun_mesh_name(&self) -> Option<&str> {
        self.gun_part().and_then(|entry| self.meshes.name_at(entry))
    }

    /// Bone count of the skel, and the gun surface count written into the plan.
    ///
    /// The surface count is `gun_plan_n`, the draws `install_prepared_fpv_plan`
    /// copies. It is not the catalog entry's stored surface list.
    pub fn gun_mesh_shape(&self) -> Option<(usize, usize)> {
        let draws = self.geometry.gun_plan_n as usize;
        if draws == 0 {
            return Some((0, 0));
        }
        let entry = self.gun_part()?;
        let skel = &self.meshes.get_at(entry)?.skel;
        Some((skel.bones.len(), draws))
    }

    fn gun_part(&self) -> Option<usize> {
        self.composition.parts.iter().find_map(|part| {
            (part.owner == FpvSurfOwner::Gun).then_some(part.model.catalog_entry)
        })
    }

    pub fn is_dual(&self) -> bool {
        self.dual
    }

    /// Index count and destination vertex count of the hands already in this plan.
    /// The parsed gun is added after this span. It does not replace it.
    pub fn hands_span(&self) -> Option<(usize, usize)> {
        let slot = self.slots.first()?;
        let part = self.composition.parts.get(slot.part)?;
        if part.owner != FpvSurfOwner::Hands {
            return None;
        }
        Some((
            part.model.layout.indices.len(),
            part.model.layout.dest_vertex_n,
        ))
    }

    /// Eye-space `tag_weapon` on the hands already in this view.
    ///
    /// The parsed gun attaches here. Placing its origin at the camera puts the
    /// camera inside the mesh.
    pub fn gun_view_parent(&self, pose: &FpvHandPose) -> Option<Mat4> {
        let dobj = self.composition.assembly.dobj();
        let index = dobj.find("tag_weapon")?;
        let skin = pose.skin.get(index)?;
        let bind = dobj.bones.get(index)?.bind_world;
        let placed = pose.eye_from_world * *skin * bind;
        if pose.offset != Vec3::ZERO {
            Some(Mat4::from_translation(pose.offset) * placed)
        } else {
            Some(placed)
        }
    }

    /// Parsed viewmodel surfaces, skinned once with the scale already on the skel
    /// and parented to this view's `tag_weapon`. Not the world model, and not a
    /// second multiply of the cast positions.
    pub fn parsed_gun_rows(
        &self,
        pose: &FpvHandPose,
        skel: &asset_model::ModelSkel,
    ) -> Option<(
        Vec<u32>,
        Vec<(u32, u32)>,
        Vec<[u8; asset_iw4::size::GFX_PACKED_VERTEX]>,
    )> {
        if skel.name == "nx1_weapon_scar2" || skel.pose.is_none() {
            return None;
        }
        let parent = self.gun_view_parent(pose)?;
        let pose_src = skel.pose.as_ref()?;
        let scale = pose_src.scale;
        if !scale.is_finite() || scale == 0.0 {
            return None;
        }
        let dobj = DObj::build(&[(pose_src, None)]).ok()?;
        let world = dobj.pose(&[], &PartBits::default(), Mat4::IDENTITY);
        let skin = dobj.skin_matrices(&world);
        let layout = build_skin_layout(skel, 0, |_| true)?;
        let ranges: Vec<(u32, u32)> = layout
            .surfaces
            .iter()
            .filter(|surface| surface.visible && surface.index_count > 0)
            .map(|surface| (surface.index_start, surface.index_count))
            .collect();
        if ranges.len() != skel.surface_index_ranges.len() || layout.dest_vertex_n == 0 {
            return None;
        }
        let mut rows = vec![[0u8; asset_iw4::size::GFX_PACKED_VERTEX]; layout.dest_vertex_n];
        let scale_m = Mat4::from_scale(Vec3::splat(scale));
        skin_packed_into(
            skel,
            |bone| parent * scale_m * skin.get(bone).copied().unwrap_or(Mat4::IDENTITY),
            |_| false,
            0,
            &layout,
            &mut rows,
        );
        Some((layout.indices, ranges, rows))
    }

    pub(super) fn build(
        composition: Arc<PreparedFpvComposition>,
        dual: bool,
        admission: &FpvMaterialAdmission,
        tracks: [Vec<Option<Arc<[u16]>>>; 2],
        meshes: Arc<FpvMeshCatalog>,
        clips: [Vec<Option<Arc<asset_anim::AnimClip>>>; 2],
    ) -> Self {
        assert_eq!(composition.assembly.mesh_identity(), meshes.identity());
        assert_eq!(admission.meshes.identity(), meshes.identity());
        let parts_n = composition.parts.len();

        // Every hand's view hands first, then every hand's gun and whatever
        // hangs off it.
        let hand_n = if dual && !composition.assembly.combined_hands() {
            2
        } else {
            1
        };
        let mut order: Vec<(usize, usize)> = Vec::new();
        for hand in 0..hand_n {
            order.push((hand, 0));
        }
        for hand in 0..hand_n {
            for part in 1..parts_n {
                // The scope and the rocket hang off the right gun only.
                if hand == 1 && part != 1 {
                    continue;
                }
                order.push((hand, part));
            }
        }

        let mut geometry = PreparedFpvGeometry {
            segments: Vec::with_capacity(order.len()),
            index_n: 0,
            surface_ranges: Vec::new(),
            materials: Vec::new(),
            material_authored: Vec::new(),
            draws: Vec::new(),
            dest_n: 0,
            packed_ok: true,
            hands_plan_n: 0,
            gun_plan_n: 0,
            scope_plan_n: 0,
            scope_house_plan_n: 0,
            scope_lens_plan_n: 0,
            plan_draw_n: 0,
            plan_skip_n: 0,
        };
        let mut slots: Vec<PlanSlot> = Vec::with_capacity(order.len());
        let mut material_key: HashMap<usize, u32> = HashMap::new();
        let mut dest_base = 0usize;
        let mut posed_surface_n = 0usize;
        for (hand, part_index) in order {
            let part = &composition.parts[part_index];
            let model = &part.model;
            posed_surface_n += model.posed_surface_n;
            geometry.packed_ok &= model.packed_ok;
            let index_base = geometry.index_n as u32;
            for surface in &model.surfaces {
                let Some(material) = admission.materials.get(surface.material as usize) else {
                    continue;
                };
                let is_scope = part.owner == FpvSurfOwner::Scope;
                let is_lens = is_scope && surface.lens_named;
                let range_index = geometry.surface_ranges.len() as u32;
                geometry
                    .surface_ranges
                    .push((index_base + surface.index_start, surface.index_count));
                let material_index = *material_key.entry(surface.authored).or_insert_with(|| {
                    let index = geometry.materials.len() as u32;
                    geometry.materials.push(material.clone());
                    geometry.material_authored.push(surface.authored);
                    index
                });
                geometry.draws.push(FpvSurfaceDraw {
                    surface: range_index,
                    material: material_index,
                    is_scope: is_scope && !is_lens,
                });
                if hand == 0 {
                    match part.owner {
                        FpvSurfOwner::Hands => geometry.hands_plan_n += 1,
                        _ => geometry.gun_plan_n += 1,
                    }
                }
                if is_scope {
                    geometry.scope_plan_n += 1;
                    if is_lens {
                        geometry.scope_lens_plan_n += 1;
                    } else {
                        geometry.scope_house_plan_n += 1;
                    }
                }
            }
            geometry
                .segments
                .push((Arc::clone(model), dest_base as u32));
            geometry.index_n += model.layout.indices.len();
            slots.push(PlanSlot {
                hand,
                part: part_index,
                dest_base,
            });
            dest_base += model.layout.dest_vertex_n;
        }
        geometry.dest_n = dest_base;
        geometry.plan_draw_n = geometry.draws.len() as u32;
        geometry.plan_skip_n = posed_surface_n.saturating_sub(geometry.draws.len()) as u32;

        let assembly = &composition.assembly;
        let tags = FpvBoltTags {
            flash: assembly.tags().flash,
            flash_silenced: assembly.tags().flash_silenced,
            brass: assembly.tags().brass,
            knife: assembly.tags().knife,
            laser: assembly.tags().laser,
            tracker_screen: assembly.tags().tracker_screen,
            tracker_light: assembly.tags().tracker_light,
        };
        let secondary_tags = assembly.combined_hands().then(|| {
            let tag = |name: &str| {
                assembly
                    .dobj()
                    .find(name)
                    .and_then(|index| u16::try_from(index).ok())
            };
            FpvBoltTags {
                flash: tag("tag_flash1"),
                flash_silenced: tag("tag_flash_silenced1"),
                brass: tag("tag_brass1"),
                ..FpvBoltTags::default()
            }
        });
        let parts = assembly.dobj().all_parts();

        static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let hand_parts = composition.assembly.combined_hands().then(|| {
            let mut masks = [PartBits::default(); 2];
            let bones = &composition.assembly.dobj().bones;
            for (index, bone) in bones.iter().enumerate() {
                let mut ancestor = Some(index);
                let mut left = bone.model == 2;
                while let Some(parent) = ancestor {
                    let name = &bones[parent].name;
                    if name == "j_shoulder_le" || name == "tag_weapon1" {
                        left = true;
                        break;
                    }
                    ancestor = bones[parent].parent;
                }
                masks[usize::from(left)].set(index);
            }
            masks
        });
        Self {
            hand_parts,
            meshes,
            clips,
            composition,
            dual,
            generation: GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            parts,
            tags,
            secondary_tags,
            slots,
            tracks,
            geometry,
        }
    }

    /// One hand's bones for this frame. No skeleton is built here.
    pub fn pose_hand(
        &self,
        hand: usize,
        anims: &[PosedClip<'_>],
        offset: Vec3,
    ) -> Option<FpvHandPose> {
        self.pose_clips(hand, anims, &[], offset)
    }

    pub fn combines_hands(&self) -> bool {
        self.composition.assembly.combined_hands()
    }

    pub fn secondary_bolt(&self, pose: &FpvHandPose) -> Option<FpvBoltFrame> {
        if pose.generation != self.generation {
            return None;
        }
        Some(FpvBoltFrame {
            bones: pose.bolt.bones.clone(),
            tags: self.secondary_tags?,
        })
    }

    pub fn pose_combined(
        &self,
        right: &[PosedClip<'_>],
        left: &[PosedClip<'_>],
    ) -> Option<FpvHandPose> {
        self.pose_clips(0, right, left, Vec3::ZERO)
    }

    fn pose_clips(
        &self,
        hand: usize,
        anims: &[PosedClip<'_>],
        left: &[PosedClip<'_>],
        offset: Vec3,
    ) -> Option<FpvHandPose> {
        if hand >= 2 || anims.is_empty() {
            if hand == 0 {
                self.note_hands_exit("no clip requested (anims empty)", anims);
            }
            return None;
        }
        if !anims.iter().all(|anim| self.owns_clip(hand, anim))
            || !left.iter().all(|anim| self.owns_clip(1, anim))
        {
            if hand == 0 {
                self.note_hands_exit("clip not owned by this rig (owns_clip false)", anims);
            }
            return None;
        }
        let bound: Vec<(&PosedClip<'_>, Vec<Option<usize>>, usize)> = anims
            .iter()
            .map(|anim| (anim, hand))
            .chain(left.iter().map(|anim| (anim, 1)))
            .filter_map(|(anim, source)| {
                let tracks = self.tracks[source].get(anim.node)?.as_ref()?;
                let bones = tracks
                    .iter()
                    .map(|&bone| (bone != FpvClipTracks::NONE).then_some(usize::from(bone)))
                    .collect();
                Some((anim, bones, source))
            })
            .collect();
        if !bound
            .iter()
            .any(|(_, bones, _)| bones.iter().any(Option::is_some))
        {
            if hand == 0 {
                self.note_hands_exit("no clip track resolved to any DObj bone", anims);
            }
            return None;
        }
        let full_body = |anim: &PosedClip<'_>| {
            anim.node != asset_game::WeaponAnimSlot::AdsUp.index()
                && anim.node != asset_game::WeaponAnimSlot::AdsDown.index()
                && anim
                    .clip
                    .tracks
                    .iter()
                    .any(|track| track.name == "j_shoulder_le")
        };
        let full_weight = if self.combines_hands() {
            bound
                .iter()
                .filter(|(anim, _, source)| *source == 0 && full_body(anim))
                .map(|(anim, _, _)| anim.weight)
                .sum::<f32>()
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let instances: Vec<AnimInstance<'_>> = bound
            .iter()
            .map(|(anim, bones, source)| AnimInstance {
                clip: anim.clip,
                tracks: bones,
                time: anim.time,
                weight: anim.weight * if *source == 1 { 1.0 - full_weight } else { 1.0 },
                parts: self.hand_parts.as_ref().and_then(|masks| {
                    if *source == 0 && full_body(anim) {
                        None
                    } else {
                        Some(&masks[*source])
                    }
                }),
            })
            .collect();
        let assembly = &self.composition.assembly;
        let world = assembly
            .dobj()
            .pose(&instances, &self.parts, Mat4::IDENTITY);
        if hand == 0 {
            self.note_hands_posed(&bound, &world);
        }
        let mut skin = assembly.dobj().skin_matrices(&world);
        for &bone in assembly.collapsed_bones() {
            if let (Some(skin), Some(world)) = (skin.get_mut(bone), world.get(bone)) {
                *skin = Mat4::from_cols(
                    bevy::math::Vec4::ZERO,
                    bevy::math::Vec4::ZERO,
                    bevy::math::Vec4::ZERO,
                    world.w_axis,
                );
            }
        }
        let eye_from_world = tag_view_to_bevy_camera() * world[assembly.view_bone()].inverse();
        // Placement diagnostic: the gun can pass admission and still be nowhere
        // near the camera. `eye_from_world` is built from `view_bone` alone, so
        // if that bone is misplaced (or the gun's bones carry a bad transform)
        // every vertex lands off-screen with nothing in the log to say so.
        // Reported once per pose change, not per frame.
        static PLACEMENT_REPORTED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !PLACEMENT_REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let view = world[assembly.view_bone()];
            let finite = |m: &Mat4| m.to_cols_array().iter().all(|v| v.is_finite());
            let bad = world.iter().filter(|m| !finite(m)).count();
            // Eye-space position of the gun and the hands' weapon mount. This is
            // where the geometry must land to be in front of the camera; anything
            // with +Z is behind it and will be clipped.
            let eye_of = |bone: usize| {
                world
                    .get(bone)
                    .map(|w| (eye_from_world * *w).w_axis.truncate())
            };
            let gun_part = assembly
                .parts()
                .iter()
                .find(|part| matches!(part.role, asset_game::FpvPartRole::Gun));
            let gun_bone = gun_part.map(|p| p.bone_base);
            let hands_part = assembly
                .parts()
                .iter()
                .find(|part| matches!(part.role, asset_game::FpvPartRole::Hands));
            diag::info!(
                Fpv,
                "fpv placement: bones={} view_bone={} view_trans={:?} non_finite={bad} \
                 gun_base={:?} gun_eye={:?} hands_eye={:?}",
                world.len(),
                assembly.view_bone(),
                view.w_axis.truncate(),
                gun_bone,
                gun_bone.and_then(eye_of),
                hands_part.and_then(|p| eye_of(p.bone_base)),
            );
        }
        let lens = assembly
            .camera_bone()
            .map(|camera| tag_camera_lens_local(world[assembly.view_bone()], world[camera]))
            .unwrap_or(Mat4::IDENTITY);
        let published = if hand == 0 {
            world.len()
        } else {
            assembly.paired_bones().min(world.len())
        };
        let shift = (offset != Vec3::ZERO).then(|| Mat4::from_translation(offset));
        let bones = world[..published]
            .iter()
            .map(|bone| {
                let bone = eye_from_world * *bone;
                match shift {
                    Some(shift) => shift * bone,
                    None => bone,
                }
            })
            .collect();
        Some(FpvHandPose {
            generation: self.generation,
            skin,
            eye_from_world,
            offset,
            lens,
            bolt: FpvBoltFrame {
                bones,
                tags: self.tags,
            },
        })
    }

    fn owns_clip(&self, hand: usize, anim: &PosedClip<'_>) -> bool {
        self.clips
            .get(hand)
            .and_then(|clips| clips.get(anim.node))
            .and_then(Option::as_ref)
            .is_some_and(|clip| std::ptr::eq(clip.as_ref(), anim.clip))
    }

    /// Skin every slot into the destination buffer the plan published. A vertex
    /// update only: indices, surface ranges and materials are untouched.
    /// The merged-DObj bone range that belongs to the hands model.
    fn hands_bone_range(&self) -> Option<std::ops::Range<usize>> {
        let part = self
            .composition
            .parts
            .iter()
            .find(|part| part.owner == FpvSurfOwner::Hands)?;
        let slot = self
            .composition
            .assembly
            .dobj()
            .models
            .iter()
            .find(|slot| slot.base == part.bone_base)?;
        Some(slot.base..slot.base + slot.bone_count)
    }

    fn note_hands_exit(&self, exit: &'static str, anims: &[PosedClip<'_>]) {
        let range = self.hands_bone_range();
        let mut diag = FpvHandsClipDiag {
            exit,
            hands_bones: range.as_ref().map_or(0, |range| range.len()),
            ..Default::default()
        };
        for anim in anims {
            let on_hands = self.tracks[0]
                .get(anim.node)
                .and_then(|tracks| tracks.as_ref())
                .map_or(0, |tracks| {
                    tracks
                        .iter()
                        .filter(|&&bone| bone != FpvClipTracks::NONE)
                        .filter(|&&bone| range.as_ref().is_some_and(|r| r.contains(&usize::from(bone))))
                        .count()
                });
            diag.tracks_total += anim.clip.tracks.len();
            diag.tracks_to_hands += on_hands;
            diag.clips.push((anim.node, anim.time, anim.weight, on_hands));
        }
        record_hands_clip(diag);
    }

    fn note_hands_posed(
        &self,
        bound: &[(&PosedClip<'_>, Vec<Option<usize>>, usize)],
        world: &[Mat4],
    ) {
        let Some(range) = self.hands_bone_range() else {
            record_hands_clip(FpvHandsClipDiag {
                exit: "no hands part in the composition",
                ..Default::default()
            });
            return;
        };
        let dobj = self.composition.assembly.dobj();
        let mut diag = FpvHandsClipDiag {
            exit: "posed",
            hands_bones: range.len(),
            ..Default::default()
        };
        for (anim, bones, source) in bound {
            if *source != 0 {
                continue;
            }
            let on_hands = bones.iter().flatten().filter(|b| range.contains(b)).count();
            diag.tracks_total += bones.len();
            diag.tracks_to_hands += on_hands;
            diag.clips.push((anim.node, anim.time, anim.weight, on_hands));
        }
        diag.mask_hands_bones = self
            .hand_parts
            .as_ref()
            .map(|masks| range.clone().filter(|&bone| masks[0].get(bone)).count());
        // Parent-relative, so a moved root does not count as animation.
        for index in range.clone() {
            let (Some(now), Some(bone)) = (world.get(index), dobj.bones.get(index)) else {
                continue;
            };
            let (parent_now, parent_bind) = match bone.parent {
                Some(parent) => match (world.get(parent), dobj.bones.get(parent)) {
                    (Some(w), Some(b)) => (*w, b.bind_world),
                    _ => continue,
                },
                None => (Mat4::IDENTITY, Mat4::IDENTITY),
            };
            let (_, rot_now, pos_now) = (parent_now.inverse() * *now).to_scale_rotation_translation();
            let (_, rot_bind, pos_bind) =
                (parent_bind.inverse() * bone.bind_world).to_scale_rotation_translation();
            let moved = (pos_now - pos_bind).length();
            let turn = rot_now.angle_between(rot_bind).to_degrees();
            if !(moved.is_finite() && turn.is_finite()) {
                diag.moved_bones += 1;
                diag.max_turn_bone = format!("{} (NON-FINITE)", bone.name);
                continue;
            }
            if moved > 0.01 || turn > 0.5 {
                diag.moved_bones += 1;
            }
            diag.max_move = diag.max_move.max(moved);
            if turn > diag.max_turn_deg {
                diag.max_turn_deg = turn;
                diag.max_turn_bone = bone.name.clone();
            }
        }
        record_hands_clip(diag);
    }

    /// Hands clip-versus-draw diagnostic, draw side. Call after `skin_into` with
    /// the rows it wrote (eye space) and this frame's draw list. Returns one
    /// line: the clip facts from the last `pose_clips`, the draw facts for the
    /// hands slot, and a verdict naming the first stage that fails.
    pub fn hands_draw_report(
        &self,
        rows: &[[u8; asset_iw4::size::GFX_PACKED_VERTEX]],
        draws: &[FpvSurfaceDraw],
        visible: bool,
    ) -> String {
        let clip = HANDS_CLIP_DIAG.lock().ok().and_then(|slot| slot.clone());

        // Index span and vertex span of each hand-0 slot, in plan order.
        let mut index_cursor = 0u32;
        let mut hands: Option<(usize, usize, u32, u32, &PreparedFpvModel)> = None;
        let mut gun_bounds: Option<(Vec3, Vec3)> = None;
        for (slot, (model, _rebase)) in self.slots.iter().zip(&self.geometry.segments) {
            let span = model.layout.indices.len() as u32;
            let owner = self.composition.parts[slot.part].owner;
            if slot.hand == 0 && owner == FpvSurfOwner::Hands && hands.is_none() {
                hands = Some((
                    slot.dest_base,
                    model.layout.dest_vertex_n,
                    index_cursor,
                    index_cursor + span,
                    model.as_ref(),
                ));
            }
            if slot.hand == 0 && owner == FpvSurfOwner::Gun && gun_bounds.is_none() {
                let end = (slot.dest_base + model.layout.dest_vertex_n).min(rows.len());
                gun_bounds = eye_bounds(&rows[slot.dest_base.min(end)..end]).map(|b| (b.0, b.1));
            }
            index_cursor += span;
        }

        let (draw_n, tris, bad_indices, verts, nonfinite, in_front, bounds) = match hands {
            Some((dest_base, vertex_n, start, end, model)) => {
                let mut draw_n = 0;
                let mut tris = 0u32;
                for draw in draws {
                    if let Some(&(first, count)) = self.geometry.surface_ranges.get(draw.surface as usize) {
                        if first >= start && first < end {
                            draw_n += 1;
                            tris += count / 3;
                        }
                    }
                }
                let bad = model.layout.indices.iter().filter(|&&i| i as usize >= vertex_n).count();
                let end_row = (dest_base + vertex_n).min(rows.len());
                let slice = &rows[dest_base.min(end_row)..end_row];
                let stats = eye_bounds(slice);
                let (nonfinite, in_front) = stats.map_or((slice.len(), 0), |s| (s.2, s.3));
                (draw_n, tris, bad, slice.len(), nonfinite, in_front, stats.map(|s| (s.0, s.1)))
            }
            None => (0, 0, 0, 0, 0, 0, None),
        };

        let verdict = match (&clip, hands.is_some()) {
            (_, false) => "PART NOT DRAWN: no hands slot in the plan".to_string(),
            (None, _) => "CLIP NOT APPLIED: pose_clips never ran for hand 0".to_string(),
            (Some(c), _) if c.exit != "posed" => format!("CLIP NOT APPLIED: {}", c.exit),
            (Some(c), _) if c.tracks_to_hands == 0 => {
                "CLIP NOT APPLIED: clip binds, but no track lands on a hands bone".to_string()
            }
            (Some(c), _) if c.moved_bones == 0 => {
                "CLIP BOUND BUT HANDS AT BIND POSE: check time, weight and mask".to_string()
            }
            _ if draw_n == 0 => "PART NOT DRAWN: no hands surface in this frame's draw list".to_string(),
            _ if bad_indices > 0 => "PART DRAWN WITH BAD INDICES: hands indices exceed the slot".to_string(),
            _ if nonfinite > 0 => "PART DRAWN WITH NON-FINITE VERTICES".to_string(),
            // Bevy cameras look down -Z, so in front of the eye is z < 0.
            _ if in_front == 0 => "PART DRAWN BEHIND THE CAMERA: every hands vertex has z >= 0".to_string(),
            _ if !visible => "PLAN NOT VISIBLE: hands fine, whole viewmodel gated off".to_string(),
            _ => "CLIP APPLIED AND PART SUBMITTED: hands are in front of the camera; \
                  check material, alpha, cull mode, depth, or frustum against the gun bounds"
                .to_string(),
        };

        let clip_text = match &clip {
            Some(c) => format!(
                "exit=\"{}\" clips={:?} tracks_to_hands={}/{} hands_bones={} mask_hands_bones={:?} \
                 moved_bones={} max_move={:.3} max_turn={:.1}deg@{}",
                c.exit,
                c.clips
                    .iter()
                    .map(|(node, time, weight, n)| format!("node{node}:t{time:.2}:w{weight:.2}:h{n}"))
                    .collect::<Vec<_>>(),
                c.tracks_to_hands,
                c.tracks_total,
                c.hands_bones,
                c.mask_hands_bones,
                c.moved_bones,
                c.max_move,
                c.max_turn_deg,
                c.max_turn_bone,
            ),
            None => "exit=<none>".to_string(),
        };
        let fmt_bounds = |b: Option<(Vec3, Vec3)>| {
            b.map_or("<none>".to_string(), |(lo, hi)| {
                format!("[{:.1},{:.1},{:.1}]..[{:.1},{:.1},{:.1}]", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z)
            })
        };
        format!(
            "fpv hands verdict: {verdict} | clip: {clip_text} | draw: hands_draws={draw_n} tris={tris} \
             plan_hands_n={} verts={verts} bad_indices={bad_indices} nonfinite={nonfinite} \
             in_front={in_front}/{verts} hands_eye={} gun_eye={} visible={visible}",
            self.geometry.hands_plan_n,
            fmt_bounds(bounds),
            fmt_bounds(gun_bounds),
        )
    }

    pub fn skin_into(
        &self,
        poses: &[Option<FpvHandPose>; 2],
        dest: &mut [[u8; asset_iw4::size::GFX_PACKED_VERTEX]],
    ) -> bool {
        if poses
            .iter()
            .flatten()
            .any(|pose| pose.generation != self.generation)
        {
            return false;
        }
        for slot in &self.slots {
            let Some(pose) = poses[slot.hand].as_ref() else {
                continue;
            };
            let part = &self.composition.parts[slot.part];
            let model = &part.model;
            let Some(skel) = self
                .meshes
                .get_at(model.catalog_entry)
                .map(|entry| &entry.skel)
            else {
                continue;
            };
            let end = slot.dest_base.saturating_add(model.layout.dest_vertex_n);
            if end > dest.len() {
                continue;
            }
            let rows = &mut dest[slot.dest_base..end];
            skin_packed_into(
                skel,
                |bone| pose.eye_from_world * pose.skin[part.bone_base + bone],
                |_| false,
                0,
                &model.layout,
                rows,
            );
            if pose.offset != Vec3::ZERO {
                translate_packed_rows(rows, pose.offset);
            }
        }
        true
    }
}

fn translate_packed_rows(rows: &mut [[u8; asset_iw4::size::GFX_PACKED_VERTEX]], delta: Vec3) {
    for row in rows {
        let x = f32::from_le_bytes([row[0], row[1], row[2], row[3]]) + delta.x;
        let y = f32::from_le_bytes([row[4], row[5], row[6], row[7]]) + delta.y;
        let z = f32::from_le_bytes([row[8], row[9], row[10], row[11]]) + delta.z;
        row[0..4].copy_from_slice(&x.to_le_bytes());
        row[4..8].copy_from_slice(&y.to_le_bytes());
        row[8..12].copy_from_slice(&z.to_le_bytes());
    }
}

/// Eye-space bounds of packed rows: (min, max, non-finite count, z<0 count).
/// Position is the first three f32s of each row, as `skin_into` writes them.
fn eye_bounds(rows: &[[u8; asset_iw4::size::GFX_PACKED_VERTEX]]) -> Option<(Vec3, Vec3, usize, usize)> {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let (mut nonfinite, mut in_front, mut seen) = (0, 0, 0);
    for row in rows {
        let f = |at: usize| f32::from_le_bytes([row[at], row[at + 1], row[at + 2], row[at + 3]]);
        let p = Vec3::new(f(0), f(4), f(8));
        if !p.is_finite() {
            nonfinite += 1;
            continue;
        }
        seen += 1;
        lo = lo.min(p);
        hi = hi.max(p);
        if p.z < 0.0 {
            in_front += 1;
        }
    }
    (seen > 0 || nonfinite > 0).then_some((lo, hi, nonfinite, in_front))
}
