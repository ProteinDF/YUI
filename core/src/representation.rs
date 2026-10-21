use crate::selector::Selector;
use crate::state::Color4;
use serde::{Deserialize, Serialize};

pub use proteindf_bridge::InteractionKind;

/// レンダリング表現の種別（RepresentationKind）
///
/// `architecture.md` 6.2.2節および10.2節に準拠。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationKind {
    BallAndStick,
    Cartoon,
    Surface,
    Licorice,
    Cpk,
    Line,
    Interaction,
}

/// 配色方式（ColorScheme）
///
/// NOTE: `architecture.md` 6.2節に準拠し、初期実装スコープは
/// `Element | Chain | Residue | SecondaryStructure | Uniform` のみに限定しています。
/// Mulliken電荷やB-factor等のスカラー値駆動による配色は本バリアントには含めず、
/// 第12章で実装する `ScalarOverlay`（12.2節）として基本色の上に重畳する設計とします。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    Element,
    Chain,
    Residue,
    SecondaryStructure,
    Uniform(Color4),
}

/// 表現層の構造化データ型（Representation）
///
/// `architecture.md` 6.2.2節および10.2節に準拠。
/// `AppState` 内にリストとして保持され、表示の重なりは後勝ち（PyMOL方式）で解決されます。
///
/// 【フィールド追加時の規則（後方互換性維持）】
/// `Representation` に新しいフィールドを追加する際は、**必ず構造体の末尾に配置し、`#[serde(default)]` を付与**してください。
/// MessagePack の配列形式でシリアライズされた旧データ（旧バージョン）との互換性を保つためです。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Representation {
    pub id: String,
    pub selection: Selector,
    pub kind: RepresentationKind,
    pub color: ColorScheme,
    pub opacity: f32,
    pub visible: bool,
    /// 表示する相互作用の種類の絞り込み（kind が Interaction のとき有効）。
    /// 空のときは全種類を表示する。
    #[serde(default)]
    pub interaction_kinds: Vec<InteractionKind>,
    /// 「隠す」カードか。`true` のカードは何も描かず、表示中（`visible`）のあいだ、選んだ原子を
    /// すべての表現から隠す（ツリーや右クリックの「Hide」が作る）。目のアイコンで隠すのを解除できる。
    /// `false` のカードの `visible` は、そのカード自身を描くかどうかだけを決める。
    #[serde(default)]
    pub hides_atoms: bool,
}

impl Representation {
    pub fn new(
        id: impl Into<String>,
        selection: Selector,
        kind: RepresentationKind,
        color: ColorScheme,
    ) -> Self {
        Self {
            id: id.into(),
            selection,
            kind,
            color,
            opacity: 1.0,
            visible: true,
            interaction_kinds: Vec::new(),
            hides_atoms: false,
        }
    }

    /// 「隠す」カードにする（`hides_atoms` を参照）
    pub fn with_hides_atoms(mut self, hides_atoms: bool) -> Self {
        self.hides_atoms = hides_atoms;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_interaction_kinds(mut self, kinds: Vec<InteractionKind>) -> Self {
        self.interaction_kinds = kinds;
        self
    }
}

impl Default for Representation {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            selection: Selector::All,
            kind: RepresentationKind::BallAndStick,
            color: ColorScheme::Element,
            opacity: 1.0,
            visible: true,
            interaction_kinds: Vec::new(),
            hides_atoms: false,
        }
    }
}

/// 構造を読み込んだときに、表示設定が1つもなければ追加する既定の表示設定。
///
/// 二次構造で色分けしたカートゥーンと、元素で色分けした球棒の2つ。
/// `architecture.md` 6.2節の「構造を読み込んだときの既定の表示設定」を参照。
pub fn default_representations() -> Vec<Representation> {
    vec![
        Representation::new(
            "cartoon_default",
            Selector::All,
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        ),
        Representation::new(
            "ball_and_stick_default",
            Selector::All,
            RepresentationKind::BallAndStick,
            ColorScheme::Element,
        ),
    ]
}

/// 表示設定が1つもなければ、既定の表示設定を追加する。
///
/// 利用者が作った表示設定や、ワークスペースから読み込んだ表示設定がある場合は何もしない。
/// 追加したときは `true` を返す。
pub fn ensure_default_representations(representations: &mut Vec<Representation>) -> bool {
    if representations.is_empty() {
        *representations = default_representations();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_representation_construction_and_defaults() {
        let rep = Representation::new(
            "rep_cartoon_a",
            Selector::PathWildcard("/A/*".to_string()),
            RepresentationKind::Cartoon,
            ColorScheme::SecondaryStructure,
        );

        assert_eq!(rep.id, "rep_cartoon_a");
        assert_eq!(rep.kind, RepresentationKind::Cartoon);
        assert_eq!(rep.color, ColorScheme::SecondaryStructure);
        assert_eq!(rep.opacity, 1.0);
        assert!(rep.visible);
        assert!(rep.interaction_kinds.is_empty());
    }

    #[test]
    fn test_representation_interaction_roundtrip_and_backward_compatibility() {
        // 1. 旧形式のYAML（interaction_kinds なし）からのデシリアライズ
        let legacy_yaml = r#"
id: rep_legacy
selection: all
kind: interaction
color: !uniform
  - 1.0
  - 0.8
  - 0.0
  - 1.0
opacity: 0.9
visible: true
"#;
        let rep_from_legacy: Representation =
            serde_yaml::from_str(legacy_yaml).expect("legacy YAML should deserialize");
        assert_eq!(rep_from_legacy.id, "rep_legacy");
        assert_eq!(rep_from_legacy.kind, RepresentationKind::Interaction);
        assert_eq!(
            rep_from_legacy.color,
            ColorScheme::Uniform([1.0, 0.8, 0.0, 1.0])
        );
        assert_eq!(rep_from_legacy.opacity, 0.9);
        assert!(rep_from_legacy.visible);
        assert!(
            rep_from_legacy.interaction_kinds.is_empty(),
            "default for missing interaction_kinds should be empty"
        );

        // 2. interaction_kinds を設定した Representation の YAML 往復
        let rep_with_kinds = Representation::new(
            "rep_inter",
            Selector::All,
            RepresentationKind::Interaction,
            ColorScheme::Uniform([1.0, 1.0, 0.0, 1.0]),
        )
        .with_interaction_kinds(vec![
            InteractionKind::HydrogenBond,
            InteractionKind::SaltBridge,
        ]);

        let yaml_str =
            serde_yaml::to_string(&rep_with_kinds).expect("YAML serialization should succeed");
        let rep_yaml_deserialized: Representation =
            serde_yaml::from_str(&yaml_str).expect("YAML deserialization should succeed");
        assert_eq!(rep_with_kinds, rep_yaml_deserialized);

        // 3. MessagePack (rmp_serde) 往復
        let msgpack_bytes =
            rmp_serde::to_vec_named(&rep_with_kinds).expect("MessagePack serialization");
        let rep_mp_deserialized: Representation =
            rmp_serde::from_slice(&msgpack_bytes).expect("MessagePack deserialization");
        assert_eq!(rep_with_kinds, rep_mp_deserialized);
    }

    #[test]
    fn test_ensure_default_representations_only_when_empty() {
        let mut reps = Vec::new();
        assert!(ensure_default_representations(&mut reps));
        assert_eq!(reps, default_representations());
        assert_eq!(reps[0].kind, RepresentationKind::Cartoon);
        assert_eq!(reps[0].color, ColorScheme::SecondaryStructure);
        assert_eq!(reps[1].kind, RepresentationKind::BallAndStick);
        assert_eq!(reps[1].color, ColorScheme::Element);

        let user = Representation::new(
            "user",
            Selector::All,
            RepresentationKind::Cpk,
            ColorScheme::Chain,
        );
        let mut reps = vec![user.clone()];
        assert!(!ensure_default_representations(&mut reps));
        assert_eq!(reps, vec![user]);
    }
}
