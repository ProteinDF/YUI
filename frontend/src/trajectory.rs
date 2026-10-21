//! 軌跡ファイルを開く操作（architecture.md 4.2節）。ファイルの解析と、フレームの読み出しは `yui_core::xyz_trajectory`。

use std::sync::Arc;

use yui_core::xyz_trajectory::XyzTrajectory;

/// 開いている軌跡と、その再生の状態（利用者ごと。保存しない）
#[derive(Debug, Clone)]
pub struct LoadedTrajectory {
    pub traj: Arc<XyzTrajectory>,
    /// 今、構造に反映しているフレーム（0 始まり）
    pub current: usize,
    /// 今のフレームのステップ（コメントの `step=N`、なければフレームの番号）
    pub step: u32,
    /// 軌跡を開く前の、構造の座標（閉じるときに戻す）
    pub original: Vec<[f32; 3]>,
}

/// メモリ上のバイト列（Project タブで取得したファイル）から、軌跡の索引を作る。
/// 大きなファイルでも画面を止めないよう、ネイティブ版では別のスレッドで走査する。
pub async fn open_from_bytes(
    name: String,
    bytes: Vec<u8>,
) -> Result<Option<Arc<XyzTrajectory>>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || XyzTrajectory::from_bytes(name, bytes))
            .await
            .map_err(|e| format!("The trajectory could not be read: {e}"))?
            .map(|t| Some(Arc::new(t)))
    }
    #[cfg(target_arch = "wasm32")]
    {
        XyzTrajectory::from_bytes(name, bytes).map(|t| Some(Arc::new(t)))
    }
}

/// File → Open Trajectory... のダイアログ。ネイティブ版は、ファイルを全部は読まず、索引だけ作る
/// （再生のときに、必要なフレームだけ読む）。ブラウザ版は、ファイルの内容をメモリに持つ。
pub async fn open_trajectory_dialog() -> Result<Option<Arc<XyzTrajectory>>, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("XYZ trajectory", &["xyz"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = handle.path().to_path_buf();
        tokio::task::spawn_blocking(move || XyzTrajectory::from_file(&path))
            .await
            .map_err(|e| format!("The trajectory could not be read: {e}"))?
            .map(|t| Some(Arc::new(t)))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = handle.file_name();
        let bytes = handle.read().await;
        XyzTrajectory::from_bytes(name, bytes).map(|t| Some(Arc::new(t)))
    }
}
