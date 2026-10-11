use super::*;

#[derive(Clone, Debug)]
enum AppearanceRepresentation {
    Models {
        view: AssetEdge<FpvMeshSpace>,
        world: AssetEdge<WorldWeaponSpace>,
    },
    Materials(usize),
}

#[derive(Clone, Debug)]
pub(super) struct PreparedWeaponAppearance {
    slot: u8,
    name: String,
    caption: Option<String>,
    preview: Option<String>,
    representation: AppearanceRepresentation,
}

impl PreparedWeaponAppearance {
    pub(super) fn prepare(row: &WeaponRow) -> Arc<[Self]> {
        let mut appearances = vec![Self {
            slot: 0,
            name: "none".to_owned(),
            caption: None,
            preview: None,
            representation: AppearanceRepresentation::Models {
                view: row.gun_xmodel_edge,
                world: row.world_model_edge,
            },
        }];
        let optional = |value: &str| (!value.is_empty()).then(|| value.to_owned());
        // Per-model camouflage meshes. Iw4 and Iw5 carry `camo_models` natively.
        //
        // Nx1 is here for a specific reason, not as a shortcut. An Nx1 row is a
        // clone of an Iw4 donor and carries the donor's `camo_models` with it, and
        // those choices are real: the row is published through the same
        // `mp/camotable.csv` backfill as any Iw4 row (see `publication.rs`), so
        // `camo_models.choices` is populated by the time this runs.
        //
        // This arm used to be empty. That discarded the inherited choices and left
        // the row with only the base appearance, and since `camouflage_choices`
        // reads `appearances`, the create-a-class camo pane saw an empty list and
        // `ui_class_camo` refused with "This weapon has no camouflage".
        //
        // Sharing the arm is correct: the representation is per-model meshes,
        // which is exactly what the donor's `camo_models.view` / `.world`
        // describe, and those edges are rebound by `resolve_fpv_mesh_edges` and
        // `resolve_world_model_edges` using the preparation recipe the row already
        // carries. Where a camo mesh does not resolve, `map_or` falls back to the
        // base gun edge, so a missing camo mesh degrades to the plain gun.
        match row.namespace {
            crate::AssetNamespace::Iw4
            | crate::AssetNamespace::Iw5
            | crate::AssetNamespace::Nx1 => {
                appearances.extend(
                    row.camo_models
                        .choices
                        .iter()
                        .filter(|choice| choice.slot != 0)
                        .map(|choice| {
                            let view = row
                                .camo_view_edges
                                .iter()
                                .find(|(slot, edge)| *slot == choice.slot && edge.is_bound())
                                .map_or(row.gun_xmodel_edge, |(_, edge)| *edge);
                            let world = row
                                .camo_world_edges
                                .iter()
                                .find(|(slot, edge)| *slot == choice.slot && edge.is_bound())
                                .map_or(row.world_model_edge, |(_, edge)| *edge);
                            Self {
                                slot: choice.slot,
                                name: choice.name.clone(),
                                caption: optional(&choice.caption_key),
                                preview: optional(&choice.preview),
                                representation: AppearanceRepresentation::Models { view, world },
                            }
                        }),
                );
            }
            crate::AssetNamespace::T5 | crate::AssetNamespace::T6 => {
                appearances.extend(
                    row.material_camos
                        .iter()
                        .enumerate()
                        .filter(|(_, camo)| camo.slot != 0)
                        .map(|(index, camo)| Self {
                            slot: camo.slot,
                            name: camo.name.clone(),
                            caption: optional(&camo.caption_key),
                            preview: optional(&camo.preview),
                            representation: AppearanceRepresentation::Materials(index),
                        }),
                );
            }
        }
        appearances.into()
    }
}

pub struct SelectedWeaponAppearance<'a> {
    registry: &'a WeaponRegistry,
    weapon: u32,
    appearance: &'a PreparedWeaponAppearance,
}

impl<'a> SelectedWeaponAppearance<'a> {
    pub fn slot(&self) -> u8 {
        self.appearance.slot
    }
    pub fn name(&self) -> &'a str {
        &self.appearance.name
    }
    pub fn caption(&self) -> Option<&'a str> {
        self.appearance.caption.as_deref()
    }
    pub fn preview(&self) -> Option<&'a str> {
        self.appearance.preview.as_deref()
    }
    pub fn material_camouflage(&self) -> Option<&'a crate::WeaponCamouflage> {
        match self.appearance.representation {
            AppearanceRepresentation::Models { .. } => None,
            AppearanceRepresentation::Materials(index) => self.registry.rows[self.weapon as usize]
                .material_camos
                .get(index),
        }
    }
    pub fn view_model<'b>(
        &self,
        catalog: &'b crate::FpvMeshCatalog,
    ) -> Option<(usize, &'b asset_model::FpvMeshEntry)> {
        if self.registry.fpv_catalog_identity != catalog.identity() {
            return None;
        }
        let view = match self.appearance.representation {
            AppearanceRepresentation::Models { view, .. } => view,
            AppearanceRepresentation::Materials(_) => {
                self.registry.rows[self.weapon as usize].gun_xmodel_edge
            }
        };
        let order = view.bound_index()?;
        Some((order, catalog.get_at(order)?))
    }
    pub fn world_model<'b>(
        &self,
        catalog: &'b crate::WorldWeaponCatalog,
    ) -> Option<&'b crate::WorldWeaponEntry> {
        self.world_order(catalog)
            .and_then(|order| catalog.get_at(order))
    }
    pub fn world_order(&self, catalog: &crate::WorldWeaponCatalog) -> Option<usize> {
        if self.registry.world_catalog_identity != catalog.identity() {
            return None;
        }
        let world = match self.appearance.representation {
            AppearanceRepresentation::Models { world, .. } => world,
            AppearanceRepresentation::Materials(_) => {
                self.registry.rows[self.weapon as usize].world_model_edge
            }
        };
        world
            .bound_index()
            .filter(|&order| catalog.get_at(order).is_some())
    }
    pub fn weapon(&self) -> crate::WeaponHandle {
        self.registry
            .bind_published_row(self.weapon)
            .expect("selected appearance owner")
            .handle()
    }
}

impl WeaponRegistry {
    pub fn camouflage_caption(&self, weapon: u32, name: &str) -> Option<&str> {
        let slot = self.camouflage_slot(weapon, name)?;
        self.select_appearance(weapon, slot)?.caption()
    }

    pub fn camouflage_preview(&self, weapon: u32, name: &str) -> Option<String> {
        let slot = self.camouflage_slot(weapon, name)?;
        self.select_appearance(weapon, slot)?
            .preview()
            .map(str::to_owned)
    }

    pub fn select_appearance(&self, weapon: u32, slot: u8) -> Option<SelectedWeaponAppearance<'_>> {
        let appearance = self
            .rows
            .get(weapon as usize)?
            .appearances
            .iter()
            .find(|appearance| appearance.slot == slot)?;
        Some(SelectedWeaponAppearance {
            registry: self,
            weapon,
            appearance,
        })
    }

    pub fn appearances_of(&self, weapon: u32) -> Vec<SelectedWeaponAppearance<'_>> {
        self.rows.get(weapon as usize).map_or_else(Vec::new, |row| {
            row.appearances
                .iter()
                .filter(|appearance| appearance.slot != 0)
                .map(|appearance| SelectedWeaponAppearance {
                    registry: self,
                    weapon,
                    appearance,
                })
                .collect()
        })
    }

    pub fn camouflage_choices(&self, weapon: u32) -> Vec<(u8, &str)> {
        self.rows.get(weapon as usize).map_or_else(Vec::new, |row| {
            row.appearances
                .iter()
                .filter(|appearance| appearance.slot != 0)
                .map(|appearance| (appearance.slot, appearance.name.as_str()))
                .collect()
        })
    }
}
