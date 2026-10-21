//! 「Share Session...」: `backend` の招待（Editor / Viewer の、期限つきの秘密）を、作る・一覧する・取り消す
//! ダイアログ（architecture.md 7.4節）。招待を作れるのは Host の秘密を持つアプリだけで、ほかのロールの秘密で
//! 開くと、`backend` が 403 を返し、その文が表示される。

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};
use serde::Deserialize;

/// 一覧に出る、有効な招待（秘密は含まない）
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InviteInfo {
    pub id: String,
    pub role: String,
    pub expires_in_secs: u64,
}

/// 作った招待（秘密は、このときだけ分かる）
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CreatedInvite {
    pub token: String,
    pub id: String,
    pub role: String,
    pub expires_in_secs: u64,
}

/// 招待のロール
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteRole {
    #[default]
    Viewer,
    Editor,
}

impl InviteRole {
    pub fn as_str(self) -> &'static str {
        match self {
            InviteRole::Viewer => "viewer",
            InviteRole::Editor => "editor",
        }
    }

    fn label(self) -> &'static str {
        match self {
            InviteRole::Viewer => "Viewer (read only)",
            InviteRole::Editor => "Editor",
        }
    }
}

/// 招待の有効な長さ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifetime {
    #[default]
    Hour,
    Day,
    Week,
}

impl Lifetime {
    pub fn secs(self) -> u64 {
        match self {
            Lifetime::Hour => 3600,
            Lifetime::Day => 24 * 3600,
            Lifetime::Week => 7 * 24 * 3600,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Lifetime::Hour => "1 hour",
            Lifetime::Day => "1 day",
            Lifetime::Week => "7 days",
        }
    }
}

/// ダイアログの状態
#[derive(Debug, Clone, Default)]
pub struct ShareState {
    pub role: InviteRole,
    pub lifetime: Lifetime,
    /// 要求の応答待ち
    pub busy: bool,
    /// いま作った招待（秘密を表示する）
    pub created: Option<CreatedInvite>,
    pub invites: Vec<InviteInfo>,
    /// 直近の失敗の文
    pub error: Option<String>,
}

/// ダイアログの操作と、その結果
#[derive(Debug, Clone)]
pub enum ShareAction {
    Close,
    SetRole(InviteRole),
    SetLifetime(Lifetime),
    Create,
    Created(Result<CreatedInvite, String>),
    Refresh,
    Listed(Result<Vec<InviteInfo>, String>),
    Revoke(String),
    Revoked(Result<String, String>),
    CopyLink,
    CopyToken,
}

impl ShareState {
    /// 要求を出さない操作の反映（結果の取り込み、選択の変更）。要求を出す操作は、アプリが `Task` にする。
    pub fn apply(&mut self, action: &ShareAction) {
        match action {
            ShareAction::SetRole(r) => self.role = *r,
            ShareAction::SetLifetime(l) => self.lifetime = *l,
            ShareAction::Create | ShareAction::Refresh | ShareAction::Revoke(_) => {
                self.busy = true;
                self.error = None;
            }
            ShareAction::Created(result) => {
                self.busy = false;
                match result {
                    Ok(created) => {
                        self.error = None;
                        self.created = Some(created.clone());
                    }
                    Err(e) => self.error = Some(e.clone()),
                }
            }
            ShareAction::Listed(result) => {
                self.busy = false;
                match result {
                    Ok(list) => self.invites = list.clone(),
                    Err(e) => self.error = Some(e.clone()),
                }
            }
            ShareAction::Revoked(result) => {
                self.busy = false;
                match result {
                    Ok(id) => {
                        self.invites.retain(|i| i.id != *id);
                        if self.created.as_ref().is_some_and(|c| c.id == *id) {
                            self.created = None;
                        }
                    }
                    Err(e) => self.error = Some(e.clone()),
                }
            }
            ShareAction::Close | ShareAction::CopyLink | ShareAction::CopyToken => {}
        }
    }
}

/// ブラウザで開くアドレス（`#` 以降は、サーバーに送られず、ログにも残らない）
pub fn invite_link(http_base: &str, token: &str) -> String {
    format!("{}/#token={token}", http_base.trim_end_matches('/'))
}

/// 残りの時間を、短く言う（「59 min」「23 h」「6 d」）
pub fn describe_remaining(secs: u64) -> String {
    match secs {
        0..=59 => "under a minute".to_string(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// 招待を作る（`POST /api/invites`）
pub async fn create_invite(
    http_base: String,
    role: InviteRole,
    lifetime: Lifetime,
) -> Result<CreatedInvite, String> {
    let body =
        serde_json::json!({ "role": role.as_str(), "ttl_secs": lifetime.secs() }).to_string();
    let text = crate::backend_auth::send_text(
        reqwest::Method::POST,
        &format!("{http_base}/api/invites"),
        Some(body),
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| format!("unexpected answer: {e}"))
}

/// 有効な招待の一覧（`GET /api/invites`）
pub async fn list_invites(http_base: String) -> Result<Vec<InviteInfo>, String> {
    let text = crate::backend_auth::send_text(
        reqwest::Method::GET,
        &format!("{http_base}/api/invites"),
        None,
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| format!("unexpected answer: {e}"))
}

/// 招待の取り消し（`DELETE /api/invites/{id}`）。成功したら、取り消した ID を返す。
pub async fn revoke_invite(http_base: String, id: String) -> Result<String, String> {
    crate::backend_auth::send_text(
        reqwest::Method::DELETE,
        &format!("{http_base}/api/invites/{id}"),
        None,
    )
    .await?;
    Ok(id)
}

/// ダイアログの表示
pub fn view_share_dialog<'a, Message: Clone + 'a>(
    state: &'a ShareState,
    http_base: &str,
    on_action: impl Fn(ShareAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let mut body = column![
        text("Share Session").size(14),
        text(
            "Give a collaborator an invitation. It has the chosen role only (it cannot open the terminal \
             or change the structure), expires by itself and can be revoked here."
        )
        .size(11)
        .width(Length::Fill),
    ]
    .spacing(10);

    let mut roles = row![text("Role").size(12).width(Length::Fixed(80.0))]
        .spacing(6)
        .align_y(Alignment::Center);
    for r in [InviteRole::Viewer, InviteRole::Editor] {
        roles = roles.push(crate::canvas::chip(
            r.label().to_string(),
            state.role == r,
            on_action(ShareAction::SetRole(r)),
        ));
    }
    body = body.push(roles);

    let mut lifetimes = row![text("Valid for").size(12).width(Length::Fixed(80.0))]
        .spacing(6)
        .align_y(Alignment::Center);
    for l in [Lifetime::Hour, Lifetime::Day, Lifetime::Week] {
        lifetimes = lifetimes.push(crate::canvas::chip(
            l.label().to_string(),
            state.lifetime == l,
            on_action(ShareAction::SetLifetime(l)),
        ));
    }
    body = body.push(lifetimes);

    body = body.push(
        button(text("Create invitation").size(12))
            .padding([4, 12])
            .style(crate::ui_style::accent_button)
            .on_press_maybe((!state.busy).then(|| on_action(ShareAction::Create))),
    );

    if let Some(created) = &state.created {
        let link = invite_link(http_base, &created.token);
        body = body.push(
            column![
                text(format!(
                    "Invitation {} ({}, valid for {}). The token is shown only now.",
                    created.id,
                    created.role,
                    describe_remaining(created.expires_in_secs)
                ))
                .size(12),
                text(link).size(11).width(Length::Fill),
                row![
                    button(text("Copy link").size(12))
                        .padding([3, 10])
                        .style(crate::ui_style::secondary_button)
                        .on_press(on_action(ShareAction::CopyLink)),
                    button(text("Copy token").size(12))
                        .padding([3, 10])
                        .style(crate::ui_style::secondary_button)
                        .on_press(on_action(ShareAction::CopyToken)),
                ]
                .spacing(6),
                text(
                    "Desktop: yui-frontend --collab <backend address>/api/collab --token <token>. \
                     Send it over a private channel."
                )
                .size(11)
                .width(Length::Fill),
            ]
            .spacing(6),
        );
    }

    if let Some(error) = &state.error {
        body = body.push(
            text(error.clone())
                .size(12)
                .style(iced::widget::text::danger),
        );
    }

    let mut list = column![
        row![
            text("Active invitations").size(12).width(Length::Fill),
            button(text("Refresh").size(12))
                .padding([2, 8])
                .style(crate::ui_style::secondary_button)
                .on_press_maybe((!state.busy).then(|| on_action(ShareAction::Refresh))),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(4);
    if state.invites.is_empty() {
        list = list.push(text("none").size(11));
    }
    for invite in &state.invites {
        list = list.push(
            row![
                text(format!(
                    "{} {} - {} left",
                    invite.role,
                    invite.id,
                    describe_remaining(invite.expires_in_secs)
                ))
                .size(11)
                .width(Length::Fill),
                button(text("Revoke").size(11))
                    .padding([2, 8])
                    .style(crate::ui_style::secondary_button)
                    .on_press_maybe(
                        (!state.busy).then(|| on_action(ShareAction::Revoke(invite.id.clone())))
                    ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    body = body.push(list);

    body = body.push(
        button(text("Close").size(12))
            .padding([4, 12])
            .style(crate::ui_style::accent_button)
            .on_press(on_action(ShareAction::Close)),
    );

    let card = container(body)
        .padding(16)
        .width(Length::Fixed(480.0))
        .style(container::rounded_box);
    container(card).center(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created() -> CreatedInvite {
        CreatedInvite {
            token: "t".repeat(64),
            id: "abc123".into(),
            role: "viewer".into(),
            expires_in_secs: 3600,
        }
    }

    #[test]
    fn test_link_and_remaining_time_text() {
        assert_eq!(
            invite_link("http://127.0.0.1:8080/", "tok"),
            "http://127.0.0.1:8080/#token=tok"
        );
        assert_eq!(describe_remaining(10), "under a minute");
        assert_eq!(describe_remaining(59 * 60 + 59), "59 min");
        assert_eq!(describe_remaining(3600), "1 h");
        assert_eq!(describe_remaining(23 * 3600 + 5), "23 h");
        assert_eq!(describe_remaining(6 * 86_400 + 5), "6 d");
        assert_eq!(Lifetime::Week.secs(), 604_800);
    }

    #[test]
    fn test_state_follows_the_actions_and_their_results() {
        let mut s = ShareState::default();
        assert_eq!((s.role, s.lifetime), (InviteRole::Viewer, Lifetime::Hour));
        s.apply(&ShareAction::SetRole(InviteRole::Editor));
        s.apply(&ShareAction::SetLifetime(Lifetime::Day));
        assert_eq!((s.role, s.lifetime), (InviteRole::Editor, Lifetime::Day));
        // 作る: 応答待ちになり、結果で、招待が出る
        s.apply(&ShareAction::Create);
        assert!(s.busy);
        s.apply(&ShareAction::Created(Ok(created())));
        assert!(!s.busy && s.created.as_ref().unwrap().id == "abc123" && s.error.is_none());
        // 失敗: 文が出て、いまの招待は、そのまま
        s.apply(&ShareAction::Create);
        s.apply(&ShareAction::Created(Err(
            "403 Forbidden: this action requires the Host role".into(),
        )));
        assert!(!s.busy && s.error.as_deref().unwrap().contains("403") && s.created.is_some());
        // 一覧と、取り消し（取り消したものが、いま作った招待なら、その表示も消す）
        s.apply(&ShareAction::Listed(Ok(vec![InviteInfo {
            id: "abc123".into(),
            role: "viewer".into(),
            expires_in_secs: 100,
        }])));
        assert_eq!(s.invites.len(), 1);
        s.apply(&ShareAction::Revoke("abc123".into()));
        assert!(s.busy);
        s.apply(&ShareAction::Revoked(Ok("abc123".into())));
        assert!(s.invites.is_empty() && s.created.is_none() && !s.busy);
        // 再度の操作で、前の失敗の文は消える
        s.apply(&ShareAction::Refresh);
        assert!(s.error.is_none());
    }

    #[test]
    fn test_answers_of_the_backend_are_read() {
        let made: CreatedInvite = serde_json::from_str(
            r#"{"token":"abc","id":"i1","role":"editor","expires_at":99,"expires_in_secs":600}"#,
        )
        .unwrap();
        assert_eq!(
            (made.id.as_str(), made.role.as_str(), made.expires_in_secs),
            ("i1", "editor", 600)
        );
        let list: Vec<InviteInfo> = serde_json::from_str(
            r#"[{"id":"i1","role":"viewer","expires_at":99,"expires_in_secs":5}]"#,
        )
        .unwrap();
        assert_eq!(list[0].expires_in_secs, 5);
    }
}
