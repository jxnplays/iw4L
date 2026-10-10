scar2 color and specular slots are empty AuthoredImage placeholders; the real PNGs are on disk under the saluki models folder; no PNG-to-IWI path exists in the clone or the dump tools.
no MW2 weapon loads a bare IWI from disk; color and specular resolve by name into an .iwd or .ff.
an external PNG-to-IWI tool exists at github.com/bruhhwtf/iwi-converter; it is not present in the clone or the dump tools.
scar2 color and specular IWIs are packaged into nx1_scar2_images.iwd under images/; client can now resolve them by name once the .iwd is loaded.
nx1_scar2_images.iwd is now in the client's main\ directory at 1,049,150 bytes.
image_candidates lowercasing checked for scar2 specular name; result: survives.
image index cache check; result: next run sees it.
register_nx1_scar_body_color empty slots confirmed at crates/assets/src/session_load/match_walk.rs:58 and crates/assets/src/session_load/match_walk.rs:84.
normal decode path check; result: will locate.
scar2 material slot reachability; result: reached.
plan_material_color_maps invocation check; result: not invoked after registration.
NamespaceTrees indexing check; result: indexes main\.
game_main_for_zone derivation check; result: derives main\ correctly.