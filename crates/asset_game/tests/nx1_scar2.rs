//! Contract tests for the NX1 SCAR Mod 2 wiring.
//!
//! These cover the parts that are reachable without a captured zone: the NX1
//! class-category mapping, and the parse of the equipped primary that
//! [`WeaponRegistry::nx1_scar2_row_of`] depends on.
//!
//! The registry lookup itself is not covered here. `WeaponRegistry` has no
//! public way to insert a synthetic donor row, and adding one purely for a test
//! would widen the crate's API for no runtime gain. That path is therefore only
//! exercised by a real match load.

use asset_game::{cac_category_from_item_group, CacAuthoredCategory, FamilyKey};

/// The class picker files a family under a category by mapping its item group.
/// The NX1 group has to resolve, or the gun lands in no folder at all.
#[test]
fn nx1_item_group_maps_to_the_nx1_category() {
    assert_eq!(
        cac_category_from_item_group("weapon_nx1"),
        Some(CacAuthoredCategory::Nx1)
    );
}

/// The NX1 category is its own folder, not folded into an existing class.
#[test]
fn nx1_category_is_distinct_from_the_weapon_classes() {
    assert_ne!(
        CacAuthoredCategory::Nx1,
        CacAuthoredCategory::Assault,
        "NX1 must not read as Assault Rifles in the class picker"
    );
    assert_eq!(CacAuthoredCategory::Nx1.menu_label(), "NX1");
    assert_eq!(CacAuthoredCategory::Nx1.slug(), "nx1");
}

/// NX1 has no string table in this tree, so the category has no localization
/// key and the menu label is the only text source. A key here would make the
/// folder render blank instead of falling back to `menu_label`.
#[test]
fn nx1_category_has_no_localization_key() {
    assert_eq!(CacAuthoredCategory::Nx1.loc_key(), None);
}

/// `nx1_scar2_row_of` redirects only when the equipped primary parses to base
/// `scar2`. These are the forms `equipped_primary` actually takes: the class
/// slot stores `asset_key()` output, and the registry stores unsuffixed bases.
#[test]
fn equipped_primary_forms_parse_to_the_scar2_base() {
    for raw in ["iw4:weapon/scar2_mp", "iw4:scar2", "iw4:scar2_mp"] {
        let key = FamilyKey::parse(raw)
            .unwrap_or_else(|| panic!("`{raw}` should parse as a weapon family key"));
        assert_eq!(key.base, asset_game::NX1_SCAR2_BASE, "base of `{raw}`");
        assert_eq!(key.namespace.as_str(), "iw4", "ns of `{raw}`");
    }
}

/// The redirect must not fire for another weapon. A primary that merely
/// contains the substring, or that is the M4 stand-in the map is actually told,
/// has to stay off the NX1 row.
#[test]
fn other_primaries_do_not_parse_to_the_scar2_base() {
    for raw in [
        "iw4:weapon/m4_mp",
        "iw4:m4",
        "iw4:weapon/scar_mp",
        "iw4:weapon/usp_mp",
        // `scar2_reflex` is an attachment variant, not the base weapon.
        "iw4:weapon/scar2_reflex_mp",
    ] {
        let base = FamilyKey::parse(raw).map(|key| key.base);
        assert_ne!(
            base.as_deref(),
            Some(asset_game::NX1_SCAR2_BASE),
            "`{raw}` must not redirect onto the NX1 row"
        );
    }
}

/// A malformed or empty primary must not resolve, so the redirect falls through
/// to the weapon the map reported rather than guessing.
#[test]
fn unusable_primaries_do_not_parse() {
    for raw in ["", "not a key", "iw4:", "iw4:weapon/"] {
        assert!(
            FamilyKey::parse(raw).is_none(),
            "`{raw}` should not parse as a family key"
        );
    }
}
