//! Contract tests for the NX1 SCAR Mod 2 wiring.
//!
//! These cover the parts that are reachable without a captured zone: the NX1
//! namespace's identity, and the parse of the equipped primary that
//! [`WeaponRegistry::nx1_scar2_row_of`] depends on.
//!
//! The registry lookup itself is not covered here. `WeaponRegistry` has no
//! public way to insert a synthetic donor row, and adding one purely for a test
//! would widen the crate's API for no runtime gain. That path is therefore only
//! exercised by a real match load.

use asset_core::AssetNamespace;
use asset_game::{cac_category_from_item_group, CacAuthoredCategory, FamilyKey, NX1_SCAR2_BASE};

/// NX1 is a namespace, not a `CacAuthoredCategory`. The class picker builds its
/// folders as (namespace, category) pairs and groups them by namespace first, so
/// a namespace is what puts NX1 beside IW4/IW5/T5/T6 at the top level. Filing NX1
/// under a category instead is what made it a subcategory of PRIMARY.
#[test]
fn nx1_is_a_namespace_not_a_weapon_class() {
    // A weapon class is chosen per weapon inside a game; NX1 has no such variant
    // and must not gain one.
    let classes = format!("{:?}", CacAuthoredCategory::Assault);
    assert!(
        !classes.contains("Nx1"),
        "CacAuthoredCategory must not carry an NX1 variant"
    );
    // Its label is not reachable as a class label either.
    assert_ne!(CacAuthoredCategory::Assault.slug(), "nx1");
}

/// The namespace's own identity: token, round-trip, and enumeration.
#[test]
fn nx1_namespace_identity() {
    assert_eq!(AssetNamespace::Nx1.as_str(), "nx1");
    assert_eq!(AssetNamespace::parse("nx1"), Some(AssetNamespace::Nx1));
    assert_eq!(
        AssetNamespace::Nx1,
        AssetNamespace::parse(AssetNamespace::Nx1.as_str()).unwrap()
    );
    // It must be enumerated like the retail games, or the picker cannot offer it.
    assert!(
        AssetNamespace::ALL.contains(&AssetNamespace::Nx1),
        "NX1 must appear in AssetNamespace::ALL or it is never iterated"
    );
    // Distinct from every retail game, including by ordering, since the picker
    // sorts folders by namespace.
    for other in [
        AssetNamespace::Iw4,
        AssetNamespace::Iw5,
        AssetNamespace::T5,
        AssetNamespace::T6,
    ] {
        assert_ne!(AssetNamespace::Nx1, other);
        assert_ne!(AssetNamespace::Nx1.as_str(), other.as_str());
    }
}

/// A weapon key under NX1 parses to the NX1 namespace and the SCAR2 base, which
/// is what makes the row land in its own top-level game folder.
#[test]
fn scar2_key_parses_into_the_nx1_namespace() {
    let key = FamilyKey::parse("nx1:weapon/scar2_mp").expect("nx1 weapon key should parse");
    assert_eq!(key.namespace, AssetNamespace::Nx1);
    assert_eq!(key.base, NX1_SCAR2_BASE);
    assert_eq!(key.asset_key(), "nx1:weapon/scar2_mp");
    // And it is not confusable with the donor it was cloned from.
    assert_ne!(key.namespace, AssetNamespace::Iw4);
}

/// NX1 has no string table, so the class picker cannot resolve a localized token
/// for it. The SCAR2 row therefore carries a literal display name, and NX1 needs
/// no `CacAuthoredCategory::loc_key`. This test records that dependency: the row
/// must not be given a category that expects a localized string.
#[test]
fn nx1_has_no_localized_category_string() {
    // The category SCAR2 files under inside NX1 is a normal retail class, so it
    // does have a localization key. If NX1 ever gains its own string table this
    // expectation is what should change.
    assert!(CacAuthoredCategory::Assault.loc_key().is_some());
    // And the item group NX1 uses must resolve to that class, or the offer is
    // dropped from the picker entirely.
    assert_eq!(
        cac_category_from_item_group("weapon_assault"),
        Some(CacAuthoredCategory::Assault)
    );
}

/// The redirect fires only when the equipped primary names base `scar2`. These
/// are the forms `equipped_primary` actually takes: the class slot stores
/// `asset_key()` output, which is namespace-qualified.
#[test]
fn equipped_primary_forms_parse_to_the_scar2_base() {
    for raw in ["nx1:weapon/scar2_mp", "nx1:scar2", "nx1:scar2_mp"] {
        let key = FamilyKey::parse(raw)
            .unwrap_or_else(|| panic!("`{raw}` should parse as a weapon family key"));
        assert_eq!(key.base, NX1_SCAR2_BASE, "base of `{raw}`");
        assert_eq!(key.namespace, AssetNamespace::Nx1, "ns of `{raw}`");
    }
}

/// The redirect must not fire for another weapon. The M4 stand-in the map is
/// actually told is the important case: it is a different namespace, so it can
/// never resolve to the NX1 row even though the base name is unrelated.
#[test]
fn other_primaries_do_not_parse_to_the_scar2_base() {
    for raw in [
        "iw4:weapon/m4_mp",
        "iw4:m4",
        "iw4:weapon/scar_mp",
        "iw4:weapon/usp_mp",
        // `scar2_reflex` is an attachment variant, not the base weapon.
        "nx1:weapon/scar2_reflex_mp",
        // The donor's own namespace must not reach the NX1 row.
        "iw4:weapon/scar2_mp",
    ] {
        let key = FamilyKey::parse(raw);
        assert_ne!(
            key.as_ref().map(|key| (key.namespace, key.base.as_str())),
            Some((AssetNamespace::Nx1, NX1_SCAR2_BASE)),
            "`{raw}` must not redirect onto the NX1 row"
        );
    }
}

/// A malformed or empty primary must not resolve, so the redirect falls through
/// to the weapon the map reported rather than guessing.
#[test]
fn unusable_primaries_do_not_parse() {
    for raw in ["", "not a key", "nx1:", "nx1:weapon/"] {
        assert!(
            FamilyKey::parse(raw).is_none(),
            "`{raw}` should not parse as a family key"
        );
    }
}
