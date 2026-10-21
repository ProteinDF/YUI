use crate::packing::CollabHeader;
use crate::role::UserRole;
use crate::state::{AppState, AppStateDelta};

/// リアルタイムコラボレーション用パケット（ヘッダー + 差分ペイロード）
/// (architecture.md - 7.2節)
#[derive(Debug, Clone, PartialEq)]
pub struct CollabPacket {
    pub header: CollabHeader,
    pub delta: AppStateDelta,
}

impl CollabPacket {
    pub fn new(header: CollabHeader, delta: AppStateDelta) -> Self {
        Self { header, delta }
    }
}

/// 通信層を抽象化するトランスポートトレイト（architecture.md 7.2節）
///
/// プラットフォーム非依存性・WebAssembly（wasm32-unknown-unknown）互換性を維持するため、
/// core にはネットワーク依存のない純粋なトレイトのみを定義します。
pub trait CollabTransport {
    type Error: std::fmt::Debug;

    fn send(&mut self, packet: CollabPacket) -> Result<(), Self::Error>;
    fn recv(&mut self) -> Result<Option<CollabPacket>, Self::Error>;
}

/// コラボレーションにおける、受信パケットの Role 権限検証
/// 権限が不十分な変更要求（Viewerからのアノテーション追加など）を検出して拒否する。
pub fn verify_role_action(role: UserRole, delta: &AppStateDelta) -> bool {
    match role {
        UserRole::Host => {
            // ホストはすべての操作権限を100%保持
            true
        }
        UserRole::Editor => {
            // エディタはカメラ、アノテーション、ポインター操作が可能
            true
        }
        UserRole::Viewer => {
            // ビューアは読み取り専用
            // カメラ、ポインターなどのローカルな視点移動やシグナルは許可するが、アノテーションや表現、名前付き選択の変更は拒否する
            !(delta.annotations.is_some()
                || delta.representations.is_some()
                || delta.named_selections.is_some())
        }
    }
}

/// 受信した AppStateDelta を現在の AppState に適用（マージ）する
pub fn apply_app_state_delta(state: &mut AppState, delta: &AppStateDelta) {
    if let Some(ref camera) = delta.camera {
        state.camera = camera.clone();
    }
    if let Some(ref settings) = delta.scene {
        state.scene = settings.clone();
    }
    if let Some(ref named) = delta.named_selections {
        state.named_selections = named.clone();
    }
    if let Some(ref representations) = delta.representations {
        state.representations = representations.clone();
    }
    if let Some(ref annotations) = delta.annotations {
        state.annotations = annotations.clone();
    }
    if let Some(ref laser_pointer) = delta.laser_pointer {
        state.laser_pointer = laser_pointer.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Annotation, CameraState};

    #[test]
    fn test_verify_role_action() {
        let mut delta = AppStateDelta {
            camera: Some(CameraState::default()),
            ..Default::default()
        };

        // カメラ操作はViewerでも許可
        assert!(verify_role_action(UserRole::Viewer, &delta));

        // アノテーション変更はViewerでは拒否
        delta.annotations = Some(vec![Annotation {
            id: "1".to_string(),
            text: "test".to_string(),
            position: [0.0, 0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
            visible: true,
            target_atom_id: None,
        }]);
        assert!(!verify_role_action(UserRole::Viewer, &delta));

        // 名前付き選択の変更もViewerでは拒否
        delta.annotations = None;
        delta.named_selections = Some(std::collections::BTreeMap::new());
        assert!(!verify_role_action(UserRole::Viewer, &delta));

        // Hostなら許可される
        assert!(verify_role_action(UserRole::Host, &delta));
    }

    #[test]
    fn test_apply_delta() {
        let mut state = AppState::default();
        let mut delta = AppStateDelta::default();

        let target_camera = CameraState {
            eye: [10.0, 20.0, 30.0],
            ..Default::default()
        };
        delta.camera = Some(target_camera.clone());

        apply_app_state_delta(&mut state, &delta);
        assert_eq!(state.camera, target_camera);
    }
}
