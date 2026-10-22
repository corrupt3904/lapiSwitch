use super::fluent::{self, Severity, glyph, icon};
use super::*;
use iced::widget::{
    self, button, checkbox, column, container, opaque, progress_bar, row, scrollable, space, stack,
    text,
};
use iced::{Alignment, Background, Border, Color, Fill};
use lapiswitch_core::state::Request;
/// Palette shortcuts; every color follows the active light or dark theme.
pub(super) fn ink() -> Color {
    fluent::p().text
}
pub(super) fn muted() -> Color {
    fluent::p().text_secondary
}
pub(super) fn accent() -> Color {
    fluent::p().accent
}
pub(super) fn pale() -> Color {
    fluent::p().accent_soft
}
pub(super) fn line() -> Color {
    fluent::p().card_border
}
pub(super) fn danger() -> Color {
    fluent::p().danger
}
pub(super) fn card_color() -> Color {
    fluent::p().card
}
const FIELD_KEY: f32 = 200.0;
/// Four-sided padding: top, right, bottom, left.
pub(super) fn pad(top: u16, right: u16, bottom: u16, left: u16) -> iced::Padding {
    iced::Padding {
        top: top.into(),
        right: right.into(),
        bottom: bottom.into(),
        left: left.into(),
    }
}
/// A small section heading above a group of cards.
pub(super) fn section<'a>(value: impl Into<String>) -> Element<'a, Message> {
    container(label(value).size(14))
        .padding(pad(12, 0, 2, 2))
        .into()
}
/// Standard (secondary) button.
pub(super) fn soft_style(theme: &Theme, status: button::Status) -> button::Style {
    fluent::standard_style(theme, status)
}
pub(super) fn label<'a>(value: impl Into<String>) -> widget::Text<'a> {
    text(transcript::readable_paths(&value.into()))
        .shaping(widget::text::Shaping::Advanced)
        .color(ink())
}
pub(super) fn small<'a>(value: impl Into<String>) -> widget::Text<'a> {
    label(value).size(13).color(muted())
}
/// Card surface; dialogs use it too.
pub(super) fn panel<'a>(
    content: impl Into<Element<'a, Message>>,
) -> widget::Container<'a, Message> {
    container(content)
        .padding(24)
        .width(Fill)
        .style(fluent::card_style)
}
/// Fluent InfoBar: icon, message and a severity tint.
pub(super) fn infobar<'a>(severity: Severity, value: impl Into<String>) -> Element<'a, Message> {
    container(
        row![
            icon(severity.glyph()).color(severity.icon_color()),
            label(value).size(14).width(Fill)
        ]
        .spacing(12),
    )
    .padding([12, 16])
    .width(Fill)
    .style(severity.style())
    .into()
}
pub(super) fn note<'a>(value: impl Into<String>, warning: bool) -> Element<'a, Message> {
    infobar(
        if warning {
            Severity::Warning
        } else {
            Severity::Info
        },
        value,
    )
}
/// A settings-card row: name on the left, value on the right.
pub(super) fn field<'a>(key: impl Into<String>, value: impl Into<String>) -> Element<'a, Message> {
    container(
        row![
            label(key).size(14).width(FIELD_KEY),
            label(value).size(14).color(muted()).width(Fill)
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([14, 16])
    .width(Fill)
    .style(fluent::card_style)
    .into()
}
fn error_note<'a>(error: &str) -> Element<'a, Message> {
    warning_note(error.to_owned())
}
fn warning_note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    infobar(Severity::Error, value)
}
pub(super) fn notification<'a>(
    content: impl Into<Element<'a, Message>>,
) -> widget::Container<'a, Message> {
    panel(content).padding([12, 16])
}
pub(super) fn notification_note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    infobar(Severity::Info, value)
}
pub(super) fn recovery_style(theme: &Theme, status: button::Status) -> button::Style {
    fluent::danger_outline_style(theme, status)
}
pub(super) fn pill_style(theme: &Theme) -> container::Style {
    fluent::pill_style(theme)
}
/// The accent button; secondary actions override the style.
pub(super) fn action<'a>(
    title: &'static str,
    message: Message,
    enabled: bool,
) -> widget::Button<'a, Message> {
    button(text(title).shaping(widget::text::Shaping::Advanced))
        .padding([8, 20])
        .style(fluent::accent_style)
        .on_press_maybe(enabled.then_some(message))
}
/// Page header: Fluent title and a one-line description.
pub(super) fn title<'a>(
    app: &App,
    ko: &'static str,
    en: &'static str,
    sub_ko: &'static str,
    sub_en: &'static str,
) -> Element<'a, Message> {
    column![
        label(app.t(ko, en)).size(28),
        label(app.t(sub_ko, sub_en)).size(14).color(muted())
    ]
    .spacing(4)
    .into()
}
/// Which navigation item the current page belongs to.
fn nav_section(app: &App) -> usize {
    match app.page {
        Page::Dashboard | Page::Resume => 0,
        Page::Edit => 1,
        Page::Region | Page::Firmware | Page::Options | Page::Confirm => 2,
        Page::Recovery => 3,
        Page::Settings => 4,
        Page::Progress | Page::Result => match app.run_kind {
            RunKind::Edit => 1,
            RunKind::Convert => 2,
            RunKind::Restore => 3,
            RunKind::Resume => 0,
        },
    }
}
/// Windows 11 settings-style navigation: app title, the connected device
/// and one item per task. Locked while an operation runs.
fn navigation(app: &App) -> Element<'_, Message> {
    let current = nav_section(app);
    let idle = !app.busy();
    let supported = !app.device.unsupported_model();
    let item = |index: usize, glyph: char, name: &'static str, message: Message, enabled: bool| {
        let selected = current == index;
        let marker = container(space().width(3).height(16)).style(move |_| container::Style {
            background: selected.then(|| fluent::p().accent.into()),
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        button(
            row![
                marker,
                icon(glyph),
                text(name).size(14).shaping(widget::text::Shaping::Advanced)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([9, 6])
        .on_press_maybe(enabled.then_some(message))
        .style(move |theme, status| {
            let mut style = fluent::subtle_style(theme, status);
            if selected {
                style.background = Some(fluent::p().subtle_selected.into());
                style.text_color = fluent::p().text;
            }
            style
        })
    };
    let mut brand = row![
        widget::image(crate::brand::image()).width(20).height(20),
        label(crate::brand::NAME).size(13)
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if app.demo {
        brand = brand.push(space().width(Fill)).push(
            container(label("DEMO").size(11).color(accent()))
                .padding([2, 8])
                .style(pill_style),
        );
    }
    column![
        container(brand).padding([0, 6]),
        container(dashboard_view::device_card(app)).padding(pad(14, 0, 10, 0)),
        item(0, glyph::HOME, app.t("기기", "Device"), Message::Home, idle),
        item(
            1,
            glyph::EDIT,
            app.t("DeviceInfo 수정", "Edit DeviceInfo"),
            Message::Edit,
            idle && supported
        ),
        item(
            2,
            glyph::GLOBE,
            app.t("리전 변환", "Region conversion"),
            Message::Convert,
            idle && supported
        ),
        item(
            3,
            glyph::HISTORY,
            app.t("원본 복구", "Restore original"),
            Message::Recovery,
            idle
        ),
        space().height(Fill),
        item(
            4,
            glyph::SETTINGS,
            app.t("설정", "Settings"),
            Message::Settings,
            idle
        ),
        container(
            column![
                small(super::supported_models()).size(12),
                small(format!("v{} · GPL-3.0", env!("CARGO_PKG_VERSION"))).size(12)
            ]
            .spacing(2)
        )
        .padding(pad(8, 6, 0, 6)),
    ]
    .spacing(4)
    .width(290)
    .height(Fill)
    .padding([20, 12])
    .into()
}
/// Region conversion steps as a Fluent breadcrumb above the page.
fn stepper(app: &App) -> Element<'_, Message> {
    let steps = [
        app.t("리전 선택", "Target region"),
        app.t("펌웨어 검사", "Firmware"),
        app.t("변환 설정", "Options"),
        app.t("최종 확인", "Review"),
        app.t("진행 / 결과", "Progress / result"),
    ];
    let page = if app.page == Page::Settings {
        app.return_page
    } else {
        app.page
    };
    let active = match page {
        Page::Region => 0,
        Page::Firmware => 1,
        Page::Options => 2,
        Page::Confirm => 3,
        _ => 4,
    };
    let mut crumbs = row![].spacing(8).align_y(Alignment::Center);
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            crumbs = crumbs.push(icon(glyph::CHEVRON).size(10).color(muted()));
        }
        let (color, size) = if i == active {
            (ink(), 14)
        } else if i < active {
            (accent(), 13)
        } else {
            (muted(), 13)
        };
        crumbs = crumbs.push(label(format!("{} {step}", i + 1)).size(size).color(color));
    }
    crumbs.into()
}
pub(super) fn view(app: &App) -> Element<'_, Message> {
    fluent::set_dark(app.dark());
    let mut body = column![].spacing(12);
    if app.conversion_steps_visible() {
        body = body.push(stepper(app));
    }
    if app.demo {
        body = body.push(notification_note(app.t(
            "미리보기 · 기기에 연결하거나 파일을 변경하지 않습니다.",
            "Preview · No device connections or file changes.",
        )));
    }
    if let Some(banner) = dashboard_view::driver_banner(app) {
        body = body.push(banner);
    }
    if let Some(banner) = conflicts::banner(app) {
        body = body.push(banner);
    }
    if let Some(pending) = &app.pending {
        body = body.push(notification(
            row![
                column![
                    label(app.t("이어갈 작업이 있습니다", "An interrupted run was found")),
                    small(format!(
                        "{} · {:?}",
                        request_label(app, &pending.state),
                        pending.state.stage
                    ))
                ]
                .spacing(3)
                .width(Fill),
                action(
                    app.t("작업 확인", "Review run"),
                    Message::Resume,
                    !app.busy()
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ));
    }
    if let Some(error) = &app.startup_error {
        body = body.push(notification_note(format!(
            "{}\n{error}",
            app.t(
                "저장된 작업 확인이 필요합니다. 새 변환은 시작할 수 없습니다.",
                "Saved state needs attention before a new conversion."
            )
        )));
    }
    if let Some(error) = &app.error {
        body = body.push(warning_note(error.clone()));
    }
    if let Some(notice) = &app.notice {
        body = body.push(notification_note(notice.clone()));
    }
    let (content, footer) = match app.page {
        Page::Dashboard => dashboard_view::dashboard(app),
        Page::Edit => edit(app),
        Page::Region => region(app),
        Page::Firmware => firmware(app),
        Page::Options => options(app),
        Page::Confirm => confirm(app),
        Page::Progress => progress(app),
        Page::Result => result(app),
        Page::Recovery => recovery(app),
        Page::Settings => super::settings_view::settings(app),
        Page::Resume => resume(app),
    };
    body = body
        .push(
            scrollable(
                container(content)
                    .max_width(1000)
                    .padding(pad(4, 18, 24, 0)),
            )
            .direction(fluent::thin_scrollbar())
            .style(fluent::scroll_style)
            .width(Fill)
            .height(Fill),
        )
        .push(container(footer).max_width(1000).padding(pad(0, 18, 0, 0)));
    let root: Element<'_, Message> = container(
        row![
            navigation(app),
            container(body)
                .padding(pad(28, 24, 20, 12))
                .width(Fill)
                .height(Fill)
        ]
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(fluent::p().bg.into()),
        text_color: Some(ink()),
        ..Default::default()
    })
    .into();
    let modal = if app.close_warning {
        Some(panel(column![
            label(app.t("진행 중인 작업을 종료할까요?", "Close the running operation?")).size(24),
            label(if app.activity == Some(Activity::Install) { app.t("드라이버 설치 프로그램이 실행 중입니다. 설치가 끝날 때까지 기다려 주세요.", "The driver installer is running. Please wait for it to finish.") } else { app.t("쓰기 도중 종료하면 기기가 부팅되지 않을 수 있습니다. 저장된 단계와 백업으로 이어하기 또는 복구가 필요할 수 있습니다.", "Closing during writes can leave the device unable to boot. Resume or restore may be required using the saved state and backup.") }),
            row![space().width(Fill), action(app.t("계속 진행", "Keep running"), Message::Stay, true), action(app.t("앱 종료", "Close app"), Message::Exit, true).style(super::fluent::danger_style)].spacing(12),
        ].spacing(20)).max_width(560))
    } else if app.firmware_confirmation.is_some() {
        Some(panel(column![
            label(app.t("순정롬 확인 불가", "Stock firmware could not be verified")).size(25).color(danger()),
            label(app.t(
                "기기의 펌웨어가 순정롬 버전 검사를 통과하지 못했습니다. 커스텀롬, GSI 또는 테스트 빌드일 가능성이 높습니다. 계속 작업을 진행하시겠습니까?",
                "The device firmware could not pass the stock firmware version check. It may be a custom ROM, GSI or test build. Do you want to continue?"
            )).color(danger()),
            row![
                space().width(Fill),
                action(app.t("취소", "Cancel"), Message::ConfirmUnrecognizedFirmware(false), true).style(soft_style),
                action(app.t("계속 진행", "Continue"), Message::ConfirmUnrecognizedFirmware(true), true),
            ].spacing(12),
        ].spacing(20)).style(fluent::warning_dialog_style).max_width(620))
    } else if let Some((details, _)) = &app.rollback_confirmation {
        Some(panel(column![
            label(app.t("롤백인덱스 경고", "Rollback index warning")).size(25).color(danger()),
            label(app.t(
                "설치할 펌웨어의 롤백인덱스가 기기 DeviceInfo에 저장된 값보다 낮습니다. 이대로 설치하면 롤백 방지로 부팅되지 않습니다.",
                "The firmware's rollback indices are lower than those stored in the device's DeviceInfo. Installed as is, rollback protection would stop it from booting."
            )).color(danger()),
            small(details.clone()),
            label(app.t(
                "0으로 초기화 후 계속: DeviceInfo의 32개 롤백인덱스를 0으로 기록하고 작업을 계속합니다.",
                "Reset to 0 and continue: writes all 32 DeviceInfo rollback indices as 0 and continues."
            )),
            label(app.t(
                "작업 취소: DeviceInfo를 바꾸지 않고 EDL로 전환해 원본 GPT·Recovery·vendor_boot를 자동으로 복구합니다.",
                "Cancel: leaves DeviceInfo unchanged, switches to EDL and automatically restores the original GPT, Recovery and vendor_boot."
            )),
            row![
                space().width(Fill),
                action(app.t("작업 취소 · 원본 복구", "Cancel · Restore originals"), Message::ConfirmRollbackReset(false), true).style(soft_style),
                action(app.t("0으로 초기화 후 계속", "Reset to 0 and continue"), Message::ConfirmRollbackReset(true), true).style(super::fluent::danger_style),
            ].spacing(12),
        ].spacing(20)).style(fluent::warning_dialog_style).max_width(660))
    } else if let Some(editor) = &app.editor {
        // The country picker opens over the editor for a region change.
        Some(if app.country_open {
            country_view::modal(app)
        } else {
            super::editor::modal(app, editor)
        })
    } else if app.edl_confirmation.is_some() {
        Some(
            panel(
                column![
                    label(app.t("EDL에서 작업 시작", "Start from EDL")).size(25),
                    label(app.t(
                        "기기가 정상 부팅이 가능한 상태일 때만 작업이 가능합니다.",
                        "The device must be able to boot normally before starting this operation."
                    )),
                    label(app.t(
                        "만약 정상 부팅이 가능하다면 확인, 불가능하다면 취소를 눌러주세요.",
                        "Select Confirm if normal boot is possible; otherwise select Cancel."
                    )),
                    row![
                        space().width(Fill),
                        action(
                            app.t("취소", "Cancel"),
                            Message::ConfirmEdlStart(false),
                            true
                        )
                        .style(soft_style),
                        action(
                            app.t("확인", "Confirm"),
                            Message::ConfirmEdlStart(true),
                            true
                        )
                    ]
                    .spacing(12),
                ]
                .spacing(20),
            )
            .max_width(620),
        )
    } else if app.reboot_confirmation.is_some() {
        Some(panel(column![
            small("TWRP · DeviceInfo"),
            label(app.t("재부팅 전 확인", "Before rebooting")).size(25),
            label(transition_text(app)),
            label(app.t("작업이 완료될 때까지 PC와 태블릿의 USB 연결을 유지해 주세요. 케이블을 분리하거나 프로그램을 종료하지 마세요.", "Keep the tablet connected to the PC by USB until the operation is complete. Do not unplug the cable or close this app.")).color(danger()),
            row![space().width(Fill), action(app.t("확인", "Confirm"), Message::ConfirmRecoveryReboot, true)],
        ].spacing(20)).style(fluent::warning_dialog_style).max_width(620))
    } else if let Some(ticket) = app.stop_confirmation {
        Some(wait_ui::modal(app, ticket))
    } else if app.archive_confirmation.is_some() {
        Some(archive::modal(app))
    } else if app.fix.confirm {
        Some(conflicts::modal(app))
    } else if app.country_open {
        Some(country_view::modal(app))
    } else if app.license_open {
        Some(licenses_view::modal(app))
    } else {
        None
    };
    if let Some(modal) = modal {
        stack![
            root,
            opaque(container(modal).center(Fill).style(|_| container::Style {
                background: Some(Background::Color(fluent::p().scrim)),
                ..Default::default()
            }))
        ]
        .into()
    } else {
        root
    }
}
pub(super) fn footer<'a>(app: &App, next: Option<(Message, bool)>) -> Element<'a, Message> {
    let mut buttons = row![
        space().width(Fill),
        action(app.t("이전", "Back"), Message::Back, !app.busy()).style(soft_style),
    ]
    .spacing(12);
    if let Some((message, enabled)) = next {
        buttons = buttons.push(action(
            app.t("다음", "Continue"),
            message,
            enabled && !app.busy(),
        ));
    }
    buttons.into()
}
fn region(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut choices = row![].spacing(16);
    for (region, name, description) in [
        (
            Region::Row,
            "ROW",
            app.t("글로벌 펌웨어", "Global firmware"),
        ),
        (Region::Prc, "PRC", app.t("중국 펌웨어", "China firmware")),
    ] {
        let selected = app.region == Some(region);
        choices = choices.push(
            button(
                column![
                    small(if selected {
                        app.t("선택됨", "SELECTED")
                    } else {
                        " "
                    }),
                    label(name).size(38),
                    label(description)
                ]
                .spacing(15),
            )
            .padding(26)
            .width(Fill)
            .on_press_maybe((!app.busy()).then_some(Message::Region(region)))
            .style(move |theme, status| {
                let mut style = button::secondary(theme, status);
                style.background = Some(if selected { pale() } else { card_color() }.into());
                style.border = Border {
                    color: if selected { accent() } else { line() },
                    width: if selected { 2.0 } else { 1.0 },
                    radius: fluent::CARD_RADIUS.into(),
                };
                style
            }),
        );
    }
    (column![title(app, "변경할 리전을 선택하세요", "Choose the target region", "선택한 리전과 일치하는 공식 펌웨어를 사용합니다.", "Use official firmware matching your target region."), choices,
        note(app.t("지원 모델: {models}. 롤백인덱스 초기화 여부는 변환 설정에서 선택합니다.", "Supported models: {models}. Choose whether to reset rollback indices in Conversion options.").replace("{models}", &super::supported_models()), false),
        small(app.t("현재 기기의 리전과 관계없이 선택한 목표 리전으로 진행합니다.", "The selected target is used regardless of the device's current region."))].spacing(26).into(),
        row![small(app.t("01 / 04 · 준비", "01 / 04 · PREPARATION")), space().width(Fill), action(app.t("펌웨어 선택", "Choose firmware"), Message::Next, !app.busy() && app.region.is_some())].align_y(Alignment::Center).into())
}
fn firmware(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![
        title(
            app,
            "펌웨어를 확인합니다",
            "Check your firmware",
            "폴더를 선택하면 모델·리전·필수 파일을 검사합니다.",
            "Select a folder to validate its model, region and required files."
        ),
        row![
            action(
                app.t("폴더 선택", "Choose folder"),
                Message::Pick(Picker::Firmware),
                !app.busy()
            ),
            small(format!(
                "{}: {}",
                app.t("목표", "Target"),
                app.region.map_or("—", Region::token)
            ))
        ]
        .spacing(18)
        .align_y(Alignment::Center)
    ]
    .spacing(20);
    if let Some(folder) = &app.folder {
        content = content.push(small(folder.display().to_string()));
    }
    if app.busy() {
        content = content.push(note(
            app.t("펌웨어를 검사하고 있습니다…", "Checking firmware…"),
            false,
        ));
    }
    if let Some(report) = &app.report {
        content = content
            .push(note(
                app.t("펌웨어 검사를 통과했습니다", "Firmware validation passed"),
                false,
            ))
            .push(field(
                app.t("모델 / 리전", "Model / region"),
                format!("{} / {}", report.device, report.region.token()),
            ))
            .push(field("Build fingerprint", &report.fingerprint))
            .push(field(
                app.t("서명 키", "Signing key"),
                format!("{:?}", report.key_class),
            ))
            .push(field("boot", report.indices.boot.to_string()))
            .push(field(
                "vbmeta_system",
                report.indices.vbmeta_system.to_string(),
            ));
        if !report.missing_images.is_empty() {
            content = content.push(note(
                format!(
                    "{}\n{}",
                    app.t(
                        "다음 누락 이미지는 건너뜁니다.",
                        "These missing images will be skipped:"
                    ),
                    report
                        .missing_images
                        .iter()
                        .map(|m| m.filename.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                true,
            ));
        }
    }
    content = content.push(small(app.t(
        "암호화된 .x 파일은 선택한 폴더 안에 XML로 복호화됩니다.",
        "Encrypted .x files are decrypted to XML in the selected folder.",
    )));
    (
        content.into(),
        footer(app, Some((Message::Next, app.report.is_some()))),
    )
}
fn options(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![title(app, "변환 설정", "Conversion options", "국가 코드를 선택하고 백업 위치를 확인하세요.", "Choose the country code and review the backup location."),
        row![small(app.t("사용자 데이터", "User data")).width(FIELD_KEY), label(app.t("초기화 필수", "Erasure required")).color(danger()).width(Fill)].spacing(12),
        warning_note(app.t("리전 변환 시 사용자 데이터는 반드시 초기화됩니다. 앱·사진·계정이 삭제되므로 중요한 파일은 먼저 별도로 백업하세요.", "Region conversion always erases user data. Apps, photos and accounts will be deleted. Back up important files separately first.")),
        field(app.t("롤백인덱스", "Rollback indices"), app.t("초기화가 기본값입니다. 미초기화는 기존 값을 유지하며, 펌웨어 롤백인덱스가 더 낮으면 TWRP 단계에서 초기화 또는 작업 취소를 묻습니다.", "Reset is the default. Keep preserves the values; if the firmware indices are lower, TWRP asks whether to reset them or cancel.")),
        row![
            button(label(app.t("초기화 · 32개 인덱스 → 0", "Reset · 32 indices → 0")).color(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {fluent::p().on_accent}else{ink()})).padding(14).on_press_maybe((!app.busy()).then_some(Message::Rollback(lapiswitch_core::deviceinfo::RollbackMode::Reset))).style(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {super::fluent::accent_style}else{soft_style}),
            button(label(app.t("미초기화 · 기존 값 유지", "Keep · Preserve values")).color(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Keep {fluent::p().on_accent}else{ink()})).padding(14).on_press_maybe((!app.busy()).then_some(Message::Rollback(lapiswitch_core::deviceinfo::RollbackMode::Keep))).style(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Keep {super::fluent::accent_style}else{soft_style}),
        ].spacing(12),
        country_view::selected(app),
        small(app.t("국가명·코드로 검색한 뒤 적용하세요. ‘변경 안 함’도 선택할 수 있습니다.", "Search by country or code and apply. You can also choose Do not change.")),
    ].spacing(22);
    content = content
        .push(field(
            app.t("로그 및 백업 위치", "Logs and backup location"),
            app.output.display().to_string(),
        ))
        .push(small(app.t(
            "백업 폴더 위치는 상단 설정에서 변경할 수 있습니다.",
            "Change the backup folder in Settings above.",
        )));
    (
        content.into(),
        footer(app, Some((Message::Next, app.options().is_some()))),
    )
}
/// Region conversion always erases user data.
/// What the reboot about to be confirmed does. An edit installs firmware
/// only when it changed the region, which the editor decided in TWRP.
fn transition_text(app: &App) -> &'static str {
    use lapiswitch_core::recovery_flow::Transition;
    let edit = app.run_kind == RunKind::Edit;
    match (app.recovery_transition, edit) {
        (Some(Transition::Twrp), false) => app.t("태블릿을 TWRP Recovery로 재부팅합니다. TWRP에서 DeviceInfo를 백업한 뒤 선택한 리전과 롤백 옵션을 적용하고 검증합니다.", "Reboot to TWRP to back up, update and verify DeviceInfo."),
        (Some(Transition::Twrp), true) => app.t("태블릿을 TWRP Recovery로 재부팅합니다. TWRP에서 DeviceInfo를 읽어 백업한 뒤 수정할 값을 선택합니다.", "Reboot to TWRP. DeviceInfo is read and backed up there, then you choose the values to change."),
        (Some(Transition::Edl), false) => app.t("DeviceInfo 쓰기와 검증을 마쳤습니다. EDL로 전환하여 원본 GPT·Recovery를 복원한 뒤 공식 펌웨어를 설치합니다. 사용자 데이터는 초기화됩니다.", "DeviceInfo verified. Enter EDL, restore original GPT/Recovery, then install firmware and erase user data."),
        (Some(Transition::Edl), true) => app.t("DeviceInfo 쓰기와 검증을 마쳤습니다. EDL로 전환하여 원본 GPT·Recovery를 복원합니다. 리전을 바꾼 경우에만 공식 펌웨어를 설치하고 사용자 데이터를 초기화합니다.", "DeviceInfo verified. Enter EDL and restore original GPT/Recovery. Firmware is installed, erasing user data, only if the region changed."),
        (None, false) => app.t("TWRP 처리 후 원본 구성을 복구하고 공식 펌웨어를 설치합니다.", "Restore the original configuration after TWRP, then install official firmware."),
        (None, true) => app.t("TWRP 처리 후 원본 구성을 복구합니다. 리전을 바꾼 경우에만 공식 펌웨어를 설치합니다.", "Restore the original configuration after TWRP. Firmware is installed only if the region changed."),
    }
}
/// One line naming what a saved run changes.
pub(super) fn request_label(app: &App, state: &lapiswitch_core::state::RunState) -> String {
    match state.request {
        Request::Convert { target, .. } => {
            format!(
                "{} · {}",
                app.t("리전 변환", "Region conversion"),
                target.token()
            )
        }
        Request::Edit => match &state.firmware {
            Some(binding) => format!(
                "{} · {}",
                app.t("DeviceInfo 수정", "DeviceInfo edit"),
                binding.region.token()
            ),
            None => app.t("DeviceInfo 수정", "DeviceInfo edit").into(),
        },
    }
}
fn summary<'a>(
    app: &App,
    region: Option<Region>,
    country: Option<&str>,
    firmware: String,
) -> Element<'a, Message> {
    column![
        field(
            app.t("목표 리전", "Target region"),
            region.map_or("—", Region::token)
        ),
        field(app.t("사용자 데이터", "User data"), app.t("삭제", "Erase")),
        field(
            app.t("국가 코드", "Country code"),
            country
                .filter(|c| *c != "—")
                .unwrap_or(app.t("변경 안 함", "Unchanged"))
        ),
        field(app.t("펌웨어", "Firmware"), firmware)
    ]
    .spacing(17)
    .into()
}
fn confirm(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let content = column![title(app, "시작 전 마지막 확인", "Ready to convert", "기기 한 대를 연결하고 USB 연결을 유지하세요.", "Connect one device and keep USB connected."),
        dashboard_view::connection_note(app),
        summary(app, app.region, app.country.as_deref(), app.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
        note(app.t("원본 GPT·활성 슬롯 Recovery·vendor_boot·misc 및 고유 파티션을 먼저 백업합니다. 변환이 시작되면 케이블을 빼거나 앱을 종료하지 마세요.", "Original GPT, active-slot Recovery and vendor_boot, misc and device-specific partitions are backed up first. Keep the cable connected and the app open throughout the conversion."), true),
        field(app.t("롤백인덱스", "Rollback indices"), if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {app.t("초기화 · 32개 → 0 / 낮은 펌웨어 index 허용", "Reset · 32 → 0 / lower firmware indices allowed")} else {app.t("미초기화 · 펌웨어가 더 낮으면 TWRP에서 확인", "Keep · asked in TWRP if firmware indices are lower")}),
        checkbox(app.agreed).label(app.t("사용자 데이터가 반드시 초기화되는 것과 위 안내를 확인했습니다.", "I understand that user data will be erased and have read the instructions.")).on_toggle(Message::Agree),
    ].spacing(24);
    (
        content.into(),
        row![
            small(app.start_blocker().unwrap_or("")).width(Fill),
            action(app.t("이전", "Back"), Message::Back, true).style(soft_style),
            action(
                if app.activity == Some(Activity::Archive) {
                    app.t("기록 보관 중…", "Archiving…")
                } else if app.deferred.is_some() {
                    app.t("연결 확인 중…", "Checking connection…")
                } else {
                    app.t("변환 시작", "Start conversion")
                },
                Message::Start,
                app.can_start()
            )
        ]
        .spacing(12)
        .into(),
    )
}
pub(super) fn logs(app: &App) -> Element<'_, Message> {
    let mut lines = app
        .logs
        .iter()
        .map(|l| label(l).size(12).width(Fill).into())
        .collect::<Vec<Element<'_, Message>>>();
    if let Some(line) = &app.transfer_line {
        lines.push(label(line).size(12).width(Fill).color(accent()).into());
    }
    container(
        scrollable(column(lines).spacing(6).width(Fill))
            .direction(fluent::thin_scrollbar())
            .style(fluent::scroll_style)
            .id("operation-logs")
            .width(Fill)
            .anchor_bottom()
            .height(180),
    )
    .padding(14)
    .width(Fill)
    .style(|_| container::Style {
        background: Some(fluent::p().bg.into()),
        border: Border {
            color: line(),
            width: 1.0,
            radius: fluent::CARD_RADIUS.into(),
        },
        ..Default::default()
    })
    .into()
}
fn overall_progress(app: &App, show_steps: bool) -> Element<'_, Message> {
    let progress = &app.progress;
    let steps = progress.flow.steps();
    let language = app.preferences.language;
    let mut content = column![
        row![
            label(app.t("전체 진행률", "Overall progress"))
                .size(15)
                .width(Fill),
            label(format!("{:.0}%", progress.percent()))
                .size(22)
                .color(accent()),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=100.0, progress.percent()).girth(10),
        small(format!(
            "STEP {:02} / {:02} · {}",
            progress.current + 1,
            steps.len(),
            steps[progress.current].title(language)
        )),
        small(app.t(
            "소요 시간 비중을 고려하며, 펌웨어 설치는 실제 전체 전송량을 반영합니다.",
            "Weighted by expected work; firmware installation follows actual total transfer."
        )),
    ]
    .spacing(8);
    if show_steps {
        // Five cards per row keep all ten new steps visible at minimum width.
        for (row_index, group) in steps.chunks(5).enumerate() {
            let mut cards = row![].spacing(5);
            for (column_index, step) in group.iter().enumerate() {
                let index = row_index * 5 + column_index;
                let current = index == progress.current;
                let done = index < progress.current || progress.complete();
                cards = cards.push(
                    container(
                        column![
                            small(format!(
                                "STEP {:02}{}",
                                index + 1,
                                if done { " ✓" } else { "" }
                            )),
                            label(step.title(language)).size(12).color(if current {
                                accent()
                            } else {
                                muted()
                            }),
                        ]
                        .spacing(4),
                    )
                    .padding(8)
                    .width(Fill)
                    .height(70)
                    .style(move |_| container::Style {
                        background: Some(if current {
                            pale().into()
                        } else {
                            card_color().into()
                        }),
                        border: Border {
                            color: if current { accent() } else { line() },
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }
            // Keep partial restore rows on the same five-column grid.
            for _ in group.len()..5 {
                cards = cards.push(space().width(Fill));
            }
            content = content.push(cards);
        }
    }
    content.into()
}
fn progress(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    // Literal pairs keep both headings visible to the translation check.
    let heading = if app.run_kind == RunKind::Restore {
        title(
            app,
            "원본 복구를 진행하고 있습니다",
            "Restoration in progress",
            "연결을 유지해 주세요. 단계가 끝나면 자동으로 넘어갑니다.",
            "Keep USB connected. Each step advances automatically.",
        )
    } else {
        title(
            app,
            "변환을 진행하고 있습니다",
            "Conversion in progress",
            "연결을 유지해 주세요. 단계가 끝나면 자동으로 넘어갑니다.",
            "Keep USB connected. Each step advances automatically.",
        )
    };
    let mut content = column![heading].spacing(18);
    if app.run_kind == RunKind::Restore {
        content = content.push(note(
            app.t(
                "백업과 작업 기록으로 DeviceInfo 상태를 판단합니다. 변경이 검증되면 TWRP에서 원래 리전을 복구한 뒤 vendor_boot·Recovery·GPT를 복원합니다. 결과 불명은 자동 복구를 차단하며 롤백인덱스는 유지합니다.",
                "Classify saved DeviceInfo evidence first. Verified region changes are reversed in TWRP before restoring vendor_boot, Recovery and GPT. Unknown results block automatic restoration. Rollback indices are retained.",
            ),
            false,
        ));
    }
    content = content.push(overall_progress(app, true));
    if let Some(detail) = &app.recovery_detail {
        content = content.push(note(detail.clone(), false));
    }
    if !app.comparison.is_empty() {
        let mut table = column![row![
            small(app.t("이미지", "Image")).width(Fill),
            small(app.t("기기 롤백인덱스", "Device rollback index")).width(Fill),
            small(app.t("펌웨어 롤백인덱스", "Firmware rollback index")).width(Fill)
        ]]
        .spacing(8);
        for comparison in &app.comparison {
            // Presentation only; the core has already made the rollback decision.
            if let Some((image, values)) = comparison
                .strip_prefix("롤백 비교 ")
                .and_then(|s| s.split_once(": 펌웨어 "))
                && let Some((firmware, device)) = values.split_once(", 기기 ")
            {
                table = table.push(row![
                    label(image).size(13).width(Fill),
                    label(device).size(13).width(Fill),
                    label(firmware).size(13).width(Fill)
                ]);
            } else {
                table = table.push(small(comparison));
            }
        }
        content = content.push(table);
    }
    if app.progress.flow.steps()[app.progress.current] == Step::Firmware
        && let Some(progress) = &app.flash
    {
        content = content.push(small(format!(
            "{} · {}%  /  {} {}",
            progress.partition,
            progress.percent,
            app.t("펌웨어 전송", "Firmware transfer"),
            if progress.operation_total_bytes > 0 {
                format!(
                    "{:.0}%",
                    (progress.operation_completed_bytes as f64
                        / progress.operation_total_bytes as f64
                        * 100.0)
                        .clamp(0.0, 100.0)
                )
            } else {
                app.t("계산 중", "Calculating").into()
            }
        )));
    }
    content = content.push(logs(app));
    (
        content.into(),
        row![
            small(app.t(
                "실시간 로그는 자동 저장됩니다. 쓰기 중에는 앱을 종료하지 마세요.",
                "Live logs are saved automatically. Keep the app open during writes.",
            ))
            .width(Fill),
            wait_ui::button(app).unwrap_or_else(|| space().into()),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    )
}
fn result(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let success = matches!(app.result, Some(Ok(_)));
    let heading = if success {
        if app.run_kind == RunKind::Restore {
            app.t("원본 복구를 마쳤습니다", "Original backup restored")
        } else {
            app.t("변환 작업을 마쳤습니다", "Conversion completed")
        }
    } else {
        app.t("작업이 중단되었습니다", "Operation stopped")
    };
    let mut content = column![label(heading).size(29), overall_progress(app, false)].spacing(20);
    match &app.result {
        Some(Ok(warnings)) => {
            content = content.push(infobar(Severity::Success, match app.run_kind {
                RunKind::Restore => app.t("원래 리전과 원본 구성을 확인·복구한 뒤 시스템 부팅을 요청했습니다. 경고와 기기의 실제 부팅 상태를 확인해 주세요.", "Original region and configuration were verified/restored, then system boot was requested. Check warnings and actual device boot."),
                RunKind::Edit => app.t("DeviceInfo 변경과 원본 구성 복원을 마쳤습니다. 리전을 바꾼 경우 펌웨어도 설치했습니다. 기기의 실제 부팅을 확인해 주세요.", "DeviceInfo changes and the original configuration restore are complete. Firmware was installed if the region changed. Check actual device boot."),
                _ => app.t("펌웨어 쓰기와 마무리 단계를 마쳤습니다. 기기의 실제 부팅과 국가 코드를 확인해 주세요.", "Firmware writes and final steps are complete. Check actual device boot and country code."),
            }));
            for warning in warnings {
                content = content.push(note(warning, true));
            }
        }
        Some(Err(error)) => {
            content = content.push(error_note(error)).push(small(app.t(
                "아래 백업과 저장된 작업을 확인하세요. 원본 복구가 필요할 수 있습니다.",
                "Review the backup and saved run below. Original restoration may be required.",
            )));
        }
        None => {}
    }
    if let Some(path) = &app.backup_path {
        content = content.push(field(app.t("백업", "Backup"), path.display().to_string()));
    }
    if let Some(path) = &app.log_path {
        content = content.push(field(
            app.t("자동 저장 로그", "Saved transcript"),
            path.display().to_string(),
        ));
    }
    content = content.push(logs(app));
    (
        content.into(),
        row![
            space().width(Fill),
            action(app.t("처음으로", "Home"), Message::Home, true).style(soft_style),
            action(app.t("원본 복구", "Recovery"), Message::Recovery, true).style(recovery_style),
            action(
                app.t("로그 다른 곳에 저장", "Export log"),
                Message::Pick(Picker::Export),
                app.log_path.is_some() || app.demo
            )
        ]
        .spacing(10)
        .into(),
    )
}
/// DeviceInfo edit setup. Values are chosen later from the live DeviceInfo
/// read in TWRP; only the EDL loader is needed now.
fn edit(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![
        title(
            app,
            "DeviceInfo 수정",
            "Edit DeviceInfo",
            "기기의 DeviceInfo를 TWRP에서 읽은 뒤 원하는 값을 직접 수정합니다.",
            "Read the device's DeviceInfo in TWRP, then edit the values directly."
        ),
        dashboard_view::connection_note(app),
        action(
            app.t("EDL 로더 선택", "Choose EDL loader"),
            Message::Pick(Picker::Loader),
            !app.busy()
        )
        .style(soft_style)
    ]
    .spacing(20);
    if let Some(path) = &app.loader {
        content = content.push(small(path.display().to_string()));
    }
    content = content
        .push(note(
            app.t(
                "원본 GPT·활성 슬롯 Recovery·vendor_boot·misc 및 고유 파티션을 먼저 백업하고 TWRP로 부팅합니다. TWRP에서 읽은 DeviceInfo 전체 필드가 편집 창에 표시되며, 각 값은 필드 형식과 크기에 맞게 입력해야 합니다.",
                "Original GPT, active-slot Recovery and vendor_boot, misc and device-specific partitions are backed up first, then the tablet boots to TWRP. Every DeviceInfo field read there is shown in the editor; each value must match its field's type and size.",
            ),
            true,
        ))
        .push(note(
            app.t(
                "리전을 바꾸지 않으면 펌웨어를 설치하지 않고 사용자 데이터를 유지합니다. 리전을 바꾸면 해당 리전 펌웨어 설치가 필수이며 사용자 데이터가 반드시 초기화됩니다.",
                "Without a region change no firmware is installed and user data is kept. A region change requires installing that region's firmware, which always erases user data.",
            ),
            false,
        ))
        .push(warning_note(app.t(
            "보안·구조 필드를 잘못 바꾸면 부팅이 되지 않을 수 있습니다. 쓰기 전 변경 목록을 반드시 확인하세요. 편집 창에서 취소하면 DeviceInfo를 바꾸지 않고 원본 구성을 자동으로 복구합니다.",
            "Wrong security/structure values can stop the tablet from booting. Always review the change list before writing. Cancelling in the editor leaves DeviceInfo unchanged and restores the original configuration automatically.",
        )))
        .push(
            checkbox(app.agreed)
                .label(app.t(
                    "위 안내를 확인했으며 DeviceInfo 수정을 진행하겠습니다.",
                    "I have read the instructions and want to edit DeviceInfo.",
                ))
                .on_toggle(Message::Agree),
        );
    (
        content.into(),
        row![
            space().width(Fill),
            action(app.t("이전", "Back"), Message::Back, !app.busy()).style(soft_style),
            action(
                app.t("DeviceInfo 수정 시작", "Start DeviceInfo edit"),
                Message::Start,
                app.can_start()
            )
        ]
        .spacing(12)
        .into(),
    )
}
fn recovery(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![
        title(
            app,
            "원본 구성 복구",
            "Restore original configuration",
            "백업한 기기 한 대를 EDL로 연결하세요.",
            "Connect the backed-up device in EDL mode."
        ),
        action(
            app.t("백업 폴더 선택", "Choose backup folder"),
            Message::Pick(Picker::Backup),
            !app.busy()
        )
        .style(soft_style)
    ]
    .spacing(20);
    if app.device.unsupported_model() {
        content = content.push(dashboard_view::connection_note(app));
    }
    if let Some(path) = &app.recovery_folder {
        content = content.push(small(path.display().to_string()));
    }
    if let Some(summary) = &app.recovery_summary {
        content = content.push(note(
            format!(
                "{}\n{summary}",
                app.t(
                    "백업 파일과 해시 확인 완료",
                    "Backup files and hashes verified"
                )
            ),
            false,
        ));
    }
    if app.busy() {
        content = content.push(small(
            app.t("백업을 검사하고 있습니다…", "Verifying backup…"),
        ));
    }
    content = content.push(
        action(
            app.t("호환 loader 선택", "Choose compatible loader"),
            Message::Pick(Picker::Loader),
            !app.busy(),
        )
        .style(soft_style),
    );
    if let Some(path) = &app.loader {
        content = content.push(small(path.display().to_string()));
    }
    content = content.push(warning_note(app.t("이 작업은 설치된 전체 펌웨어를 이전 버전으로 되돌리는 기능이 아닙니다. DeviceInfo 변경이 검증되면 TWRP에서 작업 전 DeviceInfo 전체를 다시 기록한 뒤 원본 vendor_boot·Recovery·GPT를 복구합니다. 쓰기 결과가 불명확하면 자동 복구를 진행하지 않습니다.", "This does not roll back the entire installed firmware. Verified DeviceInfo changes are reversed in TWRP by writing back the complete original DeviceInfo before restoring original vendor_boot, Recovery and GPT. Unknown write results block automatic restoration.")))
        .push(checkbox(app.agreed).label(app.t("이 백업을 만든 기기이며 원본 복구를 진행하겠습니다.", "This is the device that created the backup; restore its original region and configuration.")).on_toggle(Message::Agree));
    (
        content.into(),
        row![
            space().width(Fill),
            action(app.t("이전", "Back"), Message::Back, !app.busy()).style(soft_style),
            action(
                app.t("원본 복구 시작", "Restore original"),
                Message::RestoreStart,
                app.can_restore()
            )
        ]
        .spacing(12)
        .into(),
    )
}
pub(super) fn driver_status(app: &App) -> String {
    use ltbox_device::driver::DriverStatus::*;
    if app.driver_checking {
        return app.t("드라이버 확인 중…", "Checking drivers…").into();
    }
    match &app.driver_status {
        Some(Present) => app
            .t(
                "필요한 드라이버가 설치되어 있습니다.",
                "Required drivers are present.",
            )
            .into(),
        Some(NotWindows) => app
            .t(
                "이 운영체제에는 추가 드라이버가 필요하지 않습니다.",
                "No additional driver is required on this platform.",
            )
            .into(),
        Some(Missing(files)) => format!(
            "{}: {}",
            app.t("드라이버 설치 필요", "Missing drivers"),
            files.join(", ")
        ),
        Some(UdevRulesMissing) => app
            .t(
                "USB 접근을 위한 udev 규칙이 필요합니다.",
                "Install udev rules for USB access.",
            )
            .into(),
        Some(UdevRulesStale) => app
            .t(
                "설치된 udev 규칙을 갱신해 주세요.",
                "Update the installed udev rules.",
            )
            .into(),
        Some(UdevRulesNoPermission) => app
            .t(
                "udev 규칙의 읽기 권한을 확인해 주세요.",
                "Check read permissions for udev rules.",
            )
            .into(),
        Some(KernelDriverMissing) => app
            .t(
                "Qualcomm Kernel 드라이버가 필요합니다.",
                "Install the Qualcomm kernel driver.",
            )
            .into(),
        Some(KernelDriverUnsupported) => app
            .t(
                "이 환경에서는 Userspace 모드를 사용해 주세요.",
                "Use Userspace mode on this platform.",
            )
            .into(),
        None => app
            .t("드라이버를 확인해 주세요.", "Check driver status.")
            .into(),
    }
}
fn resume(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![title(
        app,
        "저장된 작업 확인",
        "Review saved run",
        "저장된 설정과 현재 단계를 확인하세요.",
        "Review the saved settings and current stage."
    )]
    .spacing(22);
    if app.device.unsupported_model() {
        content = content.push(dashboard_view::connection_note(app));
    }
    if let Some(pending) = &app.pending {
        let state = &pending.state;
        content = content.push(field(app.t("작업", "Operation"), request_label(app, state)));
        content = match &state.firmware {
            Some(binding) => content.push(summary(
                app,
                Some(binding.region),
                state.options.country.as_deref(),
                binding.firmware_dir.display().to_string(),
            )),
            None => content.push(field(
                app.t("펌웨어", "Firmware"),
                app.t(
                    "설치 안 함 · 사용자 데이터 유지",
                    "Not installed · user data kept",
                ),
            )),
        };
        content = content.push(field(
            app.t("백업", "Backup"),
            state.backup_dir.display().to_string(),
        ));
        content = content.push(field(
            app.t("현재 단계", "Saved stage"),
            format!("{:?}", state.recovery.phase),
        ));
        if let Request::Convert { rollback_mode, .. } = state.request {
            content = content.push(field(
                app.t("롤백인덱스", "Rollback indices"),
                if rollback_mode == lapiswitch_core::deviceinfo::RollbackMode::Reset {
                    app.t("초기화 · 32개 → 0", "Reset · 32 → 0")
                } else {
                    app.t("미초기화", "Keep")
                },
            ));
        }
        let guidance = match pending.action {
            ResumeAction::Recovery => app.t("현재 기록으로는 이어갈 수 없습니다. 백업과 작업 기록을 확인한 뒤 원본 복구를 사용하세요.", "This checkpoint cannot be resumed. Review the backup and saved record, then use original restoration."),
            ResumeAction::Flash => app.t("펌웨어를 다시 설치하며 사용자 데이터를 반드시 초기화합니다.", "Firmware will be installed again and user data will be erased."),
            ResumeAction::Finalize => app.t("쓰기 완료 뒤 최종 재부팅 인계 기록을 확인합니다. 실제 부팅은 기기에서 확인하세요.", "Check the final reboot handoff after completed writes. Check actual boot on the device."),
            _ => app.t("저장된 TWRP·DeviceInfo 단계부터 결과를 재확인합니다. 검증 후 원본 GPT·Recovery 복원과 펌웨어 설치를 진행합니다.", "Recheck the saved TWRP/DeviceInfo stage, then restore original GPT/Recovery and install firmware."),
        };
        content = content.push(note(guidance, true)).push(
            checkbox(app.agreed)
                .label(app.t(
                    "저장된 설정과 이어갈 작업을 확인했습니다.",
                    "I have reviewed the saved settings and resume behavior.",
                ))
                .on_toggle(Message::Agree),
        );
    }
    (
        content.into(),
        row![
            space().width(Fill),
            action(app.t("이전", "Back"), Message::Back, !app.busy()).style(soft_style),
            action(
                app.t("원본 복구", "Recovery"),
                Message::Recovery,
                !app.busy()
            )
            .style(recovery_style),
            action(
                app.t("이어하기", "Resume"),
                Message::ResumeStart,
                app.can_resume()
            )
        ]
        .spacing(10)
        .into(),
    )
}
