// SPDX-FileCopyrightText: The Yui development team
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use crate::atom_group::AtomGroup;
use proteindf_bridge::format::{MmcifWriteOptions, Pdb, SimpleMmcif};

/// 構造ファイルのエクスポート形式（PDB / mmCIF）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureExportFormat {
    Pdb,
    Mmcif,
}

impl StructureExportFormat {
    /// 拡張子文字列（大文字小文字を問わない、先頭のドット有無も許容）から形式を判定する。
    pub fn from_extension(ext: &str) -> Option<Self> {
        let trimmed = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        match trimmed.as_str() {
            "pdb" | "ent" => Some(Self::Pdb),
            "cif" | "mmcif" => Some(Self::Mmcif),
            _ => None,
        }
    }

    /// ファイルパスの拡張子から形式を判定する。
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let ext = path.as_ref().extension()?.to_str()?;
        Self::from_extension(ext)
    }

    /// 既定のファイル拡張子（ドットなし）
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Pdb => "pdb",
            Self::Mmcif => "cif",
        }
    }
}

/// `AtomGroup` を指定された形式（PDB / mmCIF）の文字列に変換して書き出す。
///
/// # 動作仕様
/// - マルチモデル構造の場合、表示モデル（`active_model`）に関係なく全モデルを書き出す。
/// - PDB 形式: `is_charge2tempfactor = false`。bridge の PDB 書き出しは、電荷を PDB の電荷カラムに
///   整数（`{:+1.0}`）として書くため、部分電荷は整数に丸められる（bridge 側の挙動）。
/// - mmCIF 形式: 既定オプション（`charge_to_b_factor = false`）。
///   非整数電荷（部分電荷）が含まれる場合は mmCIF の形式電荷制約によりエラーとなるため、
///   利用者に PDB への切り替えを促す分かりやすいエラーメッセージを返す（PDB でも部分電荷は
///   丸められる旨をメッセージに含める）。
pub fn export_structure_to_string(
    atom_group: &AtomGroup,
    format: StructureExportFormat,
) -> Result<String, String> {
    match format {
        StructureExportFormat::Pdb => {
            let mut pdb = Pdb::new(None);
            pdb.set_by_atomgroup(atom_group, false)
                .map_err(|e| format!("Failed to export structure to PDB: {e}"))?;
            Ok(pdb.get_text())
        }
        StructureExportFormat::Mmcif => {
            let opts = MmcifWriteOptions::default();
            let mut buf = Vec::new();
            SimpleMmcif::write_structure(atom_group, &mut buf, &opts).map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("non-integer formal charge")
                    || err_str.contains("formal charge in mmCIF must be an integer")
                {
                    format!(
                        "Structures with non-integer partial charges cannot be exported to mmCIF, because mmCIF formal charges must be integers. Export to PDB instead (note: PDB also stores integer formal charges, so partial charges are rounded). ({err_str})"
                    )
                } else {
                    format!("Failed to export structure to mmCIF: {err_str}")
                }
            })?;
            String::from_utf8(buf)
                .map_err(|e| format!("Invalid UTF-8 sequence in exported mmCIF output: {e}"))
        }
    }
}
