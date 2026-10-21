//! 「保存だけ」（圧縮なし）の zip を、メモリ上で作る。
//!
//! ブラウザ版のアニメーションの書き出しで、連番の PNG をまとめて 1 つのファイルとしてダウンロードさせるために使う
//! （ブラウザには、フォルダへ書く手段も、ffmpeg もない）。PNG は、もう圧縮されているので、圧縮は要らない。
//! 依存を増やさないよう、必要なぶんだけ書く: 各ファイルの局所ヘッダー、データ、中央ディレクトリ、終端レコード。
//! ZIP64 は使わない。ファイルが 65,535 個を超える、または全体が 4 GiB 以上になるときは、エラーにする。

// ブラウザ版（wasm）が使う。デスクトップ版でも、テストのために一緒にビルドするので、ふだんは使われない。
#![cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]

/// zip に入れるファイル 1 つ
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// 局所ヘッダーなどに書く、固定の日時（2026-01-01 00:00:00。DOS 形式）。
/// 中身が同じなら、同じバイト列になる（作った時刻に左右されない）。
const DOS_DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// 一般目的ビット 11（ファイル名が UTF-8）
const FLAG_UTF8: u16 = 1 << 11;

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    table
}

/// CRC-32（zip、PNG、gzip と同じ多項式）
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(crc32_table);
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// `entries` を、順に並べた zip（圧縮なし）にして返す。
pub fn build_zip(entries: &[ZipEntry]) -> Result<Vec<u8>, String> {
    if entries.len() > u16::MAX as usize {
        return Err(format!(
            "A zip holds at most {} files ({} given)",
            u16::MAX,
            entries.len()
        ));
    }
    let total: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    if total + 64 * entries.len() as u64 + 1024 >= u32::MAX as u64 {
        return Err("The files are too large for a zip (4 GiB)".to_string());
    }

    let mut out: Vec<u8> = Vec::with_capacity(total as usize + 128 * entries.len() + 64);
    let mut central: Vec<u8> = Vec::with_capacity(80 * entries.len());
    for e in entries {
        let name = e.name.as_bytes();
        if name.len() > u16::MAX as usize {
            return Err(format!("The file name '{}' is too long", e.name));
        }
        let crc = crc32(&e.data);
        let offset = out.len() as u32;
        let size = e.data.len() as u32;

        // 局所ヘッダー
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // 展開に必要なバージョン
        out.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // 圧縮方式: 保存のみ
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes()); // 圧縮後の大きさ
        out.extend_from_slice(&size.to_le_bytes()); // 元の大きさ
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // 追加フィールドなし
        out.extend_from_slice(name);
        out.extend_from_slice(&e.data);

        // 中央ディレクトリのレコード
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // 作ったバージョン
        central.extend_from_slice(&20u16.to_le_bytes()); // 展開に必要なバージョン
        central.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // 追加フィールド
        central.extend_from_slice(&0u16.to_le_bytes()); // コメント
        central.extend_from_slice(&0u16.to_le_bytes()); // 開始ディスク
        central.extend_from_slice(&0u16.to_le_bytes()); // 内部属性
        central.extend_from_slice(&0u32.to_le_bytes()); // 外部属性
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }

    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);

    // 終端レコード
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // このディスク
    out.extend_from_slice(&0u16.to_le_bytes()); // 中央ディレクトリのディスク
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // コメントなし
    Ok(out)
}

#[cfg(test)]
fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
#[cfg(test)]
fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// テスト用の読み出し: 終端レコードから中央ディレクトリをたどり、(名前, 中身) を返す。CRC も検査する。
#[cfg(test)]
pub(crate) fn read_zip(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let eocd = zip.len() - 22;
    assert_eq!(le32(zip, eocd), 0x0605_4b50, "end record");
    let count = le16(zip, eocd + 10) as usize;
    let mut pos = le32(zip, eocd + 16) as usize;
    let mut files = Vec::new();
    for _ in 0..count {
        assert_eq!(le32(zip, pos), 0x0201_4b50, "central record");
        assert_eq!(le16(zip, pos + 10), 0, "stored, not compressed");
        let crc = le32(zip, pos + 16);
        let size = le32(zip, pos + 24) as usize;
        let name_len = le16(zip, pos + 28) as usize;
        let local = le32(zip, pos + 42) as usize;
        let name = String::from_utf8(zip[pos + 46..pos + 46 + name_len].to_vec()).unwrap();
        // 局所ヘッダーの先の中身
        assert_eq!(le32(zip, local), 0x0403_4b50, "local header");
        let local_name_len = le16(zip, local + 26) as usize;
        let local_extra = le16(zip, local + 28) as usize;
        let start = local + 30 + local_name_len + local_extra;
        let data = zip[start..start + size].to_vec();
        assert_eq!(crc32(&data), crc, "crc of {name}");
        assert_eq!(le32(zip, local + 14), crc, "local header crc of {name}");
        files.push((name, data));
        pos += 46 + name_len;
    }
    assert_eq!(pos, eocd, "the central directory ends at the end record");
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_known_values() {
        // 標準の検査値
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn test_zip_round_trip_with_our_reader() {
        let entries = vec![
            ZipEntry {
                name: "frame_0001.png".into(),
                data: (0..=255u8).cycle().take(5000).collect(),
            },
            ZipEntry {
                name: "frame_0002.png".into(),
                data: vec![],
            },
            ZipEntry {
                name: "結.txt".into(),
                data: "こんにちは".as_bytes().to_vec(),
            },
        ];
        let zip = build_zip(&entries).unwrap();
        let files = read_zip(&zip);
        assert_eq!(files.len(), 3);
        for (e, (name, data)) in entries.iter().zip(&files) {
            assert_eq!((&e.name, &e.data), (name, data));
        }
        // 同じ入力は、同じバイト列（時刻に左右されない）
        assert_eq!(zip, build_zip(&entries).unwrap());
        // 空の zip は、終端レコードだけ
        assert_eq!(build_zip(&[]).unwrap().len(), 22);
    }

    #[test]
    fn test_zip_limits() {
        let one = |n: usize| {
            (0..n)
                .map(|i| ZipEntry {
                    name: format!("f{i}"),
                    data: vec![],
                })
                .collect::<Vec<_>>()
        };
        assert!(build_zip(&one(65_535)).is_ok());
        assert!(build_zip(&one(65_536)).unwrap_err().contains("at most"));
        let long = ZipEntry {
            name: "a".repeat(70_000),
            data: vec![],
        };
        assert!(build_zip(&[long]).unwrap_err().contains("too long"));
    }

    /// `ZIP_DUMP` に出力先を指定して実行すると、zip をファイルに書く（外部の zip ソフトで確かめるため）
    #[test]
    fn test_zip_can_be_dumped_for_external_tools() {
        let Ok(path) = std::env::var("ZIP_DUMP") else {
            return;
        };
        let entries: Vec<ZipEntry> = (1..=3)
            .map(|k| ZipEntry {
                name: format!("frame_{k:04}.png"),
                data: (0..=255u8).cycle().skip(k).take(1000 * k).collect(),
            })
            .collect();
        std::fs::write(path, build_zip(&entries).unwrap()).unwrap();
    }
}
