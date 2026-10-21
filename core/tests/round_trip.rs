//! architecture.md 2.1節に準拠した AppState のロスレス相互変換テスト
//! (Rust struct ⇄ MessagePack ⇄ YAML)

use yui_core::{
    ATOM_GROUP_FORMAT_VERSION, Annotation, AppState, AtomGroup, AtomGroupExt, CameraState,
    ColorScheme, Compression, DataPaths, Interaction, InteractionKind, InteractionSet,
    LaserPointer, Representation, RepresentationKind, SceneSettings, Selector, TrajectoryState,
    export_app_state_to_yaml, import_app_state_from_yaml, pack_app_state, unpack_app_state,
};

/// atom_group, representations, annotations, interactions 等を豊富に含むテスト用 AppState を生成する
fn create_test_populated_state() -> AppState {
    let (atom_group, sec_map) = AtomGroup::create_test_peptide();
    let molecule = atom_group.to_flat_molecule();

    let representations = vec![
        Representation::new(
            "rep_cartoon",
            Selector::PathWildcard("/A/*".to_string()),
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        )
        .with_opacity(0.8),
        Representation::new(
            "rep_stick",
            Selector::And(vec![
                Selector::ResidueName(vec!["ALA".to_string()]),
                Selector::Symbol(vec!["C".to_string(), "N".to_string(), "O".to_string()]),
            ]),
            RepresentationKind::BallAndStick,
            ColorScheme::Element,
        ),
        Representation::new(
            "rep_cpk",
            Selector::PathWildcard("/*/A/6/*".to_string()),
            RepresentationKind::Cpk,
            ColorScheme::Uniform([1.0, 0.5, 0.2, 1.0]),
        )
        .with_visible(false),
        Representation::new(
            "rep_interaction",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Uniform([1.0, 0.9, 0.2, 1.0]),
        )
        .with_interaction_kinds(vec![
            InteractionKind::HydrogenBond,
            InteractionKind::Disulfide,
        ]),
    ];

    AppState {
        version: ATOM_GROUP_FORMAT_VERSION,
        camera: CameraState {
            eye: [10.0, 20.0, 50.0],
            target: [0.0, 5.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 60.0,
        },
        scene: SceneSettings {
            background_color: [0.1, 0.2, 0.3, 1.0],
        },
        named_selections: {
            let mut map = std::collections::BTreeMap::new();
            map.insert("all_atoms".to_string(), Selector::All);
            map.insert("active_site".to_string(), Selector::AtomIds(vec![1, 2, 3]));
            map.insert(
                "non_carbon".to_string(),
                Selector::Not {
                    inner: Box::new(Selector::Symbol(vec!["C".to_string()])),
                },
            );
            map
        },
        atom_group: Some(atom_group),
        secondary_structure_map: sec_map,
        interactions: InteractionSet::with_interactions(vec![
            Interaction::with_metrics(
                InteractionKind::Disulfide,
                vec![
                    "/model_1/A/6/SG".to_string(),
                    "/model_1/A/11/SG".to_string(),
                ],
                Some(2.02),
                None,
                Some("disulfide".to_string()),
            ),
            Interaction::with_metrics(
                InteractionKind::HydrogenBond,
                vec!["/model_1/A/2/N".to_string(), "/model_1/A/1/O".to_string()],
                Some(2.85),
                Some(160.0),
                Some("backbone".to_string()),
            ),
            Interaction::with_metrics(
                InteractionKind::SaltBridge,
                vec!["/model_1/A/4/".to_string(), "/model_1/A/1/".to_string()],
                None,
                None,
                Some("GLU - NTM".to_string()),
            ),
            Interaction::with_metrics(
                InteractionKind::ChPi,
                vec![
                    "/model_1/B/15/CD1".to_string(),
                    "/model_1/A/19/".to_string(),
                ],
                Some(3.8),
                Some(25.0),
                Some("CH-pi (TYR)".to_string()),
            ),
        ]),
        molecule,
        representations,
        annotations: vec![Annotation {
            id: "ann_1".to_string(),
            text: "Ala N-terminus".to_string(),
            position: [0.0, 0.0, 0.0],
            color: [0.0, 1.0, 0.0, 1.0],
            visible: true,
            target_atom_id: Some(1),
        }],
        data_paths: DataPaths {
            pdb_path: Some("/virtual/path/data.pdb".to_string()),
            cube_path: Some("/virtual/path/density.cube".to_string()),
        },
        trajectory: TrajectoryState {
            file_path: "trajectory.trj".to_string(),
            total_frames: 50,
            current_frame_index: 5,
            play_speed_fps: 24,
            is_playing: true,
        },
        laser_pointer: LaserPointer {
            active: true,
            position: [1.45, 0.0, 0.0],
            user_id: "instructor_1".to_string(),
            color: [1.0, 0.0, 0.0, 1.0],
        },
        active_model: None,
        cube_view: Default::default(),
    }
}

#[test]
fn test_msgpack_round_trip_uncompressed() {
    let original = create_test_populated_state();
    let packed =
        pack_app_state(&original, 1, Compression::None).expect("Failed to pack MessagePack");
    let (version, compression, restored) =
        unpack_app_state(&packed).expect("Failed to unpack MessagePack");

    assert_eq!(version, 1);
    assert_eq!(compression, Compression::None);
    assert_eq!(
        original, restored,
        "MessagePack (None) round-trip must be completely lossless"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn test_msgpack_round_trip_zstd() {
    let original = create_test_populated_state();
    let packed = pack_app_state(&original, 1, Compression::Zstd)
        .expect("Failed to pack MessagePack with Zstd");
    let (version, compression, restored) =
        unpack_app_state(&packed).expect("Failed to unpack MessagePack with Zstd");

    assert_eq!(version, 1);
    assert_eq!(compression, Compression::Zstd);
    assert_eq!(
        original, restored,
        "MessagePack (Zstd) round-trip must be completely lossless"
    );
}

#[test]
fn test_yaml_round_trip_in_memory() {
    let original = create_test_populated_state();
    let yaml_str = serde_yaml::to_string(&original).expect("Failed to serialize to YAML");
    let restored: AppState =
        serde_yaml::from_str(&yaml_str).expect("Failed to deserialize from YAML");

    assert_eq!(
        original, restored,
        "YAML in-memory round-trip must be completely lossless"
    );
}

#[test]
fn test_three_way_round_trip_struct_to_yaml_to_msgpack_to_struct() {
    let original = create_test_populated_state();

    // 1. Struct -> YAML string
    let yaml_str = serde_yaml::to_string(&original).unwrap();

    // 2. YAML string -> Struct
    let from_yaml: AppState = serde_yaml::from_str(&yaml_str).unwrap();
    assert_eq!(original, from_yaml);

    // 3. Struct -> MessagePack
    let packed = pack_app_state(&from_yaml, 1, Compression::None).unwrap();

    // 4. MessagePack -> Struct
    let (_, _, from_msgpack) = unpack_app_state(&packed).unwrap();
    assert_eq!(
        original, from_msgpack,
        "Three-way round-trip (Struct -> YAML -> MsgPack -> Struct) must be lossless"
    );
}

#[test]
fn test_three_way_round_trip_struct_to_msgpack_to_yaml_to_struct() {
    let original = create_test_populated_state();

    // 1. Struct -> MessagePack
    let packed = pack_app_state(&original, 1, Compression::Zstd).unwrap();

    // 2. MessagePack -> Struct
    let (_, _, from_msgpack) = unpack_app_state(&packed).unwrap();
    assert_eq!(original, from_msgpack);

    // 3. Struct -> YAML
    let yaml_str = serde_yaml::to_string(&from_msgpack).unwrap();

    // 4. YAML -> Struct
    let from_yaml: AppState = serde_yaml::from_str(&yaml_str).unwrap();
    assert_eq!(
        original, from_yaml,
        "Three-way round-trip (Struct -> MsgPack -> YAML -> Struct) must be lossless"
    );
}

#[test]
fn test_yaml_file_export_import_with_populated_state() {
    let temp_dir = std::env::temp_dir().join(format!("yui_roundtrip_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let yaml_file_path = temp_dir.join("workspace.yaml");
    let mock_pdb_path = temp_dir.join("structures").join("peptide.pdb");
    std::fs::create_dir_all(mock_pdb_path.parent().unwrap()).unwrap();
    std::fs::write(&mock_pdb_path, "MOCK PDB DATA").unwrap();

    let mut state = create_test_populated_state();
    state.data_paths.pdb_path = Some(mock_pdb_path.to_str().unwrap().to_string());
    state.data_paths.cube_path = None;

    // Export
    export_app_state_to_yaml(&yaml_file_path, &state).expect("Failed to export YAML");

    // Check that relative path was written
    let file_content = std::fs::read_to_string(&yaml_file_path).unwrap();
    assert!(file_content.contains("pdb_path: structures/peptide.pdb"));
    assert!(file_content.contains("rep_cartoon"));
    assert!(file_content.contains("TestPeptide"));

    // Import
    let imported_state =
        import_app_state_from_yaml(&yaml_file_path).expect("Failed to import YAML");

    assert_eq!(state.version, imported_state.version);
    assert_eq!(state.camera, imported_state.camera);
    assert_eq!(state.atom_group, imported_state.atom_group);
    assert_eq!(state.molecule, imported_state.molecule);
    assert_eq!(state.representations, imported_state.representations);
    assert_eq!(state.named_selections, imported_state.named_selections);
    assert_eq!(state.annotations, imported_state.annotations);
    assert_eq!(state.trajectory, imported_state.trajectory);
    assert_eq!(state.laser_pointer, imported_state.laser_pointer);

    // Re-resolved absolute path should match
    assert_eq!(
        imported_state.data_paths.pdb_path.unwrap(),
        mock_pdb_path.to_str().unwrap()
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_backward_compatibility_missing_scene_defaults() {
    // 既存の完全な AppState から scene キーを除去して旧形式データをシミュレート
    let original = create_test_populated_state();
    let mut yaml_val: serde_yaml::Value = serde_yaml::to_value(&original).unwrap();
    if let serde_yaml::Value::Mapping(ref mut map) = yaml_val {
        map.remove(serde_yaml::Value::String("scene".to_string()));
    }
    let legacy_yaml = serde_yaml::to_string(&yaml_val).unwrap();

    let state: AppState = serde_yaml::from_str(&legacy_yaml)
        .expect("Legacy YAML without scene should deserialize successfully with defaults");
    assert_eq!(state.scene, SceneSettings::default());
    assert_eq!(
        state.scene.background_color,
        yui_core::state::DEFAULT_BACKGROUND_COLOR
    );
}

#[test]
fn test_selector_not_yaml_and_msgpack_round_trip() {
    let mut state = AppState::default();
    state.representations.push(Representation::new(
        "rep_not_isolated",
        Selector::Not {
            inner: Box::new(Selector::AtomIds(vec![1, 2, 3])),
        },
        RepresentationKind::BallAndStick,
        ColorScheme::Uniform([1.0, 0.0, 0.0, 1.0]),
    ));
    state.representations.push(Representation::new(
        "rep_not_nested",
        Selector::And(vec![
            Selector::PathWildcard("/A/*".to_string()),
            Selector::Not {
                inner: Box::new(Selector::Symbol(vec!["C".to_string()])),
            },
        ]),
        RepresentationKind::Cartoon,
        ColorScheme::Chain,
    ));

    // 1. YAML 往復テスト（以前はここで "nested enums in YAML is not supported yet" エラーが発生していた）
    let yaml_str =
        serde_yaml::to_string(&state).expect("YAML serialization of Selector::Not must succeed");
    let from_yaml: AppState =
        serde_yaml::from_str(&yaml_str).expect("YAML deserialization must succeed");
    assert_eq!(
        state, from_yaml,
        "YAML round-trip with Selector::Not must be completely lossless"
    );

    // 2. MessagePack 往復テスト
    let packed =
        pack_app_state(&state, 1, Compression::None).expect("MessagePack packing must succeed");
    let (_, _, from_msgpack) =
        unpack_app_state(&packed).expect("MessagePack unpacking must succeed");
    assert_eq!(
        state, from_msgpack,
        "MessagePack round-trip with Selector::Not must be completely lossless"
    );
}

#[test]
fn test_backward_compatibility_missing_named_selections() {
    // 既存の完全な AppState から named_selections キーを除去して旧形式データをシミュレート
    let original = create_test_populated_state();
    let mut yaml_val: serde_yaml::Value = serde_yaml::to_value(&original).unwrap();
    if let serde_yaml::Value::Mapping(ref mut map) = yaml_val {
        map.remove(serde_yaml::Value::String("named_selections".to_string()));
    }
    let legacy_yaml = serde_yaml::to_string(&yaml_val).unwrap();

    let state: AppState = serde_yaml::from_str(&legacy_yaml).expect(
        "Legacy YAML without named_selections should deserialize successfully with empty map",
    );
    assert!(state.named_selections.is_empty());
}

#[test]
fn test_backward_compatibility_missing_interactions() {
    // 既存の完全な AppState から interactions キーを除去して旧形式データをシミュレート
    let original = create_test_populated_state();
    let mut yaml_val: serde_yaml::Value = serde_yaml::to_value(&original).unwrap();
    if let serde_yaml::Value::Mapping(ref mut map) = yaml_val {
        map.remove(serde_yaml::Value::String("interactions".to_string()));
    }
    let legacy_yaml = serde_yaml::to_string(&yaml_val).unwrap();

    let state: AppState = serde_yaml::from_str(&legacy_yaml).expect(
        "Legacy YAML without interactions should deserialize successfully with empty InteractionSet",
    );
    assert!(state.interactions.is_empty());
}

fn wrap_payload_with_header(raw_payload: &[u8], version: u8, compression: Compression) -> Vec<u8> {
    let payload = match compression {
        Compression::None => raw_payload.to_vec(),
        Compression::Zstd => {
            #[cfg(target_arch = "wasm32")]
            {
                panic!("Zstd compression is not supported in Wasm environment");
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                zstd::encode_all(std::io::Cursor::new(raw_payload), 3).unwrap()
            }
        }
    };

    let mut buffer = Vec::with_capacity(6 + payload.len());
    buffer.extend_from_slice(yui_core::packing::MAGIC_YUI);
    buffer.push(version);
    buffer.push(compression as u8);
    buffer.extend_from_slice(&payload);
    buffer
}

#[test]
fn test_backward_compatibility_msgpack_array_format() {
    let mut original = create_test_populated_state();
    original.interactions = InteractionSet::default();

    // rmp_serde::to_vec (配列形式) でシリアライズ
    let raw_array_payload =
        rmp_serde::to_vec(&original).expect("Serialization to array format should succeed");

    // 1. 非圧縮 (Compression::None) でのヘッダーラッピングと unpack_app_state 検証
    let packed_none = wrap_payload_with_header(&raw_array_payload, 1, Compression::None);
    let (v_none, comp_none, unpacked_none) =
        unpack_app_state(&packed_none).expect("unpack_app_state from array payload must succeed");
    assert_eq!(v_none, 1);
    assert_eq!(comp_none, Compression::None);
    assert_eq!(
        original, unpacked_none,
        "Unpacked AppState from array format (None) must match original"
    );

    // 2. Zstd圧縮 (Compression::Zstd) でのヘッダーラッピングと unpack_app_state 検証
    let packed_zstd = wrap_payload_with_header(&raw_array_payload, 1, Compression::Zstd);
    let (v_zstd, comp_zstd, unpacked_zstd) = unpack_app_state(&packed_zstd)
        .expect("unpack_app_state from zstd array payload must succeed");
    assert_eq!(v_zstd, 1);
    assert_eq!(comp_zstd, Compression::Zstd);
    assert_eq!(
        original, unpacked_zstd,
        "Unpacked AppState from array format (Zstd) must match original"
    );
}

#[test]
fn test_backward_compatibility_msgpack_legacy_2026_10_0_array_format() {
    let mut original = create_test_populated_state();
    original.interactions = InteractionSet::default();

    // rmp_serde::to_vec (配列形式) でシリアライズ
    let raw_array_payload =
        rmp_serde::to_vec(&original).expect("Serialization to array format should succeed");

    // rmpv::Value に変換し、末尾の interactions 要素（index 12）を取り除いて 2026.10.0 時点（要素数12）を再現
    let mut val: rmpv::Value =
        rmp_serde::from_slice(&raw_array_payload).expect("Decoding to rmpv::Value should succeed");
    let arr = match val {
        rmpv::Value::Array(ref mut arr) => arr,
        _ => panic!("Expected array payload from rmp_serde::to_vec"),
    };
    assert_eq!(
        arr.len(),
        15,
        "Current AppState should have 15 fields when serialized as array"
    );
    let removed_cube_view = arr.pop().expect("Should pop cube_view element");
    assert!(removed_cube_view.is_array());
    let removed_active_model = arr.pop().expect("Should pop active_model element");
    assert_eq!(removed_active_model, rmpv::Value::Nil);
    let removed_interactions = arr.pop().expect("Should pop interactions element");
    assert_eq!(
        arr.len(),
        12,
        "2026.10.0 legacy array should have exactly 12 fields"
    );

    // 空の InteractionSet 相当が取り除かれたことを念のため確認
    let expected_empty_inter: rmpv::Value =
        rmp_serde::from_slice(&rmp_serde::to_vec(&InteractionSet::default()).unwrap()).unwrap();
    assert_eq!(removed_interactions, expected_empty_inter);

    let mut legacy_raw_payload = Vec::new();
    rmpv::encode::write_value(&mut legacy_raw_payload, &val)
        .expect("Encoding modified array must succeed");

    // 1. 非圧縮 (Compression::None) でのヘッダーラッピングと unpack_app_state 検証
    let packed_none = wrap_payload_with_header(&legacy_raw_payload, 1, Compression::None);
    let (v_none, comp_none, unpacked_none) = unpack_app_state(&packed_none).expect(
        "unpack_app_state must successfully decode legacy 2026.10.0 12-element array payload",
    );
    assert_eq!(v_none, 1);
    assert_eq!(comp_none, Compression::None);
    assert!(
        unpacked_none.interactions.is_empty(),
        "interactions must be default (empty) in unpacked legacy state"
    );
    assert_eq!(
        unpacked_none.active_model, None,
        "active_model must be default (None) in unpacked legacy state"
    );
    assert_eq!(
        original, unpacked_none,
        "Legacy 2026.10.0 unpacked state must match original with empty interactions and None active_model"
    );

    // 2. Zstd圧縮 (Compression::Zstd) でのヘッダーラッピングと unpack_app_state 検証
    let packed_zstd = wrap_payload_with_header(&legacy_raw_payload, 1, Compression::Zstd);
    let (v_zstd, comp_zstd, unpacked_zstd) = unpack_app_state(&packed_zstd).expect(
        "unpack_app_state must successfully decode zstd legacy 2026.10.0 12-element array payload",
    );
    assert_eq!(v_zstd, 1);
    assert_eq!(comp_zstd, Compression::Zstd);
    assert!(
        unpacked_zstd.interactions.is_empty(),
        "interactions must be default (empty) in unpacked zstd legacy state"
    );
    assert_eq!(
        original, unpacked_zstd,
        "Legacy 2026.10.0 unpacked zstd state must match original with empty interactions"
    );
}

#[test]
fn test_legacy_yaml_without_interaction_kinds_in_representation() {
    // 既存の populated な AppState から各 representation の interaction_kinds を除去して旧形式データをシミュレート
    let original = create_test_populated_state();
    let mut yaml_val: serde_yaml::Value = serde_yaml::to_value(&original).unwrap();
    if let serde_yaml::Value::Mapping(map) = &mut yaml_val
        && let Some(serde_yaml::Value::Sequence(reps)) =
            map.get_mut(serde_yaml::Value::String("representations".to_string()))
    {
        for rep in reps {
            if let serde_yaml::Value::Mapping(rep_map) = rep {
                rep_map.remove(serde_yaml::Value::String("interaction_kinds".to_string()));
            }
        }
    }
    let legacy_yaml = serde_yaml::to_string(&yaml_val).unwrap();
    assert!(
        !legacy_yaml.contains("interaction_kinds"),
        "Sanity check: legacy YAML should not contain interaction_kinds"
    );

    let state: AppState = serde_yaml::from_str(&legacy_yaml).expect(
        "Legacy YAML without interaction_kinds in representations should deserialize successfully",
    );

    // representations が正しくデシリアライズされ、interaction_kinds が空（デフォルト）になることを確認
    assert_eq!(state.representations.len(), original.representations.len());
    for rep in &state.representations {
        assert!(
            rep.interaction_kinds.is_empty(),
            "interaction_kinds should default to empty vector for legacy representation"
        );
    }
}

#[test]
fn test_backward_compatibility_missing_active_model() {
    // 1. YAML形式で active_model キーが存在しない旧形式データからデシリアライズ
    let original = create_test_populated_state();
    let mut yaml_val: serde_yaml::Value = serde_yaml::to_value(&original).unwrap();
    if let serde_yaml::Value::Mapping(ref mut map) = yaml_val {
        map.remove(serde_yaml::Value::String("active_model".to_string()));
    }
    let legacy_yaml = serde_yaml::to_string(&yaml_val).unwrap();
    assert!(
        !legacy_yaml.contains("active_model"),
        "legacy YAML should not contain active_model"
    );

    let state: AppState = serde_yaml::from_str(&legacy_yaml)
        .expect("Legacy YAML without active_model should deserialize successfully with None");
    assert_eq!(state.active_model, None);

    // 2. MessagePack (Map形式) で active_model キーが存在しない旧形式データを検証
    let mut map_val: rmpv::Value =
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&original).unwrap()).unwrap();
    if let rmpv::Value::Map(ref mut map) = map_val {
        map.retain(|(k, _)| k.as_str() != Some("active_model"));
    }
    let mut legacy_map_payload = Vec::new();
    rmpv::encode::write_value(&mut legacy_map_payload, &map_val).unwrap();
    let packed_map = wrap_payload_with_header(&legacy_map_payload, 1, Compression::None);
    let (_v, _comp, unpacked) = unpack_app_state(&packed_map).expect(
        "unpack_app_state must successfully decode legacy map payload without active_model",
    );
    assert_eq!(unpacked.active_model, None);

    // 3. MessagePack 配列形式（13要素: interactions はデフォルト、active_model なし）を検証
    let mut original_array = original.clone();
    original_array.interactions = InteractionSet::default();
    let raw_array_payload =
        rmp_serde::to_vec(&original_array).expect("Serialization to array format should succeed");
    let mut val: rmpv::Value =
        rmp_serde::from_slice(&raw_array_payload).expect("Decoding to rmpv::Value should succeed");
    if let rmpv::Value::Array(ref mut arr) = val {
        assert_eq!(arr.len(), 15);
        arr.pop(); // cube_view を除去して 14 要素（2026.10.3 の形式）にする
        assert_eq!(arr.len(), 14);
        let mut v14 = Vec::new();
        rmpv::encode::write_value(&mut v14, &rmpv::Value::Array(arr.clone())).unwrap();
        let (_v, _c, unpacked14) =
            unpack_app_state(&wrap_payload_with_header(&v14, 1, Compression::None))
                .expect("unpack_app_state must decode the 14-element array of 2026.10.3");
        assert_eq!(unpacked14.cube_view, Default::default());
        arr.pop(); // active_model を除去して 13 要素にする
        assert_eq!(arr.len(), 13);
    }
    let mut legacy_array_payload = Vec::new();
    rmpv::encode::write_value(&mut legacy_array_payload, &val).unwrap();
    let packed_array = wrap_payload_with_header(&legacy_array_payload, 1, Compression::None);
    let (_v, _comp, unpacked_arr) = unpack_app_state(&packed_array)
        .expect("unpack_app_state must successfully decode legacy 13-element array payload");
    assert_eq!(unpacked_arr.active_model, None);
}
