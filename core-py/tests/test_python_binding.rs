use pyo3::prelude::*;
use pyo3::types::PyModule;
use serial_test::serial;
use std::sync::OnceLock;

static INIT_YUI_MODULE: OnceLock<Result<(), String>> = OnceLock::new();

/// テスト実行時に一度だけ `yui` モジュールを初期化して `sys.modules` に登録する。
///
/// 【並行テスト実行とグローバル状態の分離について】
/// CPython インタプリタのモジュールテーブル（`sys.modules`）や型オブジェクトは、
/// プロセス全体で共有されるグローバル状態です。
/// 各テスト関数が並行（マルチスレッド）に `PyModule::new` で `sys.modules["yui"]` を
/// 再登録・上書きすると、テストスクリプト実行中にモジュールや型定義が差し替えられ、
/// レースコンディション（AssertionError等のテスト失敗）の原因となります。
/// そのため、初期化を1回のみに限定し、かつ各テスト関数に `#[serial]` 属性を付与して
/// 直列実行を保証しています。
///
/// NOTE: `std::sync::Once` ではなく `OnceLock<Result<(), String>>` を使うのは、
/// `Once::call_once` は2回目以降の呼び出しではクロージャを実行せず、初回の結果を
/// 呼び出し元へ伝播できない（呼び出しごとにローカル変数を`Ok(())`で初期化する実装だと、
/// 初回登録が実際に失敗していても以後は常に成功したかのように見えてしまう）ため。
/// `OnceLock::get_or_init` は初回に計算した結果を保持し、以後の呼び出し全てに
/// 同じ結果を返す。
fn setup_yui_module(py: Python<'_>) -> Result<(), String> {
    INIT_YUI_MODULE
        .get_or_init(|| {
            let setup = || -> PyResult<()> {
                let yui_mod = PyModule::new(py, "yui")?;
                yui::yui(&yui_mod)?;
                let sys = py.import("sys")?;
                let modules = sys.getattr("modules")?;
                modules.set_item("yui", yui_mod)?;
                Ok(())
            };
            setup().map_err(|e| e.to_string())
        })
        .clone()
}

#[test]
#[serial]
fn test_python_api_load_peptide_and_counts() {
    Python::attach(|py| {
        setup_yui_module(py).expect("Failed to setup yui module");

        let script = r#"
import yui

app = yui.AppState()
assert app.version == 2
assert app.atom_group is None

app.load_test_peptide()
ag = app.atom_group
assert ag is not None
assert ag.model_count == 1
assert ag.atom_count == 9

atoms, bonds = app.molecule_counts()
assert atoms == 9
# 結合数の正確な閾値・妥当性は proteindf-bridge 側の Bond::setup() アルゴリズムの
# 責務（YUI はそれを消費するのみ）。ここでは Python バインディング経由で
# 実結合が配線されていること（空でないこと）だけを検証する。
assert bonds > 0
"#;
        py.run(&std::ffi::CString::new(script).unwrap(), None, None)
            .expect("Script execution failed");
    });
}

#[test]
#[serial]
fn test_python_api_method_chaining_and_representations() {
    Python::attach(|py| {
        setup_yui_module(py).expect("Failed to setup yui module");

        let script = r#"
import yui

app = yui.AppState()
app.load_test_peptide()

# 1. Method chain with app.select(path_wildcard_str)
builder = app.select("/A/5").show("cartoon").color_by("chain")
assert len(app.representations) == 1
rep1 = app.representations[0]
assert rep1.kind == "cartoon"
assert rep1.color == "chain"
assert rep1.opacity == 1.0
assert rep1.visible is True

# 2. Add another representation with typed constructor: yui.symbol("C", "N", "O")
app.select(yui.symbol("C", "N", "O")).show("ball_and_stick").color_by("element").opacity(0.8).visible(True)
assert len(app.representations) == 2
rep2 = app.representations[1]
assert rep2.kind == "ballandstick"
assert rep2.color == "element"
assert abs(rep2.opacity - 0.8) < 1e-4

# 3. Add representation with typed constructor: yui.residue_name("GLY")
app.select(yui.residue_name("GLY")).show("surface").color_by("uniform", (1.0, 0.5, 0.0, 1.0))
assert len(app.representations) == 3
rep3 = app.representations[2]
assert rep3.kind == "surface"
assert "uniform" in rep3.color
"#;
        py.run(&std::ffi::CString::new(script).unwrap(), None, None)
            .expect("Script execution failed");
    });
}

#[test]
#[serial]
fn test_python_api_selector_typed_constructors_and_combinators() {
    Python::attach(|py| {
        setup_yui_module(py).expect("Failed to setup yui module");

        let script = r#"
import yui

app = yui.AppState()
app.load_test_peptide()
ag = app.atom_group

# 1. PathWildcard query: residue 5 (ALA) has 5 atoms (id 1..5)
s5 = yui.select("/A/5")
ids5 = s5.select_atom_ids(ag)
assert ids5 == [1, 2, 3, 4, 5]

# 2. Typed constructor: yui.residue_name("GLY") -> 4 atoms (id 6..9)
s_gly = yui.residue_name("GLY")
ids_gly = s_gly.select_atom_ids(ag)
assert ids_gly == [6, 7, 8, 9]

# 3. Typed constructor: yui.symbol("N") -> 2 atoms: 1, 6
s_n = yui.symbol("N")
ids_n = s_n.select_atom_ids(ag)
assert ids_n == [1, 6]

# 4. Typed constructor: yui.name("CA", "CB") -> 3 atoms (CA in ALA, CB in ALA, CA in GLY: ids 2, 5, 7)
s_cas = yui.name("CA", "CB")
ids_cas = s_cas.select_atom_ids(ag)
assert ids_cas == [2, 5, 7]

# 5. Typed constructor: yui.residue_range(5, 5, chain="A") -> 5 atoms (id 1..5)
s_range = yui.residue_range(5, 5, chain="A")
assert s_range.select_atom_ids(ag) == [1, 2, 3, 4, 5]

# 6. Typed constructor: yui.atom_ids(1, 3, 6) -> 3 atoms
s_atom_ids = yui.atom_ids(1, 3, 6)
assert s_atom_ids.select_atom_ids(ag) == [1, 3, 6]

# 7. Typed constructor: yui.path_regex("^/model_1/A/5/") -> 5 atoms
s_regex = yui.path_regex(r"^/model_1/A/5/")
assert s_regex.select_atom_ids(ag) == [1, 2, 3, 4, 5]

# 8. Combinators: and_(ALA, symbol:N) -> 1 atom (id 1)
s_ala_n = yui.and_(s5, s_n)
assert s_ala_n.select_atom_ids(ag) == [1]

# 9. Combinators: or_(ALA, GLY) -> all 9 atoms
s_all = yui.or_(s5, s_gly)
assert len(s_all.select_atom_ids(ag)) == 9

# 10. Combinators: not_(GLY) -> ALA (5 atoms)
s_not_gly = yui.not_(s_gly)
assert len(s_not_gly.select_atom_ids(ag)) == 5
"#;
        py.run(&std::ffi::CString::new(script).unwrap(), None, None)
            .expect("Script execution failed");
    });
}

#[test]
#[serial]
fn test_python_api_yaml_round_trip() {
    Python::attach(|py| {
        setup_yui_module(py).expect("Failed to setup yui module");

        let script = r#"
import yui

app1 = yui.AppState()
app1.load_test_peptide()
app1.select("/A/5").show("cartoon").color_by("chain")
app1.select("/A/6").show("ball_and_stick").color_by("element").opacity(0.5)
# not_ セレクタを含む表現を追加（以前は to_yaml() で nested enums エラーになっていた）
app1.select(yui.not_(yui.symbol("C"))).show("licorice").color_by("uniform", (0.0, 1.0, 0.0, 1.0))

# ラウンドトリップ前の結合数を記録（正確な値そのものではなく、
# インポート後も同じ値が保たれることを検証するための基準値）
_, bonds_before = app1.molecule_counts()
assert bonds_before > 0

# Export to YAML string
yaml_str = app1.to_yaml()
assert "cartoon" in yaml_str
assert "ball_and_stick" in yaml_str
assert "not" in yaml_str

# Import into fresh AppState
app2 = yui.AppState()
app2.from_yaml(yaml_str)
assert len(app2.representations) == 3
assert app2.representations[0].kind == "cartoon"
assert app2.representations[1].kind == "ballandstick"
assert abs(app2.representations[1].opacity - 0.5) < 1e-4
assert app2.representations[2].kind == "licorice"

# Atom group and molecule counts preserved
assert app2.atom_group is not None
assert app2.atom_group.atom_count == 9
atoms, bonds = app2.molecule_counts()
assert atoms == 9
# 結合数の具体的な値ではなく、YAMLラウンドトリップ前後で一致することを検証する
# （proteindf-bridge の Bond::setup() アルゴリズムそのものの正しさは
# YUI 側のテストの責務ではない）
assert bonds == bonds_before
assert "inner:" in yaml_str
"#;
        py.run(&std::ffi::CString::new(script).unwrap(), None, None)
            .expect("Script execution failed");
    });
}
