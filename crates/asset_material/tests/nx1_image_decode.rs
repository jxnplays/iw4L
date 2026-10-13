//! Regression guard for the NX1 viewmodel being invisible.
//!
//! Every NX1 image slot used to be filtered out before it reached the decoder,
//! because the decode loop dispatched only the four zone namespaces. The images
//! were then never uploaded, the FPV draw answered `colour map not uploaded`, and
//! the rig dropped every surface — so the whole NX1 viewmodel (gun *and* hands)
//! vanished. Nothing logged an error, which is what made it expensive to find.
//!
//! These tests pin the contract that was broken. They read no game files.

use asset_core::AssetNamespace;
use asset_material::material_images::IMAGE_DECODE_NAMESPACES;

/// NX1 must be a decode namespace. Without it its maps never upload.
#[test]
fn nx1_images_are_dispatched_to_the_decoder() {
    assert!(
        IMAGE_DECODE_NAMESPACES.contains(&AssetNamespace::Nx1),
        "Nx1 missing from IMAGE_DECODE_NAMESPACES: its images will never decode, \
         the FPV draw will refuse every NX1 surface, and the viewmodel will vanish"
    );
}

/// Every zone namespace keeps decoding.
#[test]
fn zone_namespaces_still_decode() {
    for ns in [
        AssetNamespace::Iw4,
        AssetNamespace::T5,
        AssetNamespace::Iw5,
        AssetNamespace::T6,
    ] {
        assert!(
            IMAGE_DECODE_NAMESPACES.contains(&ns),
            "{} lost its image decode namespace",
            ns.as_str()
        );
    }
}

/// The list covers every namespace the catalog can name, so a future family
/// cannot be added without also being routed to the decoder.
#[test]
fn no_namespace_is_left_unrouted() {
    for ns in [
        AssetNamespace::Iw4,
        AssetNamespace::T5,
        AssetNamespace::Iw5,
        AssetNamespace::T6,
        AssetNamespace::Nx1,
    ] {
        assert!(
            IMAGE_DECODE_NAMESPACES.contains(&ns),
            "{} has no image decode route",
            ns.as_str()
        );
    }
    assert_eq!(
        IMAGE_DECODE_NAMESPACES.len(),
        5,
        "a namespace was added to AssetNamespace without a decode route"
    );
}

/// NX1 is not a zone game, so it has no fastfile tree to read images from. That
/// is correct and is why NX1 maps must travel as in-memory payloads: the decoder
/// builds an empty index for this namespace and `prepare_unshared` reads the
/// payload instead. If this ever changes, NX1 images would silently come from
/// nothing.
#[test]
fn nx1_has_no_tree_so_its_maps_must_be_payloads() {
    let trees = asset_transport::NamespaceTrees::default();
    assert!(
        trees.main_for(AssetNamespace::Nx1).is_none(),
        "NX1 is not a fastfile namespace; if it gained a tree, the payload path \
         and the empty-index assumption in decode_requests_from_trees need review"
    );
}