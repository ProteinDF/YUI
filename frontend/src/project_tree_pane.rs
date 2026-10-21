use std::collections::{BTreeSet, HashMap};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Fill, Length, Task};

use crate::state_sync::derive_http_fs_list_url;

/// バックエンド `/api/fs/list` から返却されるエントリ構造 (Phase 6f-iii)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FsEntryItem {
    pub name: String,
    pub is_dir: bool,
}

/// プロジェクトツリーのノードデータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTreeNode {
    pub name: String,
    pub is_dir: bool,
    /// 未展開/未取得なら None。取得済みなら子ノードの相対パス一覧。
    pub children: Option<Vec<String>>,
}

/// プロジェクトツリーの動作モード
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TreeMode {
    /// 初期化中（初回バックエンド接続試行中）
    Uninitialized,
    /// バックエンドREST API経由で閲覧中（ネイティブ・WASM共通）
    Backend,
    /// ローカルファイルシステムを閲覧中（ネイティブのみ）
    LocalNative,
    /// スタンドアロン実行で利用不可（WASM）
    Unavailable,
}

/// プロジェクトツリーペインの状態
pub struct ProjectTreePaneState {
    pub mode: TreeMode,
    /// キー: ルートからの相対パス（`""` はルート自身）
    pub nodes: HashMap<String, ProjectTreeNode>,
    pub expanded: BTreeSet<String>,
    pub selected_path: Option<String>,
    pub error_message: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub local_root: Option<PathBuf>,
    pub base_url: Option<String>,
}

impl Default for ProjectTreePaneState {
    fn default() -> Self {
        Self {
            mode: TreeMode::Uninitialized,
            nodes: HashMap::new(),
            expanded: BTreeSet::new(),
            selected_path: None,
            error_message: None,
            #[cfg(not(target_arch = "wasm32"))]
            local_root: None,
            base_url: None,
        }
    }
}

/// プロジェクトツリーペインのメッセージ
#[derive(Debug, Clone)]
pub enum ProjectTreePaneMessage {
    /// 「Refresh」ボタン押下
    RefreshPressed,
    /// ルートディレクトリ一覧の初期取得完了
    InitLoaded(Result<Vec<FsEntryItem>, String>),
    /// ディレクトリノードの展開・折りたたみ切り替え
    ToggleExpanded(String),
    /// バックエンド経由の子階層読み込み完了
    ChildrenLoaded {
        path: String,
        result: Result<Vec<FsEntryItem>, String>,
    },
    /// ファイル選択イベント
    FileSelected(String),
    /// ファイルの右クリック（「Open with」で、開き方を選ぶ）
    FileOpenWithRequested(String),
    /// 「Open Folder」ボタン押下（ローカルモード用）
    OpenFolderPressed,
    /// フォルダ選択ダイアログ完了
    #[cfg(not(target_arch = "wasm32"))]
    FolderPicked(Option<PathBuf>),
    /// ローカルフォルダの子階層読み込み完了
    #[cfg(not(target_arch = "wasm32"))]
    LocalChildrenLoaded {
        path: String,
        result: Result<Vec<FsEntryItem>, String>,
    },
}

/// バックエンドの `/api/fs/list` からディレクトリ一覧を取得する非同期関数
pub async fn fetch_backend_fs_list(url: String) -> Result<Vec<FsEntryItem>, String> {
    let resp = crate::backend_auth::get(&url)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP status error: {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let entries = serde_json::from_slice::<Vec<FsEntryItem>>(&bytes)
        .map_err(|e| format!("Failed to parse JSON: {e}"))?;

    Ok(entries)
}

/// ローカルフォルダ選択ダイアログ（ネイティブ限定）
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder_dialog() -> Option<PathBuf> {
    let handle = rfd::AsyncFileDialog::new().pick_folder().await?;
    Some(handle.path().to_path_buf())
}

/// ローカルディレクトリを1階層のみ非同期列挙（ネイティブ限定）
#[cfg(not(target_arch = "wasm32"))]
pub async fn read_local_dir(full_path: PathBuf) -> Result<Vec<FsEntryItem>, String> {
    let mut read_dir = tokio::fs::read_dir(&full_path)
        .await
        .map_err(|e| format!("Failed to open directory: {e}"))?;

    let mut entries = Vec::new();
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            let name = entry.file_name().to_string_lossy().to_string();
            entries.push(FsEntryItem {
                name,
                is_dir: meta.is_dir(),
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

impl ProjectTreePaneState {
    /// 新規作成と初期フェッチタスクの発行
    pub fn new(base_url: Option<String>) -> (Self, Task<ProjectTreePaneMessage>) {
        let state = Self {
            base_url: base_url.clone(),
            ..Default::default()
        };

        let effective_base = base_url.unwrap_or_else(|| "ws://127.0.0.1:8080".to_string());
        let list_url = derive_http_fs_list_url(&effective_base, "");
        let task = Task::perform(
            fetch_backend_fs_list(list_url),
            ProjectTreePaneMessage::InitLoaded,
        );

        (state, task)
    }

    pub fn update(&mut self, message: ProjectTreePaneMessage) -> Task<ProjectTreePaneMessage> {
        match message {
            ProjectTreePaneMessage::RefreshPressed => {
                self.error_message = None;
                if self.mode == TreeMode::Backend || self.mode == TreeMode::Uninitialized {
                    let base = self
                        .base_url
                        .clone()
                        .unwrap_or_else(|| "ws://127.0.0.1:8080".to_string());
                    let list_url = derive_http_fs_list_url(&base, "");
                    Task::perform(
                        fetch_backend_fs_list(list_url),
                        ProjectTreePaneMessage::InitLoaded,
                    )
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(ref root) = self.local_root {
                        let full = root.clone();
                        return Task::perform(read_local_dir(full), |res| {
                            ProjectTreePaneMessage::LocalChildrenLoaded {
                                path: "".to_string(),
                                result: res,
                            }
                        });
                    }
                    Task::none()
                }
            }
            ProjectTreePaneMessage::InitLoaded(Ok(entries)) => {
                self.mode = TreeMode::Backend;
                self.error_message = None;
                self.populate_children("", entries);
                Task::none()
            }
            ProjectTreePaneMessage::InitLoaded(Err(err)) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // バックエンドなしの単独起動は通常の使い方なので、エラーとして出さない
                    // （ローカルのフォルダを開くボタンを案内する）。
                    let _ = err;
                    self.mode = TreeMode::LocalNative;
                    self.error_message = None;
                    Task::none()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.mode = TreeMode::Unavailable;
                    self.error_message = Some(format!("Backend unreachable: {err}"));
                    Task::none()
                }
            }
            ProjectTreePaneMessage::ToggleExpanded(path) => {
                if self.expanded.contains(&path) {
                    self.expanded.remove(&path);
                    Task::none()
                } else {
                    self.expanded.insert(path.clone());
                    // 子階層が未ロードなら非同期取得を発行（遅延読み込み）
                    let needs_load = self
                        .nodes
                        .get(&path)
                        .map(|n| n.children.is_none())
                        .unwrap_or(false);

                    if needs_load {
                        match self.mode {
                            TreeMode::Backend => {
                                let base = self
                                    .base_url
                                    .clone()
                                    .unwrap_or_else(|| "ws://127.0.0.1:8080".to_string());
                                let url = derive_http_fs_list_url(&base, &path);
                                let p = path.clone();
                                Task::perform(fetch_backend_fs_list(url), move |result| {
                                    ProjectTreePaneMessage::ChildrenLoaded { path: p, result }
                                })
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            TreeMode::LocalNative => {
                                if let Some(ref root) = self.local_root {
                                    let full = root.join(&path);
                                    let p = path.clone();
                                    Task::perform(read_local_dir(full), move |result| {
                                        ProjectTreePaneMessage::LocalChildrenLoaded {
                                            path: p,
                                            result,
                                        }
                                    })
                                } else {
                                    Task::none()
                                }
                            }
                            _ => Task::none(),
                        }
                    } else {
                        Task::none()
                    }
                }
            }
            ProjectTreePaneMessage::ChildrenLoaded { path, result } => {
                match result {
                    Ok(entries) => {
                        self.populate_children(&path, entries);
                    }
                    Err(err) => {
                        self.error_message = Some(format!("Failed to load '{path}': {err}"));
                    }
                }
                Task::none()
            }
            ProjectTreePaneMessage::FileSelected(path) => {
                self.selected_path = Some(path);
                Task::none()
            }
            ProjectTreePaneMessage::FileOpenWithRequested(path) => {
                self.selected_path = Some(path);
                Task::none()
            }
            ProjectTreePaneMessage::OpenFolderPressed => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(pick_folder_dialog(), ProjectTreePaneMessage::FolderPicked)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::none()
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            ProjectTreePaneMessage::FolderPicked(Some(path)) => {
                self.mode = TreeMode::LocalNative;
                self.local_root = Some(path.clone());
                self.nodes.clear();
                self.expanded.clear();
                self.selected_path = None;
                self.error_message = None;

                Task::perform(read_local_dir(path), |result| {
                    ProjectTreePaneMessage::LocalChildrenLoaded {
                        path: "".to_string(),
                        result,
                    }
                })
            }
            #[cfg(not(target_arch = "wasm32"))]
            ProjectTreePaneMessage::FolderPicked(None) => {
                // キャンセル
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            ProjectTreePaneMessage::LocalChildrenLoaded { path, result } => {
                match result {
                    Ok(entries) => {
                        self.populate_children(&path, entries);
                    }
                    Err(err) => {
                        self.error_message = Some(format!("Failed to load local '{path}': {err}"));
                    }
                }
                Task::none()
            }
        }
    }

    /// 指定パスの子要素を登録するヘルパー
    fn populate_children(&mut self, parent_path: &str, entries: Vec<FsEntryItem>) {
        let mut child_keys = Vec::new();

        for entry in entries {
            let child_rel_path = if parent_path.is_empty() {
                entry.name.clone()
            } else {
                format!("{parent_path}/{}", entry.name)
            };

            self.nodes
                .entry(child_rel_path.clone())
                .or_insert_with(|| ProjectTreeNode {
                    name: entry.name,
                    is_dir: entry.is_dir,
                    children: None,
                });

            child_keys.push(child_rel_path);
        }

        // 親ノードの children を更新
        if let Some(parent) = self.nodes.get_mut(parent_path) {
            parent.children = Some(child_keys);
        } else if parent_path.is_empty() {
            // ルートノードの初期化
            self.nodes.insert(
                "".to_string(),
                ProjectTreeNode {
                    name: "root".to_string(),
                    is_dir: true,
                    children: Some(child_keys),
                },
            );
        }
    }

    pub fn view(&self) -> Element<'_, ProjectTreePaneMessage> {
        let mode_label = match self.mode {
            TreeMode::Uninitialized => "Connecting...",
            TreeMode::Backend => "Backend FS",
            TreeMode::LocalNative => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if let Some(ref root) = self.local_root {
                        root.file_name()
                            .map(|s| s.to_str().unwrap_or("Local"))
                            .unwrap_or("Local")
                    } else {
                        "Local (No folder)"
                    }
                }
                #[cfg(target_arch = "wasm32")]
                "Local"
            }
            TreeMode::Unavailable => "Unavailable",
        };

        #[allow(unused_mut)]
        let mut toolbar = row![
            text(format!("[{mode_label}]")).size(11),
            button(text("Refresh").size(11))
                .padding([2, 6])
                .style(crate::ui_style::secondary_button)
                .on_press(ProjectTreePaneMessage::RefreshPressed),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);

        #[cfg(not(target_arch = "wasm32"))]
        if self.mode == TreeMode::LocalNative || self.mode == TreeMode::Uninitialized {
            toolbar = toolbar.push(
                button(text("Open Folder").size(11))
                    .padding([2, 6])
                    .style(crate::ui_style::secondary_button)
                    .on_press(ProjectTreePaneMessage::OpenFolderPressed),
            );
        }

        let content_elem: Element<'_, ProjectTreePaneMessage> = match self.mode {
            TreeMode::Unavailable => container(
                text("backendに接続できないため、WebAssembly環境ではプロジェクトツリーを利用できません。")
                    .size(12),
            )
            .padding(8)
            .into(),
            TreeMode::Uninitialized => {
                container(text("バックエンドに接続中...").size(12))
                    .padding(8)
                    .into()
            }
            TreeMode::LocalNative => {
                #[cfg(not(target_arch = "wasm32"))]
                if self.local_root.is_none() {
                    container(
                        column![
                            text("フォルダが開かれていません。").size(12),
                            button(text("フォルダを開く...").size(12))
                                .style(crate::ui_style::secondary_button).on_press(ProjectTreePaneMessage::OpenFolderPressed),
                        ]
                        .spacing(8),
                    )
                    .padding(8)
                    .into()
                } else {
                    self.render_tree_view()
                }
                #[cfg(target_arch = "wasm32")]
                container(text("利用不可").size(12)).into()
            }
            TreeMode::Backend => self.render_tree_view(),
        };

        let mut main_col = column![container(toolbar).padding(4).width(Fill)];
        if let Some(ref err) = self.error_message {
            main_col = main_col.push(container(text(err).size(11)).padding([0, 4]));
        }
        main_col = main_col.push(content_elem);

        main_col.spacing(2).width(Fill).height(Fill).into()
    }

    fn render_tree_view(&self) -> Element<'_, ProjectTreePaneMessage> {
        let mut rows = Vec::new();

        if let Some(root_node) = self.nodes.get("")
            && let Some(ref children) = root_node.children
        {
            for child_key in children {
                self.render_node_rows(child_key, 0, &mut rows);
            }
        }

        if rows.is_empty() {
            return container(text("(空のディレクトリ)").size(11))
                .padding(8)
                .into();
        }

        let col = column(rows).spacing(1).width(Fill);
        scrollable(col).width(Fill).height(Fill).into()
    }

    fn render_node_rows<'a>(
        &'a self,
        path: &'a str,
        depth: usize,
        rows: &mut Vec<Element<'a, ProjectTreePaneMessage>>,
    ) {
        let Some(node) = self.nodes.get(path) else {
            return;
        };

        let is_expanded = self.expanded.contains(path);
        let is_selected = self.selected_path.as_deref() == Some(path);

        let indent_width = (depth * 14) as f32;
        let indent_spacer = container(text("").size(11)).width(Length::Fixed(indent_width));

        let row_content: Element<'_, ProjectTreePaneMessage> = if node.is_dir {
            let toggle_icon = if is_expanded { "▼" } else { "▶" };
            let toggle_btn = button(text(toggle_icon).size(10))
                .padding([1, 4])
                .style(crate::ui_style::flat_button)
                .on_press(ProjectTreePaneMessage::ToggleExpanded(path.to_string()));

            let name_label = text(format!("{}/", node.name)).size(12);
            let name_btn = button(name_label)
                .padding([1, 4])
                .style(crate::ui_style::flat_button)
                .on_press(ProjectTreePaneMessage::ToggleExpanded(path.to_string()));

            row![indent_spacer, toggle_btn, name_btn]
                .spacing(2)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            let spacer = container(text("").size(10)).width(Length::Fixed(14.0));
            let file_label = text(node.name.clone()).size(12);

            let mut file_btn = button(file_label)
                .padding([1, 4])
                .style(crate::ui_style::flat_button)
                .on_press(ProjectTreePaneMessage::FileSelected(path.to_string()));

            if is_selected {
                file_btn = file_btn.style(crate::ui_style::selected_flat_button);
            }

            let file_row = row![indent_spacer, spacer, file_btn]
                .spacing(2)
                .align_y(iced::Alignment::Center);
            iced::widget::mouse_area(file_row)
                .on_right_press(ProjectTreePaneMessage::FileOpenWithRequested(
                    path.to_string(),
                ))
                .into()
        };

        rows.push(row_content);

        // 展開されている場合のみ子要素を再帰的に走査
        if is_expanded && node.is_dir {
            if let Some(ref children) = node.children {
                for child_key in children {
                    self.render_node_rows(child_key, depth + 1, rows);
                }
            } else {
                let loading_indent = ((depth + 1) * 14) as f32;
                rows.push(
                    row![
                        container(text("").size(11)).width(Length::Fixed(loading_indent)),
                        text("読み込み中...").size(11),
                    ]
                    .spacing(2)
                    .into(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_tree_initial_state() {
        let state = ProjectTreePaneState::default();
        assert_eq!(state.mode, TreeMode::Uninitialized);
        assert!(state.nodes.is_empty());
        assert!(state.expanded.is_empty());
        assert_eq!(state.selected_path, None);
        assert_eq!(state.error_message, None);
    }

    #[test]
    fn test_init_loaded_success_populates_root() {
        let mut state = ProjectTreePaneState::default();
        let entries = vec![
            FsEntryItem {
                name: "file.txt".to_string(),
                is_dir: false,
            },
            FsEntryItem {
                name: "folder".to_string(),
                is_dir: true,
            },
        ];

        let _ = state.update(ProjectTreePaneMessage::InitLoaded(Ok(entries)));

        assert_eq!(state.mode, TreeMode::Backend);
        assert_eq!(state.error_message, None);

        let root = state.nodes.get("").expect("Root node should exist");
        assert_eq!(
            root.children,
            Some(vec!["file.txt".to_string(), "folder".to_string()])
        );

        let file = state.nodes.get("file.txt").unwrap();
        assert_eq!(file.name, "file.txt");
        assert!(!file.is_dir);

        let folder = state.nodes.get("folder").unwrap();
        assert_eq!(folder.name, "folder");
        assert!(folder.is_dir);
        assert_eq!(folder.children, None);
    }

    #[test]
    fn test_init_loaded_failure_fallback() {
        let mut state = ProjectTreePaneState::default();
        let _ = state.update(ProjectTreePaneMessage::InitLoaded(Err(
            "Connection refused".to_string(),
        )));

        // 単独起動（ネイティブ）はエラーにしない。ブラウザではバックエンドが必須なので出す。
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert_eq!(state.mode, TreeMode::LocalNative);
            assert!(state.error_message.is_none());
        }

        #[cfg(target_arch = "wasm32")]
        {
            assert_eq!(state.mode, TreeMode::Unavailable);
            assert!(state.error_message.is_some());
        }
    }

    #[test]
    fn test_toggle_expanded_and_populate_children() {
        let mut state = ProjectTreePaneState::default();
        let entries = vec![FsEntryItem {
            name: "sub".to_string(),
            is_dir: true,
        }];
        let _ = state.update(ProjectTreePaneMessage::InitLoaded(Ok(entries)));

        // 展開切り替え
        let _ = state.update(ProjectTreePaneMessage::ToggleExpanded("sub".to_string()));
        assert!(state.expanded.contains("sub"));

        // 子要素読み込み完了
        let sub_entries = vec![FsEntryItem {
            name: "nested.txt".to_string(),
            is_dir: false,
        }];
        let _ = state.update(ProjectTreePaneMessage::ChildrenLoaded {
            path: "sub".to_string(),
            result: Ok(sub_entries),
        });

        let sub_node = state.nodes.get("sub").unwrap();
        assert_eq!(sub_node.children, Some(vec!["sub/nested.txt".to_string()]));

        let nested = state.nodes.get("sub/nested.txt").unwrap();
        assert_eq!(nested.name, "nested.txt");
        assert!(!nested.is_dir);

        // 再度 Toggle で折りたたみ
        let _ = state.update(ProjectTreePaneMessage::ToggleExpanded("sub".to_string()));
        assert!(!state.expanded.contains("sub"));
    }

    #[test]
    fn test_file_selection() {
        let mut state = ProjectTreePaneState::default();
        let _ = state.update(ProjectTreePaneMessage::FileSelected("doc.md".to_string()));
        assert_eq!(state.selected_path.as_deref(), Some("doc.md"));
    }
}
