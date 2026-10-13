//! NX1 viewmodel textures: decode the cast's PNG colour/normal/specular maps and
//! bind them to the NX1 materials.
//!
//! An NX1 material row written in the match walk cannot admit on its own: a
//! material's colour map is normally decoded out of the zone image trees, which
//! hold none of the NX1 bytes. So each NX1 material is a clone of a real,
//! admitting retail material (so its techset is compiled and the row admits),
//! with its colour/normal/specular bindings repointed at NX1's own maps.
//!
//! The maps themselves ride the *normal* image pipeline. Each decoded PNG is
//! written into its image row as a BGRA payload with `format = 21`
//! (`PixelFormat::Bgra8`) and `decoded = None`. That is exactly the shape the
//! retail loader expects: the demand planner selects any image that is not
//! already decoded, `prepare_unshared` sees a non-empty payload and calls
//! `decode_gfx_image` on it, and the image is planned, decoded and uploaded to
//! the GPU like any other colour map. Hand-installing a GPU image here instead
//! would mark the row "already decoded", so the planner would skip it and it
//! would never be uploaded — the material would bind no texture and the gun
//! would vanish.
//!
//! The cast names textures per material as `_images/<material>/<file>.png`; the
//! bytes sit beside the meshes in the export's `images` directory.

use asset_core::AssetNamespace;
use asset_material::{MaterialDefinitions, MaterialTextureBinding};
use std::sync::Arc;

/// The four maps a cast material can name, in the order the `matl` node declares
/// them (the texture ids live in those same slots, one id per map).
#[derive(Clone, Default, Debug)]
pub struct Nx1MaterialTextures {
    pub specular: Option<String>,
    pub normal: Option<String>,
    pub albedo: Option<String>,
    pub detail: Option<String>,
}

/// `PixelFormat::Bgra8` — the uncompressed format `decode_gfx_image` accepts for
/// a raw payload, so the loader turns the PNG straight into BGRA8.
const FORMAT_BGRA8: u32 = 21;

/// Decode one NX1 PNG to BGRA8 bytes, or `None` if it is not a plain image.
fn png_to_bgra8(bytes: &[u8]) -> Option<(u16, u16, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None, // palette; rare in this dump
    };
    let src = &buffer[..info.buffer_size()];
    // Normalize to BGRA8 (blue, green, red, alpha) — the byte order
    // `decode_gfx_image` reads for `Bgra8`.
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for px in src.chunks_exact(channels) {
        let (r, g, b, a) = match channels {
            1 => (px[0], px[0], px[0], 255),
            2 => (px[0], px[0], px[0], px[1]),
            3 => (px[0], px[1], px[2], 255),
            _ => (px[0], px[1], px[2], px[3]),
        };
        bgra.extend_from_slice(&[b, g, r, a]);
    }
    Some((width as u16, height as u16, bgra))
}

/// Find an existing image by name in the map namespace, or install a new one
/// carrying the decoded PNG as a BGRA payload the loader will decode. Returns the
/// image row index.
fn install_nx1_image(
    global: &mut MaterialDefinitions,
    namespace: AssetNamespace,
    file: &str,
    images_root: &std::path::Path,
    semantic: u8,
    srgb: bool,
) -> Option<usize> {
    let stem = file.trim_end_matches(".png");
    if let Some(at) = global.image_index_by_key(namespace, stem) {
        return Some(at);
    }
    let path = images_root.join(file);
    let bytes = std::fs::read(&path).ok()?;
    let (width, height, bgra) = png_to_bgra8(&bytes)?;
    global.images.push(asset_material::AuthoredImage {
        namespace,
        name: asset_core::AssetRef::decode(stem),
        map_type: 0,
        semantic,
        category: 0,
        use_srgb_reads: srgb,
        width,
        height,
        depth: 1,
        level_count: 1,
        format: FORMAT_BGRA8,
        // Decoded by the normal loader from this payload, so `decoded` stays
        // None and the demand planner picks the image up and uploads it.
        payload: Arc::new(bgra),
        decoded: None,
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    });
    Some(global.images.len() - 1)
}

/// Build one NX1 material row: a clone of `donor` (so the techset is real and it
/// admits) with its colour/normal/specular bindings repointed at NX1's own maps.
///
/// Returns the new material's authored row, or `None` if none of its maps could be
/// decoded (in which case the caller keeps the donor so the gun still draws).
pub fn register_nx1_textured_material(
    global: &mut MaterialDefinitions,
    namespace: AssetNamespace,
    material_name: &str,
    textures: &Nx1MaterialTextures,
    donor: usize,
    images_root: &std::path::Path,
    report: &mut Vec<String>,
) -> Option<usize> {
    let mut material = global.materials.get(donor)?.clone();
    let donor_bindings = material.textures.len();

    // Repoint the donor's own bindings rather than replacing the list.
    //
    // The techset `mc_l_sm_r0c0n0s0p0` names more slots than colour/normal/specular
    // (detail `d0`, and whatever else), and the sampler state and texture-name
    // metadata on a binding are not free to invent. Overwriting `material.textures`
    // wholesale dropped every binding the donor had that NX1 does not supply, and
    // stamped `sampler_state: 0` with zeroed name hashes on the ones it did. The
    // surface then admitted, drew, and sampled nothing — an invisible viewmodel.
    //
    // So: keep the donor's bindings and their metadata, and swap only the image
    // each slot points at. A slot the donor does not have is added as a clone of
    // the donor's colour binding so it inherits a real sampler state.
    let mut bindings: Vec<MaterialTextureBinding> = Vec::new();
    let mut swapped = 0usize;
    {
        let mut add = |file: &str, semantic: u8, srgb: bool| {
            let Some(image) = install_nx1_image(global, namespace, file, images_root, semantic, srgb)
            else {
                return;
            };
            match material
                .textures
                .iter_mut()
                .find(|binding| binding.semantic == semantic)
            {
                Some(existing) => {
                    existing.image = Some(image);
                    swapped += 1;
                }
                None => {
                    // No slot to repoint: clone the donor's colour binding so the
                    // new one carries the same sampler state and name metadata.
                    let template = material
                        .textures
                        .iter()
                        .find(|binding| binding.semantic == asset_material::TS_COLOR_MAP)
                        .cloned()
                        .unwrap_or(MaterialTextureBinding {
                            name_hash: 0,
                            name_start: 0,
                            name_end: 0,
                            sampler_state: 0,
                            semantic,
                            image: None,
                        });
                    bindings.push(MaterialTextureBinding {
                        semantic,
                        image: Some(image),
                        ..template
                    });
                }
            }
            let _ = srgb;
        };
        if let Some(file) = textures.albedo.as_deref() {
            add(file, asset_material::TS_COLOR_MAP, true);
        }
        if let Some(file) = textures.normal.as_deref() {
            add(file, asset_material::TS_NORMAL_MAP, false);
        }
        if let Some(file) = textures.specular.as_deref() {
            add(file, asset_material::TS_SPECULAR_MAP, false);
        }
    }

    // No decodable maps: keep the donor so the surface still draws (borrowed
    // look) rather than dropping it.
    if swapped == 0 && bindings.is_empty() {
        report.push(format!(
            "nx1 tex {material_name}: NO MAPS BOUND (albedo={:?} normal={:?} spec={:?}) -> donor {donor}",
            textures.albedo, textures.normal, textures.specular
        ));
        return None;
    }
    // Append any slot the donor had no binding for; everything else is untouched.
    material.textures.extend(bindings);

    // What actually landed, so a surface that draws untextured is named rather
    // than spotted by eye.
    //
    // A surface with no usable colour map falls through to a flat shader colour,
    // which is what a solid untextured part on the gun is. That can happen when the
    // cast names no albedo, when the PNG will not read, or when the slot was
    // repointed at a pre-existing catalog image rather than the NX1 bytes.
    let slot = |semantic: u8| {
        material
            .textures
            .iter()
            .find(|b| b.semantic == semantic)
            .and_then(|b| b.image)
    };
    let colour = slot(asset_material::TS_COLOR_MAP);
    let payload = colour.and_then(|i| global.images.get(i as usize));
    report.push(format!(
        "nx1 tex {material_name}: donor_bindings={donor_bindings} swapped={swapped} \
         colour={colour:?} normal={:?} spec={:?} image={:?}",
        slot(asset_material::TS_NORMAL_MAP),
        slot(asset_material::TS_SPECULAR_MAP),
        payload.map(|i| (i.name.as_str().to_owned(), i.width, i.height, i.format, i.payload.len(), i.decoded.is_some()))
    ));

    material.name = asset_core::AssetRef::decode(material_name);
    material.namespace = namespace;
    global.materials.push(material);
    Some(global.materials.len() - 1)
}