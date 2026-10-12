//! NX1 viewmodel textures: decode the cast's PNG colour/normal/specular maps and
//! bind them to the NX1 materials.
//!
//! An NX1 material row written in the match walk cannot admit on its own: a
//! material's colour map is normally decoded out of the zone image trees, which
//! hold none of the NX1 bytes. So this does what the trees would do — decodes each
//! map to a real GPU image and installs it directly into the row's image, then
//! points the material's colour/normal/specular bindings at those images. The
//! material keeps a real compiled techset (cloned from a rendering retail
//! material), so it admits; the pixels are NX1's own, so it looks like a SCAR-H.
//!
//! The cast names textures per material as `_images/<material>/<file>.png`; the
//! bytes sit beside the meshes in the export's `images` directory.

use asset_core::AssetNamespace;
use asset_material::{MaterialDefinitions, MaterialTextureBinding};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
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

/// Decode one NX1 PNG into a GPU image the renderer can bind.
///
/// NX1 ships uncompressed RGBA PNGs. They are decoded to straight RGBA8 and
/// uploaded with the same sampler and usage the retail decoded images use, so the
/// material path treats them like any other colour map.
fn decode_png_texture(bytes: &[u8], srgb: bool) -> Option<Image> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).ok()?;
    let width = info.width;
    let height = info.height;
    if width == 0 || height == 0 {
        return None;
    }
    let color = info.color_type;
    let channels = match color {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None, // palette; rare in this dump
    };
    // Normalize whatever PNG variant to RGBA8, which is what the renderer binds.
    let src = &buffer[..info.buffer_size()];
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for px in src.chunks_exact(channels) {
        match channels {
            1 => rgba.extend_from_slice(&[px[0], px[0], px[0], 255]),
            2 => rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            3 => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
            _ => rgba.extend_from_slice(&[px[0], px[1], px[2], px[3]]),
        }
    }
    // Single-level texture; a per-pixel decode does not build a mip chain.
    let format = if srgb {
        TextureFormat::Rgba8UnormSrgb
    } else {
        TextureFormat::Rgba8Unorm
    };
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = 1;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image.data = Some(rgba);
    Some(image)
}

/// Find an existing image by name in the map namespace, or install a new one
/// carrying the decoded PNG. Returns the image row index.
fn install_nx1_image(
    global: &mut MaterialDefinitions,
    namespace: AssetNamespace,
    file: &str,
    images_root: &std::path::Path,
    srgb: bool,
) -> Option<usize> {
    let stem = file.trim_end_matches(".png");
    if let Some(at) = global.image_index_by_key(namespace, stem) {
        return Some(at);
    }
    let path = images_root.join(file);
    let bytes = std::fs::read(&path).ok()?;
    let gpu = decode_png_texture(&bytes, srgb)?;
    let image = asset_material::AuthoredImage {
        namespace,
        name: asset_core::AssetRef::decode(stem),
        map_type: 0,
        semantic: if srgb {
            asset_material::TS_COLOR_MAP
        } else {
            asset_material::TS_NORMAL_MAP
        },
        category: 0,
        use_srgb_reads: srgb,
        width: gpu.width() as u16,
        height: gpu.height() as u16,
        depth: 1,
        level_count: 1,
        format: 0,
        payload: std::sync::Arc::new(Vec::new()),
        decoded: Some(Arc::new(gpu)),
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    };
    global.images.push(image);
    Some(global.images.len() - 1)
}

/// Build one NX1 material row: a clone of `donor` (so the techset is real and it
/// admits) with its colour/normal/specular bindings repointed at NX1's own PNGs.
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
) -> Option<usize> {
    let mut material = global.materials.get(donor)?.clone();

    let mut bindings: Vec<MaterialTextureBinding> = Vec::new();
    // colour
    if let Some(file) = textures.albedo.as_deref() {
        if let Some(image) = install_nx1_image(global, namespace, file, images_root, true) {
            bindings.push(MaterialTextureBinding {
                name_hash: 0,
                name_start: 0,
                name_end: 0,
                sampler_state: 0,
                semantic: asset_material::TS_COLOR_MAP,
                image: Some(image),
            });
        }
    }
    // normal
    if let Some(file) = textures.normal.as_deref() {
        if let Some(image) = install_nx1_image(global, namespace, file, images_root, false) {
            bindings.push(MaterialTextureBinding {
                name_hash: 0,
                name_start: 0,
                name_end: 0,
                sampler_state: 0,
                semantic: asset_material::TS_NORMAL_MAP,
                image: Some(image),
            });
        }
    }
    // specular
    if let Some(file) = textures.specular.as_deref() {
        if let Some(image) = install_nx1_image(global, namespace, file, images_root, false) {
            bindings.push(MaterialTextureBinding {
                name_hash: 0,
                name_start: 0,
                name_end: 0,
                sampler_state: 0,
                semantic: asset_material::TS_SPECULAR_MAP,
                image: Some(image),
            });
        }
    }

    // No decodable maps: keep the donor so the surface still draws (borrowed
    // look) rather than dropping it.
    if bindings.is_empty() {
        return None;
    }

    material.name = asset_core::AssetRef::decode(material_name);
    material.namespace = namespace;
    material.textures = bindings;
    global.materials.push(material);
    Some(global.materials.len() - 1)
}