use crate::state::{AppState, AppStateDelta};

// --- 第2章: データ構造 & 状態管理 ---

/// MessagePackのヘッダーにおけるマジックナンバー ("YUI\0")
pub const MAGIC_YUI: &[u8; 4] = b"YUI\0";

/// 圧縮フラグ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None = 0,
    Zstd = 1,
}

impl TryFrom<u8> for Compression {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Compression::None),
            1 => Ok(Compression::Zstd),
            _ => Err("Invalid compression flag"),
        }
    }
}

/// 中間/ネットワーク/ディスク保存用 MessagePackファイルのパッキング
/// ヘッダー構成: [Magic: "YUI\0" (4B)] + [Version (1B)] + [Compression Flag (1B)] + [Payload]
pub fn pack_app_state(
    state: &AppState,
    version: u8,
    compression: Compression,
) -> Result<Vec<u8>, String> {
    let raw_payload = rmp_serde::to_vec_named(state).map_err(|e| e.to_string())?;

    let payload = match compression {
        Compression::None => raw_payload,
        Compression::Zstd => {
            #[cfg(target_arch = "wasm32")]
            {
                return Err("Zstd compression is not supported in Wasm environment".to_string());
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                zstd::encode_all(std::io::Cursor::new(raw_payload), 3).map_err(|e| e.to_string())?
            }
        }
    };

    let mut buffer = Vec::with_capacity(6 + payload.len());
    buffer.extend_from_slice(MAGIC_YUI);
    buffer.push(version);
    buffer.push(compression as u8);
    buffer.extend_from_slice(&payload);

    Ok(buffer)
}

/// MessagePackペイロードから `version` フィールドを覗き見るヘルパー。
/// 厳密なAppStateデシリアライズ前にフォーマット互換性を検証するために使用する。
///
/// `rmp_serde` はデフォルトで構造体を Array（タプル）形式でシリアライズし、
/// 名前付きでシリアライズした場合は Map 形式となるため、両方に対応する。
fn peek_state_format_version(payload: &[u8]) -> Result<u32, String> {
    let value: rmpv::Value = rmp_serde::from_slice(payload)
        .map_err(|e| format!("Failed to parse MessagePack payload: {e}"))?;
    if let Some(map) = value.as_map() {
        for (k, v) in map {
            if k.as_str() == Some("version") {
                return v.as_u64().map(|n| n as u32).ok_or_else(|| {
                    "Malformed AppState payload: version field is not a number".to_string()
                });
            }
        }
        return Err("Malformed AppState payload: missing version field in map".to_string());
    } else if let Some(array) = value.as_array() {
        // AppState の第1フィールドは `version: u32`
        if let Some(first) = array.first() {
            return first.as_u64().map(|n| n as u32).ok_or_else(|| {
                "Malformed AppState payload: first element (version) is not a number".to_string()
            });
        }
        return Err("Malformed AppState payload: empty array".to_string());
    }
    Err("Malformed AppState payload: not a map or array".to_string())
}

/// MessagePackファイルからの AppState デシリアライズ
pub fn unpack_app_state(data: &[u8]) -> Result<(u8, Compression, AppState), String> {
    if data.len() < 6 {
        return Err("Data too short to contain a valid YUI header".to_string());
    }

    let magic: [u8; 4] = data[0..4].try_into().unwrap();
    if &magic != MAGIC_YUI {
        return Err("Invalid magic bytes, expected 'YUI\\0'".to_string());
    }

    let version = data[4];
    let compression = Compression::try_from(data[5])?;

    let payload = &data[6..];

    let decompressed_payload = match compression {
        Compression::None => payload.to_vec(),
        Compression::Zstd => {
            #[cfg(target_arch = "wasm32")]
            {
                return Err("Zstd decompression is not supported in Wasm environment".to_string());
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                zstd::decode_all(std::io::Cursor::new(payload)).map_err(|e| e.to_string())?
            }
        }
    };

    // AppState構造体のフォーマット互換性検証（ヘッダーのversionバイトとは別物）
    let found_version = peek_state_format_version(&decompressed_payload)?;
    if found_version != crate::state::ATOM_GROUP_FORMAT_VERSION {
        return Err(format!(
            "Unsupported or legacy atom_group format (state.version = {found_version}, expected {}). \
             This file was likely saved before Phase 6e (real proteindf-bridge AtomGroup integration) \
             and cannot be loaded. Please regenerate or re-export it with a current version of YUI.",
            crate::state::ATOM_GROUP_FORMAT_VERSION
        ));
    }

    let state: AppState =
        rmp_serde::from_slice(&decompressed_payload).map_err(|e| e.to_string())?;

    Ok((version, compression, state))
}

// --- 第5章: リアルタイム・コラボレーション ---

/// リアルタイムコラボレーション用パケットのヘッダー情報
#[derive(Debug, Clone, PartialEq)]
pub struct CollabHeader {
    pub sender_id: [u8; 16],
    pub role_token: u64, // 8 Bytes
    pub timestamp: u64,  // 8 Bytes
}

/// コラボレーション通信のパッキング (Delta 同期)
/// パケット構成:
/// [Header: SenderID (16B) | RoleToken (8B) | Timestamp (8B)] + [Payload: AppStateDelta (MsgPack)]
pub fn pack_collab_packet(header: &CollabHeader, delta: &AppStateDelta) -> Result<Vec<u8>, String> {
    let payload = rmp_serde::to_vec(delta).map_err(|e| e.to_string())?;

    let mut buffer = Vec::with_capacity(32 + payload.len());
    buffer.extend_from_slice(&header.sender_id);
    buffer.extend_from_slice(&header.role_token.to_be_bytes());
    buffer.extend_from_slice(&header.timestamp.to_be_bytes());
    buffer.extend_from_slice(&payload);

    Ok(buffer)
}

/// コラボレーション通信のアンパッキング
pub fn unpack_collab_packet(data: &[u8]) -> Result<(CollabHeader, AppStateDelta), String> {
    if data.len() < 32 {
        return Err("Collaboration packet too short".to_string());
    }

    let mut sender_id = [0u8; 16];
    sender_id.copy_from_slice(&data[0..16]);

    let mut role_token_bytes = [0u8; 8];
    role_token_bytes.copy_from_slice(&data[16..24]);
    let role_token = u64::from_be_bytes(role_token_bytes);

    let mut timestamp_bytes = [0u8; 8];
    timestamp_bytes.copy_from_slice(&data[24..32]);
    let timestamp = u64::from_be_bytes(timestamp_bytes);

    let header = CollabHeader {
        sender_id,
        role_token,
        timestamp,
    };

    let payload = &data[32..];
    let delta: AppStateDelta = rmp_serde::from_slice(payload).map_err(|e| e.to_string())?;

    Ok((header, delta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom_group::AtomGroupExt;
    use crate::state::{Annotation, Atom, Molecule};

    #[test]
    fn test_pack_unpack_none() {
        let (group, _sec_map) = crate::atom_group::AtomGroup::create_test_peptide();
        let state = AppState {
            molecule: Molecule {
                atoms: vec![Atom {
                    id: 1,
                    element: "H".to_string(),
                    position: [1.0, 2.0, 3.0],
                    radius: 1.2,
                    color: [1.0, 1.0, 1.0, 1.0],
                    residue_name: "ALA".to_string(),
                    residue_index: 10,
                    chain_id: "A".to_string(),
                }],
                bonds: vec![],
            },
            annotations: vec![Annotation {
                id: "annot-1".to_string(),
                text: "Hydrogen".to_string(),
                position: [1.0, 2.0, 3.0],
                color: [0.0, 1.0, 0.0, 1.0],
                visible: true,
                target_atom_id: Some(1),
            }],
            atom_group: Some(group),
            representations: vec![crate::representation::Representation::new(
                "rep1",
                crate::selector::Selector::PathWildcard("/A/*".to_string()),
                crate::representation::RepresentationKind::BallAndStick,
                crate::representation::ColorScheme::Element,
            )],
            ..Default::default()
        };

        let packed = pack_app_state(&state, 1, Compression::None).unwrap();
        let (version, compression, unpacked) = unpack_app_state(&packed).unwrap();

        assert_eq!(version, 1);
        assert_eq!(compression, Compression::None);
        assert_eq!(unpacked.version, crate::state::ATOM_GROUP_FORMAT_VERSION);
        assert_eq!(unpacked.molecule.atoms.len(), 1);
        assert_eq!(unpacked.molecule.atoms[0].element, "H");
        assert_eq!(unpacked.annotations.len(), 1);
        assert_eq!(unpacked.annotations[0].text, "Hydrogen");
        assert_eq!(unpacked.atom_group, state.atom_group);
        assert_eq!(unpacked.representations, state.representations);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_pack_unpack_zstd() {
        let mut state = AppState::default();
        let (group, _sec_map) = crate::atom_group::AtomGroup::create_test_peptide();
        state.atom_group = Some(group);
        state.representations = vec![crate::representation::Representation::new(
            "rep_zstd",
            crate::selector::Selector::ResidueName(vec!["ALA".to_string()]),
            crate::representation::RepresentationKind::Cartoon,
            crate::representation::ColorScheme::SecondaryStructure,
        )];

        let packed = pack_app_state(&state, 2, Compression::Zstd).unwrap();
        let (version, compression, unpacked) = unpack_app_state(&packed).unwrap();

        assert_eq!(version, 2);
        assert_eq!(compression, Compression::Zstd);
        assert_eq!(unpacked.version, crate::state::ATOM_GROUP_FORMAT_VERSION);
        assert_eq!(unpacked.atom_group, state.atom_group);
        assert_eq!(unpacked.representations, state.representations);
    }

    #[test]
    fn test_unpack_app_state_unsupported_version() {
        let legacy_state = AppState {
            version: 1, // 旧バージョン
            ..Default::default()
        };

        let packed = pack_app_state(&legacy_state, 1, Compression::None).unwrap();
        let result = unpack_app_state(&packed);

        assert!(result.is_err(), "unpacking legacy version must fail");
        let err = result.err().unwrap();
        assert!(
            err.contains("Unsupported or legacy atom_group format"),
            "error message should indicate legacy format: {err}"
        );
        assert!(
            err.contains("state.version = 1"),
            "error message should mention found version: {err}"
        );
    }
}
