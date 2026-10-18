use super::*;
use iced::widget::{
    self, button, checkbox, column, container, opaque, progress_bar, row, scrollable, space, stack,
    text,
};
use iced::{Alignment, Background, Border, Color, Fill};
pub(super) const INK: Color = Color::from_rgb8(25, 39, 62);
pub(super) const MUTED: Color = Color::from_rgb8(100, 116, 139);
pub(super) const ACCENT: Color = Color::from_rgb8(36, 89, 198);
pub(super) const PALE: Color = Color::from_rgb8(238, 244, 255);
pub(super) const LINE: Color = Color::from_rgb8(221, 229, 240);
/// Very light gray for unselected choices and secondary actions.
pub(super) const SOFT: Color = Color::from_rgb8(242, 244, 247);
const SOFT_HOVER: Color = Color::from_rgb8(231, 235, 241);
const DANGER: Color = Color::from_rgb8(187, 42, 52);
const FIELD_KEY: f32 = 165.0;
fn faded(style: button::Style, status: button::Status) -> button::Style {
    if status != button::Status::Disabled {
        return style;
    }
    button::Style {
        background: style.background.map(|b| b.scale_alpha(0.5)),
        text_color: style.text_color.scale_alpha(0.5),
        ..style
    }
}
/// Very light gray button with dark text and a thin outline.
pub(super) fn soft_style(_: &Theme, status: button::Status) -> button::Style {
    faded(
        button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => SOFT_HOVER,
                    _ => SOFT,
                }
                .into(),
            ),
            text_color: INK,
            border: Border {
                color: LINE,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        },
        status,
    )
}
/// Transparent until hovered: the header icon and idle navigation.
pub(super) fn quiet_style(_: &Theme, status: button::Status) -> button::Style {
    faded(
        button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(SOFT_HOVER.into()),
                _ => None,
            },
            text_color: INK,
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        status,
    )
}
pub(super) fn label<'a>(value: impl Into<String>) -> widget::Text<'a> {
    text(transcript::readable_paths(&value.into()))
        .shaping(widget::text::Shaping::Advanced)
        .color(INK)
}
pub(super) fn small<'a>(value: impl Into<String>) -> widget::Text<'a> {
    label(value).size(13).color(MUTED)
}
pub(super) fn panel<'a>(
    content: impl Into<Element<'a, Message>>,
) -> widget::Container<'a, Message> {
    container(content)
        .padding(24)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border {
                color: LINE,
                width: 1.0,
                radius: 16.0.into(),
            },
            ..Default::default()
        })
}
pub(super) fn note<'a>(value: impl Into<String>, warning: bool) -> Element<'a, Message> {
    container(label(value).size(14))
        .padding(14)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(
                if warning {
                    Color::from_rgb8(255, 247, 228)
                } else {
                    PALE
                }
                .into(),
            ),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
pub(super) fn field<'a>(key: impl Into<String>, value: impl Into<String>) -> Element<'a, Message> {
    // Wide enough for 12 CJK characters, so Japanese labels don't wrap mid-word.
    row![small(key).width(FIELD_KEY), label(value).width(Fill)]
        .spacing(12)
        .into()
}
pub(super) fn result_error_text(error: &str) -> String {
    let heading = lapiswitch_core::prepare::ROLLBACK_REJECTED_MESSAGE;
    if let Some(details) = error.strip_prefix(heading) {
        format!("{heading}\n{}", details.trim_start())
    } else {
        error.into()
    }
}
fn error_note<'a>(error: &str) -> Element<'a, Message> {
    warning_note(result_error_text(error))
}
fn warning_note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    container(label(value).size(14).color(DANGER))
        .padding(14)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(255, 247, 228).into()),
            border: Border {
                color: DANGER,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        })
        .into()
}
pub(super) fn notification<'a>(
    content: impl Into<Element<'a, Message>>,
) -> widget::Container<'a, Message> {
    panel(content).padding(14)
}
pub(super) fn notification_note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    notification(label(value).size(14)).into()
}
pub(super) fn recovery_style(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::secondary(theme, status);
    let disabled = status == button::Status::Disabled;
    let red = if disabled { MUTED } else { DANGER };
    style.background = Some(
        match status {
            button::Status::Hovered | button::Status::Pressed => Color::from_rgb8(255, 240, 241),
            _ => Color::WHITE,
        }
        .into(),
    );
    style.text_color = red;
    style.border.color = red;
    style.border.width = 1.0;
    style
}
/// White rounded pill with a thin outline: connection status and DEMO.
pub(super) fn pill_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::WHITE.into()),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}
pub(super) fn action<'a>(
    title: &'static str,
    message: Message,
    enabled: bool,
) -> widget::Button<'a, Message> {
    button(text(title).shaping(widget::text::Shaping::Advanced))
        .padding([11, 20])
        .on_press_maybe(enabled.then_some(message))
}
pub(super) fn title<'a>(
    app: &App,
    ko: &'static str,
    en: &'static str,
    sub_ko: &'static str,
    sub_en: &'static str,
) -> Element<'a, Message> {
    column![label(app.t(ko, en)).size(29), small(app.t(sub_ko, sub_en))]
        .spacing(8)
        .into()
}
pub(super) fn view(app: &App) -> Element<'_, Message> {
    let mut header = row![
        row![
            widget::image(crate::brand::image()).width(44).height(44),
            column![
                label(crate::brand::NAME).size(24),
                small(crate::brand::SUBTITLE)
            ]
            .spacing(2)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        space().width(Fill),
        dashboard_view::badge(app),
        widget::tooltip(
            button(
                widget::svg(widget::svg::Handle::from_memory(
                    include_bytes!("../../assets/settings.svg").as_slice(),
                ))
                .width(24)
                .height(24)
            )
            .padding(12)
            .on_press_maybe((!app.busy()).then_some(Message::Settings))
            .style(quiet_style),
            app.t("설정", "Settings"),
            widget::tooltip::Position::Bottom,
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if app.demo {
        header = header.push(
            container(label("DEMO").size(12).color(ACCENT))
                .padding([8, 12])
                .style(pill_style),
        );
    }
    let steps = [
        app.t("리전 선택", "Target region"),
        app.t("펌웨어 검사", "Firmware"),
        app.t("변환 설정", "Options"),
        app.t("최종 확인", "Review"),
        app.t("진행 / 결과", "Progress / result"),
    ];
    let active = match app.page {
        Page::Region => 0,
        Page::Firmware => 1,
        Page::Options => 2,
        Page::Confirm => 3,
        Page::Progress | Page::Result => 4,
        _ => 9,
    };
    let mut nav = column![
        action("Dashboard", Message::Home, !app.busy())
            .width(Fill)
            .style(if app.page == Page::Dashboard {
                button::primary
            } else {
                quiet_style
            })
    ]
    .spacing(12);
    if app.conversion_steps_visible() {
        nav = nav.push(small(app.t("변환 순서", "CONVERSION")));
        for (i, step) in steps.iter().enumerate() {
            let selected = i == active;
            nav = nav.push(
                container(
                    row![
                        label(format!("{:02}", i + 1)).size(14).color(if selected {
                            ACCENT
                        } else {
                            MUTED
                        }),
                        label(*step).size(14)
                    ]
                    .spacing(12),
                )
                .padding([13, 14])
                .width(Fill)
                .style(move |_| container::Style {
                    background: selected.then_some(PALE.into()),
                    border: Border {
                        radius: 9.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            );
        }
    }
    nav = nav
        .push(space().height(Fill))
        .push(small("Lenovo TB520FU · lapis"))
        .push(small(format!("v{} · GPL-3.0", env!("CARGO_PKG_VERSION"))));
    let mut body = column![].spacing(18);
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
                        pending.state.target_region.token(),
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
        .push(scrollable(panel(content)).width(Fill).height(Fill))
        .push(footer);
    let root: Element<'_, Message> = container(
        column![
            header,
            row![container(nav).width(190).height(Fill), body.width(Fill)]
                .spacing(24)
                .height(Fill)
        ]
        .spacing(25),
    )
    .padding(26)
    .width(Fill)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(245, 247, 251).into()),
        text_color: Some(INK),
        ..Default::default()
    })
    .into();
    let modal = if app.close_warning {
        Some(panel(column![
            label(app.t("진행 중인 작업을 종료할까요?", "Close the running operation?")).size(24),
            label(if app.activity == Some(Activity::Install) { app.t("드라이버 설치 프로그램이 실행 중입니다. 설치가 끝날 때까지 기다려 주세요.", "The driver installer is running. Please wait for it to finish.") } else { app.t("쓰기 도중 종료하면 기기가 부팅되지 않을 수 있습니다. 저장된 단계와 백업으로 이어하기 또는 복구가 필요할 수 있습니다.", "Closing during writes can leave the device unable to boot. Resume or restore may be required using the saved state and backup.") }),
            row![space().width(Fill), action(app.t("계속 진행", "Keep running"), Message::Stay, true), action(app.t("앱 종료", "Close app"), Message::Exit, true).style(button::danger)].spacing(12),
        ].spacing(20)).max_width(560))
    } else if app.firmware_confirmation.is_some() {
        Some(panel(column![
            label(app.t("순정롬 확인 불가", "Stock firmware could not be verified")).size(25).color(DANGER),
            label(app.t(
                "기기의 펌웨어가 순정롬 버전 검사를 통과하지 못했습니다. 커스텀롬, GSI 또는 테스트 빌드일 가능성이 높습니다. 계속 작업을 진행하시겠습니까?",
                "The device firmware could not pass the stock firmware version check. It may be a custom ROM, GSI or test build. Do you want to continue?"
            )).color(DANGER),
            row![
                space().width(Fill),
                action(app.t("취소", "Cancel"), Message::ConfirmUnrecognizedFirmware(false), true).style(soft_style),
                action(app.t("계속 진행", "Continue"), Message::ConfirmUnrecognizedFirmware(true), true),
            ].spacing(12),
        ].spacing(20)).style(|_| container::Style {
            background: Some(Color::from_rgb8(255, 247, 228).into()),
            text_color: Some(DANGER),
            border: Border { color: DANGER, width: 2.0, radius: 16.0.into() },
            ..Default::default()
        }).max_width(620))
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
            label(if let Some(transition)=app.recovery_transition {match transition {lapiswitch_core::recovery_flow::Transition::Twrp=>app.t("태블릿을 TWRP Recovery로 재부팅합니다. TWRP에서 DeviceInfo를 백업한 뒤 선택한 리전과 롤백 옵션을 적용하고 검증합니다.","Reboot to TWRP to back up, update and verify DeviceInfo."),lapiswitch_core::recovery_flow::Transition::Edl=>app.t("DeviceInfo 쓰기와 검증을 마쳤습니다. EDL로 전환하여 원본 GPT·Recovery를 복원한 뒤 공식 펌웨어를 설치합니다. 사용자 데이터는 초기화됩니다.","DeviceInfo verified. Enter EDL, restore original GPT/Recovery, then install firmware and erase user data.")}} else {app.t("TWRP 처리 후 원본 구성을 복구하고 공식 펌웨어를 설치합니다.","Restore the original configuration after TWRP, then install official firmware.")}),
            label(app.t("작업이 완료될 때까지 PC와 태블릿의 USB 연결을 유지해 주세요. 케이블을 분리하거나 프로그램을 종료하지 마세요.", "Keep the tablet connected to the PC by USB until the operation is complete. Do not unplug the cable or close this app.")).color(DANGER),
            row![space().width(Fill), action(app.t("확인", "Confirm"), Message::ConfirmRecoveryReboot, true)],
        ].spacing(20)).style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border { color: DANGER, width: 2.0, radius: 16.0.into() },
            ..Default::default()
        }).max_width(620))
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
                background: Some(Background::Color(Color::from_rgba8(16, 28, 48, 0.55))),
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
                style.background = Some(if selected { PALE } else { Color::WHITE }.into());
                style.border = Border {
                    color: if selected { ACCENT } else { LINE },
                    width: if selected { 2.0 } else { 1.0 },
                    radius: 12.0.into(),
                };
                style
            }),
        );
    }
    (column![title(app, "변경할 리전을 선택하세요", "Choose the target region", "선택한 리전과 일치하는 공식 펌웨어를 사용합니다.", "Use official firmware matching your target region."), choices,
        note(app.t("지원 모델은 TB520FU입니다. 롤백인덱스 초기화 여부는 변환 설정에서 선택합니다.", "For TB520FU only. Choose whether to reset rollback indices in Conversion options."), false),
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
                format!("TB520FU / {}", report.region.token()),
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
        row![small(app.t("사용자 데이터", "User data")).width(FIELD_KEY), label(app.t("초기화 필수", "Erasure required")).color(DANGER).width(Fill)].spacing(12),
        warning_note(app.t("리전 변환 시 사용자 데이터는 반드시 초기화됩니다. 앱·사진·계정이 삭제되므로 중요한 파일은 먼저 별도로 백업하세요.", "Region conversion always erases user data. Apps, photos and accounts will be deleted. Back up important files separately first.")),
        field(app.t("롤백인덱스", "Rollback indices"), app.t("초기화가 기본값입니다. 미초기화는 기존 다운그레이드 차단을 유지합니다.", "Reset is the default. Keep enforces the existing rollback check.")),
        row![
            button(label(app.t("초기화 · 32개 인덱스 → 0", "Reset · 32 indices → 0")).color(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {Color::WHITE}else{INK})).padding(14).on_press_maybe((!app.busy()).then_some(Message::Rollback(lapiswitch_core::deviceinfo::RollbackMode::Reset))).style(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {button::primary}else{soft_style}),
            button(label(app.t("미초기화 · 기존 값 유지", "Keep · Preserve values")).color(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Keep {Color::WHITE}else{INK})).padding(14).on_press_maybe((!app.busy()).then_some(Message::Rollback(lapiswitch_core::deviceinfo::RollbackMode::Keep))).style(if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Keep {button::primary}else{soft_style}),
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
fn summary<'a>(
    app: &App,
    region: Option<Region>,
    wipe: bool,
    country: Option<&str>,
    firmware: String,
) -> Element<'a, Message> {
    column![
        field(
            app.t("목표 리전", "Target region"),
            region.map_or("—", Region::token)
        ),
        field(
            app.t("사용자 데이터", "User data"),
            if wipe {
                app.t("삭제", "Erase")
            } else {
                app.t("유지", "Keep")
            }
        ),
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
        summary(app, app.region, true, app.country.as_deref(), app.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
        note(app.t("원본 GPT·활성 슬롯 Recovery·vendor_boot·misc 및 고유 파티션을 먼저 백업합니다. 변환이 시작되면 케이블을 빼거나 앱을 종료하지 마세요.", "Original GPT, active-slot Recovery and vendor_boot, misc and device-specific partitions are backed up first. Keep the cable connected and the app open throughout the conversion."), true),
        field(app.t("롤백인덱스", "Rollback indices"), if app.rollback_mode==lapiswitch_core::deviceinfo::RollbackMode::Reset {app.t("초기화 · 32개 → 0 / 낮은 펌웨어 index 허용", "Reset · 32 → 0 / lower firmware indices allowed")} else {app.t("미초기화 · 낮은 펌웨어 index 차단", "Keep · lower firmware indices blocked")}),
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
        lines.push(label(line).size(12).width(Fill).color(ACCENT).into());
    }
    container(
        scrollable(column(lines).spacing(6).width(Fill))
            .id("operation-logs")
            .width(Fill)
            .anchor_bottom()
            .height(180),
    )
    .padding(14)
    .width(Fill)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(247, 249, 252).into()),
        border: Border {
            radius: 10.0.into(),
            ..Default::default()
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
                .color(ACCENT),
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
                                ACCENT
                            } else {
                                MUTED
                            }),
                        ]
                        .spacing(4),
                    )
                    .padding(8)
                    .width(Fill)
                    .height(70)
                    .style(move |_| container::Style {
                        background: Some(if current {
                            PALE.into()
                        } else {
                            Color::from_rgb8(248, 250, 253).into()
                        }),
                        border: Border {
                            color: if current { ACCENT } else { LINE },
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }
            // Keep partial legacy/restore rows on the same five-column grid.
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
            content = content.push(note(if app.run_kind == RunKind::Restore { app.t("원래 리전과 원본 구성을 확인·복구한 뒤 시스템 부팅을 요청했습니다. 경고와 기기의 실제 부팅 상태를 확인해 주세요.", "Original region and configuration were verified/restored, then system boot was requested. Check warnings and actual device boot.") } else { app.t("펌웨어 쓰기와 마무리 단계를 마쳤습니다. 기기의 실제 부팅과 국가 코드를 확인해 주세요.", "Firmware writes and final steps are complete. Check actual device boot and country code.") }, false));
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
    content = content.push(warning_note(app.t("이 작업은 설치된 전체 펌웨어를 이전 버전으로 되돌리는 기능이 아닙니다. DeviceInfo 변경이 검증되면 TWRP에서 원래 리전으로 되돌린 뒤 원본 vendor_boot·Recovery·GPT를 복구합니다. 쓰기 결과가 불명확하면 자동 복구를 진행하지 않습니다. 롤백인덱스는 유지합니다.", "This does not roll back the entire installed firmware. Verified DeviceInfo region changes are reversed in TWRP before restoring original vendor_boot, Recovery and GPT. Unknown write results block automatic restoration. Rollback indices are retained.")))
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
        content = content
            .push(summary(
                app,
                Some(state.target_region),
                state.options.wipe,
                state.options.country.as_deref(),
                state.firmware_dir.display().to_string(),
            ))
            .push(field(
                app.t("백업", "Backup"),
                state.backup_dir.display().to_string(),
            ));
        if let Some(recovery) = &state.recovery {
            content = content
                .push(field(
                    app.t("현재 단계", "Saved stage"),
                    format!("{:?}", recovery.phase),
                ))
                .push(field(
                    app.t("롤백인덱스", "Rollback indices"),
                    if state.options.rollback_mode
                        == lapiswitch_core::deviceinfo::RollbackMode::Reset
                    {
                        app.t("초기화 · 32개 → 0", "Reset · 32 → 0")
                    } else {
                        app.t("미초기화", "Keep")
                    },
                ));
        }
        let guidance = if state.recovery.is_none() {
            app.t("커스텀 BOOT 방식의 변환·이어하기 지원을 종료했습니다. 기존 백업의 원본 복구 또는 이전 작업 보관 후 새 TWRP 변환을 사용하세요.", "Custom BOOT conversion and resume have been retired. Restore the original backup or archive the old run and start a new TWRP conversion.")
        } else {
            match pending.action {
                ResumeAction::Recovery | ResumeAction::ResetUncertain => app.t("현재 기록으로는 이어갈 수 없습니다. 백업과 작업 기록을 확인한 뒤 원본 복구를 사용하세요.", "This checkpoint cannot be resumed. Review the backup and saved record, then use original restoration."),
                ResumeAction::Flash => app.t("펌웨어를 다시 설치하며 사용자 데이터를 반드시 초기화합니다.", "Firmware will be installed again and user data will be erased."),
                ResumeAction::Finalize => app.t("쓰기 완료 뒤 최종 재부팅 인계 기록을 확인합니다. 실제 부팅은 기기에서 확인하세요.", "Check the final reboot handoff after completed writes. Check actual boot on the device."),
                _ => app.t("저장된 TWRP·DeviceInfo 단계부터 결과를 재확인합니다. 검증 후 원본 GPT·Recovery 복원과 펌웨어 설치를 진행합니다.", "Recheck the saved TWRP/DeviceInfo stage, then restore original GPT/Recovery and install firmware."),
            }
        };
        content = content.push(note(guidance, true));
        if state.recovery.is_some() {
            content = content.push(
                checkbox(app.agreed)
                    .label(app.t(
                        "저장된 설정과 이어갈 작업을 확인했습니다.",
                        "I have reviewed the saved settings and resume behavior.",
                    ))
                    .on_toggle(Message::Agree),
            );
        }
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
