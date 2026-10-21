#![cfg(not(target_arch = "wasm32"))]

use crate::state::FrameData;
use rayon::prelude::*;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

/// 軌跡ファイルから指定のフレーム（インデックス）を非同期に読み込み、Rayonでパースする。
/// XYZフォーマットを想定: 1フレームあたり (num_atoms + 2) 行
pub async fn load_frame_from_file<P: AsRef<Path>>(
    file_path: P,
    frame_index: u32,
    num_atoms: usize,
) -> Result<FrameData, String> {
    let file = File::open(file_path)
        .await
        .map_err(|e| format!("Failed to open trajectory file: {}", e))?;
    let mut reader = BufReader::new(file);
    let mut lines = Vec::with_capacity(num_atoms + 2);

    let lines_per_frame = num_atoms + 2;
    let start_line = frame_index as usize * lines_per_frame;

    let mut current_line = 0;
    let mut line_buf = String::new();

    while reader
        .read_line(&mut line_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?
        > 0
    {
        if current_line >= start_line && current_line < start_line + lines_per_frame {
            lines.push(line_buf.trim().to_string());
        }
        if current_line >= start_line + lines_per_frame {
            break;
        }
        line_buf.clear();
        current_line += 1;
    }

    if lines.len() < lines_per_frame {
        return Err(format!(
            "Frame {} is out of range or incomplete (expected {} lines, got {})",
            frame_index,
            lines_per_frame,
            lines.len()
        ));
    }

    // Rayonを使って、取得した文字列配列を並列にパース
    let step: u32 = lines[1]
        .split('=')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(frame_index);

    let atom_lines = &lines[2..];
    let positions: Result<Vec<[f32; 3]>, String> = atom_lines
        .par_iter()
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return Err(
                    "Invalid coordinates format: line must contain element x y z".to_string(),
                );
            }
            let x = parts[1].parse::<f32>().map_err(|e| e.to_string())?;
            let y = parts[2].parse::<f32>().map_err(|e| e.to_string())?;
            let z = parts[3].parse::<f32>().map_err(|e| e.to_string())?;
            Ok([x, y, z])
        })
        .collect();

    let positions = positions?;
    Ok(FrameData { step, positions })
}

/// テスト用およびデモ用のダミー軌跡ファイル（XYZ形式）を作成する
pub async fn create_dummy_trajectory_file<P: AsRef<Path>>(
    file_path: P,
    num_frames: u32,
    num_atoms: usize,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // 親ディレクトリの作成
    if let Some(parent) = file_path.as_ref().parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut file = File::create(file_path)
        .await
        .map_err(|e| format!("Failed to create dummy trajectory file: {}", e))?;

    for frame in 0..num_frames {
        let mut frame_str = format!("{}\nstep={}\n", num_atoms, frame);

        for a in 0..num_atoms {
            // 時間軸に沿ってゆっくりと回転・波打つダミーモーション
            let t = (a as f32) * 0.15 + (frame as f32) * 0.1;
            let r = 20.0 + (a as f32) * 0.01;
            let x = r * t.cos();
            let y = r * t.sin();
            let z = (a as f32) * 0.05 - 15.0;

            frame_str.push_str(&format!("C {} {} {}\n", x, y, z));
        }

        file.write_all(frame_str.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }

    file.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dummy_trajectory_load() {
        let temp_dir = std::env::temp_dir().join("yui_traj_test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("test_traj.xyz");

        let num_frames = 5;
        let num_atoms = 100;

        create_dummy_trajectory_file(&file_path, num_frames, num_atoms)
            .await
            .unwrap();

        let frame_data = load_frame_from_file(&file_path, 2, num_atoms)
            .await
            .unwrap();

        assert_eq!(frame_data.step, 2);
        assert_eq!(frame_data.positions.len(), num_atoms);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
