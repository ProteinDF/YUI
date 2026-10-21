use clap::Parser;

/// 結 (YUI) ヘッドレスレンダラー（雛形）。
/// 現状はYAML状態ファイルを読み込み、内容を検証・要約表示するのみ。
/// 高解像度PNG/SVG/オフラインレンダリングは今後実装する（architecture.md 4章参照）。
#[derive(Parser, Debug)]
#[command(name = "yui-renderer-native", version)]
struct Args {
    /// 入力YAML状態ファイルへのパス
    input: String,
}

fn main() {
    let args = Args::parse();

    match yui_core::import_app_state_from_yaml(&args.input) {
        Ok(state) => {
            println!("Loaded: {}", args.input);
            println!("  atoms: {}", state.molecule.atoms.len());
            println!("  bonds: {}", state.molecule.bonds.len());
            println!("  annotations: {}", state.annotations.len());
        }
        Err(e) => {
            eprintln!("Failed to load '{}': {}", args.input, e);
            std::process::exit(1);
        }
    }
}
