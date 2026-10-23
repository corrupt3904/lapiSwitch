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
    infobar_with(severity.glyph(), severity, value)
}
/// An InfoBar with its own icon in the severity's tint.
fn infobar_with<'a>(
    glyph: char,
    severity: Severity,
    value: impl Into<String>,
) -> Element<'a, Message> {
    container(
        row![
            icon(glyph).color(severity.icon_color()),
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
/// Something that failed: a red bar with a crossed circle.
fn error_note<'a>(error: &str) -> Element<'a, Message> {
    infobar(Severity::Error, error.to_owned())
}
/// A serious caution before acting: a red bar with a warning triangle.
fn warning_note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    infobar_with(glyph::WARNING, Severity::Error, value)
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
        Page::Firmware | Page::Confirm => 1,
        Page::Edit => 2,
        Page::Recovery => 3,
        Page::Settings => 4,
        Page::Dump => 5,
        Page::Progress | Page::Result => match app.run_kind {
            RunKind::Convert => 1,
            RunKind::Edit => 2,
            RunKind::Dump => 5,
            RunKind::Restore => 3,
            RunKind::Resume => 0,
        },
    }
}
/// The selection marker at the left edge of a navigation item.
fn nav_marker<'a>(selected: bool, height: f32) -> Element<'a, Message> {
    container(space().width(3).height(height))
        .style(move |_| container::Style {
            background: selected.then(|| fluent::p().accent.into()),
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
/// Windows 11 settings-style navigation: the connected device card is the
/// home button, then the two tasks; restore and settings sit at the bottom.
/// Locked while an operation runs.
fn navigation(app: &App) -> Element<'_, Message> {
    let current = nav_section(app);
    let idle = !app.busy();
    let supported = !app.device.unsupported_model();
    let writable = supported && !app.device.dump_only();
    let item = |index: usize,
                glyph: char,
                name: &'static str,
                message: Message,
                enabled: bool,
                danger: bool| {
        let selected = current == index;
        let symbol = icon(glyph);
        let symbol = if danger && enabled {
            symbol.color(fluent::p().danger)
        } else {
            symbol
        };
        button(
            row![
                nav_marker(selected, 16.0),
                symbol,
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
    let home = current == 0;
    let device = button(
        row![nav_marker(home, 36.0), dashboard_view::device_card(app)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([12, 6])
    .on_press_maybe(idle.then_some(Message::Home))
    .style(move |_, status| {
        let p = fluent::p();
        button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => p.card_hover,
                    _ => p.card,
                }
                .into(),
            ),
            text_color: p.text,
            border: Border {
                color: if home { p.accent } else { p.card_border },
                width: 1.0,
                radius: fluent::CARD_RADIUS.into(),
            },
            ..Default::default()
        }
    });
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
    let divider = container(space().width(Fill).height(1)).style(|_| container::Style {
        background: Some(line().into()),
        ..Default::default()
    });
    column![
        container(brand).padding([0, 6]),
        container(device).padding(pad(14, 0, 10, 0)),
        item(
            1,
            glyph::FLASH,
            app.t("펌웨어 플래싱", "Firmware flashing"),
            Message::Convert,
            idle && writable,
            false
        ),
        item(
            2,
            glyph::EDIT,
            app.t("DeviceInfo 수정", "Edit DeviceInfo"),
            Message::Edit,
            idle && writable,
            false
        ),
        item(
            5,
            glyph::SAVE,
            app.t("DeviceInfo 덤프", "Dump DeviceInfo"),
            Message::Dump,
            idle && supported,
            false
        ),
        space().height(Fill),
        container(divider).padding([6, 6]),
        item(
            3,
            glyph::HISTORY,
            app.t("원본 복구", "Restore original"),
            Message::Recovery,
            idle,
            true
        ),
        item(
            4,
            glyph::SETTINGS,
            app.t("설정", "Settings"),
            Message::Settings,
            idle,
            false
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
/// The firmware flashing steps; each page's title is its step name.
fn flash_steps(app: &App) -> [&'static str; 3] {
    [
        app.t("펌웨어 검사", "Firmware check"),
        app.t("최종 확인", "Final check"),
        app.t("진행 / 결과", "Progress / result"),
    ]
}
/// Fluent settings group: rows in one card, separated by hairlines.
fn group<'a>(rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut content = column![];
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            content = content.push(container(space().width(Fill).height(1)).style(|_| {
                container::Style {
                    background: Some(line().into()),
                    ..Default::default()
                }
            }));
        }
        content = content.push(row);
    }
    container(content)
        .width(Fill)
        .style(fluent::card_style)
        .into()
}
/// A row of a group: name, value and an optional control on the right.
fn group_row<'a>(
    name: &'static str,
    value: Element<'a, Message>,
    control: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = row![
        label(name).size(14).width(FIELD_KEY),
        container(value).width(Fill).padding([2, 0]).clip(true)
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    if let Some(control) = control {
        content = content.push(control);
    }
    container(content).padding([14, 16]).width(Fill).into()
}
/// A group row that opens a choice: the whole row is the button.
fn link_row<'a>(
    name: &'static str,
    value: String,
    message: Message,
    enabled: bool,
) -> Element<'a, Message> {
    button(
        row![
            label(name).size(14).width(FIELD_KEY),
            container(
                label(value)
                    .size(14)
                    .color(muted())
                    .wrapping(iced::widget::text::Wrapping::None)
            )
            .width(Fill)
            .clip(true),
            icon(glyph::CHEVRON).size(12).color(muted())
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([14, 16])
    .width(Fill)
    .on_press_maybe(enabled.then_some(message))
    .style(|theme, status| {
        let mut style = fluent::subtle_style(theme, status);
        style.border.radius = 0.0.into();
        style
    })
    .into()
}
/// The chosen rollback handling, shown on the final check.
fn rollback_value(app: &App) -> String {
    match app.rollback_mode {
        lapiswitch_core::deviceinfo::RollbackMode::Reset => {
            app.t("초기화 (기본값)", "Reset (default)").into()
        }
        lapiswitch_core::deviceinfo::RollbackMode::Keep => app.t("유지", "Keep").into(),
    }
}
/// Reset or keep the rollback indices, chosen after the country code or
/// from the final check.
fn rollback_modal(app: &App) -> iced::widget::Container<'_, Message> {
    use lapiswitch_core::deviceinfo::RollbackMode;
    let mut choices = column![].spacing(4);
    for (mode, name, detail) in [
        (
            RollbackMode::Reset,
            app.t("초기화 (기본값)", "Reset (default)"),
            app.t(
                "32개 모두 0으로 기록합니다. 더 낮은 펌웨어도 설치할 수 있습니다.",
                "All 32 are written as 0, so lower firmware can be installed.",
            ),
        ),
        (
            RollbackMode::Keep,
            app.t("유지", "Keep"),
            app.t(
                "기존 값을 유지합니다. 펌웨어가 더 낮으면 TWRP에서 초기화 또는 취소를 묻습니다.",
                "Values are kept; if the firmware is lower, TWRP asks to reset or cancel.",
            ),
        ),
    ] {
        let selected = app.rollback_draft == mode;
        choices = choices.push(
            button(
                row![
                    label(if selected { "●" } else { "○" })
                        .size(17)
                        .color(accent()),
                    column![label(name).size(15), small(detail)].spacing(2)
                ]
                .spacing(13)
                .align_y(Alignment::Center),
            )
            .on_press(Message::Rollback(mode))
            .padding([12, 14])
            .width(Fill)
            .style(move |theme, state| {
                let mut style = button::secondary(theme, state);
                style.background = Some(if selected { pale() } else { card_color() }.into());
                style
            }),
        );
    }
    panel(
        column![
            label(app.t("롤백인덱스 선택", "Choose rollback handling")).size(25),
            choices,
            row![
                space().width(Fill),
                action(app.t("취소", "Cancel"), Message::RollbackCancel, true).style(soft_style),
                action(app.t("선택", "Select"), Message::RollbackApply, true)
            ]
            .spacing(12),
        ]
        .spacing(16),
    )
    .max_width(590)
}
/// How a region change affects later flashing, from the final check.
fn region_notice(app: &App) -> iced::widget::Container<'_, Message> {
    panel(
        column![
            row![
                icon(glyph::ALERT).size(22).color(danger()),
                label(app.t("Region 변경 안내", "About the region change")).size(25)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            label(app.t(
                "본 툴은 기기 자체의 Region을 변경하는 방식으로 작동합니다.\n따라서 이후 LTBOX에서 펌웨어 플래싱할때 '기기 지역 선택' 옵션에서 변경된 Region으로 선택해야 합니다.",
                "This tool works by changing the device's own region.\nWhen you later flash firmware with LTBox, choose the changed region in its 'Device region' option.",
            ))
            .size(15),
            label(app.t(
                "만약 내수용 기기에 본 툴을 사용하여 글로벌 펌웨어를 설치한 경우, 이후 Pixel 등 커스텀 펌웨어를 설치할 때는 반드시 ROW 버전을 선택해야 합니다.\n또한, 내수 펌웨어로 복귀할 경우에도 반드시 본 툴을 통해 내수 펌웨어를 플래싱해야 정상적인 OTA 업데이트가 지원됩니다.",
                "If you installed global firmware on a China-market device with this tool, always choose the ROW version when you later install custom firmware such as Pixel.\nTo return to China firmware, flash it with this tool as well; otherwise OTA updates will not work normally.",
            ))
            .size(15),
            row![
                space().width(Fill),
                action(app.t("확인", "Confirm"), Message::RegionNoticeClose, true)
            ],
        ]
        .spacing(16),
    )
    .max_width(640)
}
/// Average width of a path character in 14px text.
pub(super) const PATH_CHAR: f32 = 8.2;
/// Width left for a group row's value next to a control `control` wide.
fn value_width(app: &App, control: f32) -> f32 {
    // Navigation, page padding and the scrollbar gutter.
    let page = (app.window_width - 290.0 - 36.0).min(1000.0) - 18.0 - 8.0;
    page - 32.0 - FIELD_KEY - 16.0 - control
}
/// A path cut in the middle to fit `width` pixels of 14px text, keeping
/// the drive and the folder name: `D:\…FU_ROW_OPEN_USER_…_260929\image`.
pub(super) fn short_path(path: &std::path::Path, width: f32) -> String {
    let full = path.display().to_string();
    let budget = ((width / PATH_CHAR) as usize).max(12);
    let chars: Vec<char> = full.chars().collect();
    if chars.len() <= budget {
        return full;
    }
    let head = match chars.iter().position(|c| matches!(c, '\\' | '/')) {
        Some(i) if i < budget / 3 => i + 1,
        _ => 0,
    };
    let tail = budget - head - 1;
    let mut short: String = chars[..head].iter().collect();
    short.push('…');
    short.extend(&chars[chars.len() - tail..]);
    short
}
/// The firmware folder, shortened to fit a value column of `width`.
fn folder_value(app: &App, width: f32) -> String {
    path_value(app, app.folder.as_deref(), width)
}
/// A chosen path shortened to `width`, or "Not selected".
fn path_value(app: &App, path: Option<&std::path::Path>, width: f32) -> String {
    path.map(|p| short_path(p, width))
        .unwrap_or_else(|| app.t("선택되지 않음", "Not selected").into())
}
/// The chosen country code for the flashing options and final check.
fn country_value(app: &App) -> String {
    match app.country.as_deref() {
        None => app.t("선택하세요", "Choose").into(),
        Some("—") => app.t("변경 안 함", "Do not change").into(),
        Some(code) => match crate::countries::find(code) {
            Some(country) => format!("{} · {}", country.code, country.name),
            None => code.into(),
        },
    }
}
/// The start footer: a problem in an InfoBar right above the buttons, or
/// a step not done yet as a muted hint beside them.
fn start_footer<'a>(app: &'a App, buttons: [Element<'a, Message>; 2]) -> Element<'a, Message> {
    let issue = app.start_issue();
    let hint: Element<'a, Message> = match issue {
        Some((None, reason)) => label(reason).size(13).color(muted()).into(),
        _ => space().into(),
    };
    let [back, start] = buttons;
    let mut footer = column![].spacing(12);
    if let Some((Some(severity), reason)) = issue {
        footer = footer.push(infobar(severity, reason));
    }
    footer
        .push(
            row![container(hint).width(Fill), back, start]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .into()
}
/// Region conversion steps as a Fluent breadcrumb above the page.
fn stepper(app: &App) -> Element<'_, Message> {
    let steps = flash_steps(app);
    let page = if app.page == Page::Settings {
        app.return_page
    } else {
        app.page
    };
    let active = match page {
        Page::Firmware => 0,
        Page::Confirm => 1,
        _ => 2,
    };
    let mut crumbs = row![].spacing(8).align_y(Alignment::Center);
    let flow = label(app.t(
        "펌웨어 플래싱 (Region 변경)",
        "Firmware flashing (Region change)",
    ))
    .size(13)
    .color(muted());
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
    column![flow, crumbs].spacing(4).into()
}
/// The DeviceInfo edit, dump and restore pages and their runs, whose
/// titles line up with the flashing pages below the step breadcrumb.
fn deviceinfo_task(app: &App) -> bool {
    match app.page {
        Page::Edit | Page::Dump | Page::Recovery => true,
        Page::Progress | Page::Result => matches!(
            app.run_kind,
            RunKind::Edit | RunKind::Dump | RunKind::Restore
        ),
        _ => false,
    }
}
/// Empty space as tall as the stepper: the same two text lines.
fn stepper_space<'a>() -> Element<'a, Message> {
    column![label(" ").size(13), label(" ").size(14)]
        .spacing(4)
        .into()
}
pub(super) fn view(app: &App) -> Element<'_, Message> {
    fluent::set_dark(app.dark());
    let mut body = column![].spacing(12);
    if app.conversion_steps_visible() {
        body = body.push(stepper(app));
    } else if deviceinfo_task(app) {
        body = body.push(stepper_space());
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
                "저장된 작업 확인이 필요합니다. 새 작업은 시작할 수 없습니다.",
                "Saved state needs attention before a new operation."
            )
        )));
    }
    if let Some(error) = &app.error {
        body = body.push(error_note(error));
    }
    if let Some(notice) = &app.notice {
        body = body.push(notification_note(notice.clone()));
    }
    let (content, footer) = match app.page {
        Page::Dashboard => dashboard_view::dashboard(app),
        Page::Edit => edit(app),
        Page::Dump => dump(app),
        Page::Firmware => firmware(app),
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
    } else if app.rollback_open {
        Some(rollback_modal(app))
    } else if app.region_notice_open {
        Some(region_notice(app))
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
fn firmware(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let steps = flash_steps(app);
    let mut content = column![
        column![
            label(steps[0]).size(28),
            label(app.t(
                "펌웨어 폴더를 선택하면 모델·필수 파일을 검사하고, vendor_boot의 DTB에서 Region을 확인합니다.",
                "Choose the firmware folder: the model and required files are checked and the region is read from the vendor_boot DTB.",
            ))
            .size(14)
            .color(muted())
        ]
        .spacing(4),
        group(vec![group_row(
            app.t("펌웨어 폴더", "Firmware folder"),
            label(folder_value(app, value_width(app, 120.0)))
                .size(14)
                .color(muted())
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
            Some(
                action(
                    app.t("폴더 선택", "Choose folder"),
                    Message::Pick(Picker::Firmware),
                    !app.busy()
                )
                .into()
            ),
        )]),
    ]
    .spacing(12);
    if app.busy() {
        content = content.push(note(
            app.t("펌웨어를 검사하고 있습니다…", "Checking firmware…"),
            false,
        ));
    }
    if let Some(report) = &app.report {
        content = content
            .push(infobar(
                Severity::Success,
                app.t(
                    "펌웨어 검사를 통과했습니다. 플래싱하면 기기 Region이 이 펌웨어의 Region으로 바뀝니다.",
                    "Firmware check passed. Flashing changes the device region to this firmware's region.",
                ),
            ))
            .push(group(vec![
                group_row(
                    app.t("Region (vendor_boot DTB)", "Region (vendor_boot DTB)"),
                    label(report.region.token()).size(18).color(accent()).into(),
                    None,
                ),
                group_row(
                    app.t("모델", "Model"),
                    label(report.device.model()).size(14).into(),
                    None,
                ),
                group_row(
                    "Build fingerprint",
                    label(report.fingerprint.clone()).size(13).color(muted()).into(),
                    None,
                ),
                group_row(
                    app.t("서명 키", "Signing key"),
                    label(format!("{:?}", report.key_class)).size(14).into(),
                    None,
                ),
                group_row(
                    app.t("롤백인덱스", "Rollback indices"),
                    label(format!(
                        "boot {} · vbmeta_system {}",
                        report.indices.boot, report.indices.vbmeta_system
                    ))
                    .size(14)
                    .into(),
                    None,
                ),
            ]));
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
/// Region conversion always erases user data.
/// What the reboot about to be confirmed does. An edit installs firmware
/// only when it changed the region, which the editor decided in TWRP.
fn transition_text(app: &App) -> &'static str {
    use lapiswitch_core::recovery_flow::Transition;
    if app.run_kind == RunKind::Dump {
        return app.t(
            "태블릿을 TWRP Recovery로 재부팅합니다. TWRP에서 DeviceInfo를 읽어 저장한 뒤 원본 구성으로 복구합니다. 기기에는 쓰지 않습니다.",
            "Reboot to TWRP. DeviceInfo is read and saved there, then the original configuration is restored. Nothing is written to DeviceInfo.",
        );
    }
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
                app.t("펌웨어 플래싱", "Firmware flashing"),
                target.token()
            )
        }
        Request::Dump => app.t("DeviceInfo 덤프", "DeviceInfo dump").into(),
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
            app.t("목표 Region", "Target region"),
            region.map_or("—", Region::token)
        ),
        field(app.t("사용자 데이터", "User data"), app.t("삭제", "Erase")),
        field(
            app.t("국가 코드", "Country code"),
            country
                .filter(|c| *c != "—")
                .unwrap_or(app.t("변경 안 함", "Unchanged"))
        ),
        field(app.t("펌웨어 폴더", "Firmware folder"), firmware)
    ]
    .spacing(17)
    .into()
}
fn confirm(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let steps = flash_steps(app);
    let idle = !app.busy();
    let content = column![
        column![
            label(steps[1]).size(28),
            label(app.t(
                "기기 한 대를 연결하고 USB 연결을 유지하세요. 항목을 눌러 바꿀 수 있습니다.",
                "Connect one device and keep USB connected. Select an item to change it.",
            ))
            .size(14)
            .color(muted())
        ]
        .spacing(4),
        warning_note(app.t(
            "펌웨어 플래싱 시 사용자 데이터는 반드시 초기화됩니다.\n앱·사진·계정·파일 등이 삭제되므로 중요한 데이터는 먼저 별도로 백업하세요.",
            "Firmware flashing always erases user data.\nApps, photos, accounts, files and more are deleted, so back up important data separately first.",
        )),
        group(vec![
            group_row(
                app.t("목표 Region", "Target region"),
                row![
                    label(app.region.map_or("—", Region::token))
                        .size(18)
                        .color(accent()),
                    label(app.t("펌웨어 vendor_boot 기준", "from the firmware's vendor_boot"))
                        .size(13)
                        .color(muted())
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
                Some(
                    button(icon(glyph::ALERT).size(16).color(danger()))
                        .padding([4, 8])
                        .on_press(Message::RegionNoticeOpen)
                        .style(fluent::subtle_style)
                        .into(),
                ),
            ),
            group_row(
                app.t("사용자 데이터", "User data"),
                label(app.t("초기화", "Erased")).size(18).color(danger()).into(),
                None,
            ),
            link_row(
                app.t("국가 코드", "Country code"),
                country_value(app),
                Message::CountryOpen,
                idle,
            ),
            link_row(
                app.t("펌웨어 폴더", "Firmware folder"),
                folder_value(app, value_width(app, 30.0)),
                Message::Pick(Picker::Firmware),
                idle,
            ),
            link_row(
                app.t("롤백인덱스", "Rollback indices"),
                rollback_value(app),
                Message::RollbackOpen,
                idle,
            ),
            group_row(
                app.t("백업 위치", "Backup location"),
                label(short_path(
                    &app.output.join("backup").join("firmware_flashing"),
                    value_width(app, 150.0),
                ))
                .size(14)
                .color(muted())
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
                Some(small(app.t("설정에서 변경", "Change in Settings")).into()),
            ),
        ]),
        checkbox(app.agreed)
            .label(app.t(
                "사용자 데이터가 반드시 초기화되는 것과 위 안내를 확인했습니다.",
                "I understand that user data will be erased and have read the instructions."
            ))
            .on_toggle(Message::Agree),
    ]
    .spacing(12);
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.t("이전", "Back"), Message::Back, true)
                    .style(soft_style)
                    .into(),
                action(
                    if app.activity == Some(Activity::Archive) {
                        app.t("기록 보관 중…", "Archiving…")
                    } else if app.deferred.is_some() {
                        app.t("연결 확인 중…", "Checking connection…")
                    } else {
                        app.t("플래싱 시작", "Start flashing")
                    },
                    Message::Start,
                    app.can_start(),
                )
                .into(),
            ],
        ),
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
    ]
    .spacing(8);
    // Only a run that installs firmware weights its transfer.
    if steps.contains(&crate::progress::Step::Firmware) {
        content = content.push(small(app.t(
            "소요 시간 비중을 고려하며, 펌웨어 설치는 실제 전체 전송량을 반영합니다.",
            "Weighted by expected work; firmware installation follows actual total transfer.",
        )));
    }
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
    } else if app.run_kind == RunKind::Edit {
        title(
            app,
            "DeviceInfo 수정을 진행하고 있습니다",
            "Editing DeviceInfo",
            "연결을 유지해 주세요. 단계가 끝나면 자동으로 넘어갑니다.",
            "Keep USB connected. Each step advances automatically.",
        )
    } else if app.run_kind == RunKind::Dump {
        title(
            app,
            "DeviceInfo 덤프를 진행하고 있습니다",
            "Dumping DeviceInfo",
            "연결을 유지해 주세요. 덤프를 저장한 뒤 원본 구성으로 자동 복구합니다.",
            "Keep USB connected. After the dump is saved, the original configuration is restored automatically.",
        )
    } else {
        title(
            app,
            "진행 / 결과",
            "Progress / result",
            "펌웨어 플래싱을 진행하고 있습니다. 연결을 유지해 주세요. 단계가 끝나면 자동으로 넘어갑니다.",
            "Firmware flashing in progress. Keep USB connected; each step advances automatically.",
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
        match app.run_kind {
            RunKind::Restore => app.t("원본 복구를 마쳤습니다", "Original backup restored"),
            RunKind::Edit => app.t("DeviceInfo 수정을 마쳤습니다", "DeviceInfo edit completed"),
            RunKind::Dump => app.t("DeviceInfo 덤프 완료", "DeviceInfo dump complete"),
            _ => app.t("펌웨어 플래싱을 마쳤습니다", "Firmware flashing completed"),
        }
    } else {
        app.t("작업이 중단되었습니다", "Operation stopped")
    };
    let header: Element<'_, Message> = if matches!(app.run_kind, RunKind::Convert | RunKind::Resume)
    {
        column![
            label(flash_steps(app)[2]).size(28),
            label(heading).size(14).color(muted())
        ]
        .spacing(4)
        .into()
    } else {
        label(heading).size(28).into()
    };
    let mut content = column![header, overall_progress(app, false)].spacing(20);
    match &app.result {
        Some(Ok(warnings)) => {
            content = content.push(infobar(Severity::Success, match app.run_kind {
                RunKind::Restore => app.t("원래 리전과 원본 구성을 확인·복구한 뒤 시스템 부팅을 요청했습니다. 경고와 기기의 실제 부팅 상태를 확인해 주세요.", "Original region and configuration were verified/restored, then system boot was requested. Check warnings and actual device boot."),
                RunKind::Edit => app.t("DeviceInfo 변경과 원본 구성 복원을 마쳤습니다. 리전을 바꾼 경우 펌웨어도 설치했습니다. 기기의 실제 부팅을 확인해 주세요.", "DeviceInfo changes and the original configuration restore are complete. Firmware was installed if the region changed. Check actual device boot."),
                RunKind::Dump => app.t("DeviceInfo를 저장하고 원본 구성으로 복구한 뒤 시스템 부팅을 요청했습니다. 기기의 실제 부팅을 확인해 주세요.", "DeviceInfo was saved and the original configuration restored, then system boot was requested. Check actual device boot."),
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
    let open_row = |name: &'static str, path: &std::path::PathBuf| {
        group_row(
            name,
            label(short_path(path, value_width(app, 110.0)))
                .size(14)
                .color(muted())
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
            Some(
                action(
                    app.t("폴더 열기", "Open folder"),
                    Message::OpenFolder(path.clone()),
                    true,
                )
                .style(soft_style)
                .into(),
            ),
        )
    };
    let mut places = vec![];
    if app.result.as_ref().is_some_and(Result::is_ok)
        && let Some(folder) = &app.dump_folder
    {
        places.push(open_row(app.t("덤프 폴더", "Dump folder"), folder));
    }
    if let Some(path) = &app.backup_path {
        places.push(open_row(app.t("백업", "Backup"), path));
    }
    if let Some(path) = &app.log_path {
        places.push(open_row(app.t("자동 저장 로그", "Saved transcript"), path));
    }
    if !places.is_empty() {
        content = content.push(group(places));
    }
    content = content.push(logs(app));
    let export = action(
        app.t("로그 다른 곳에 저장", "Export log"),
        Message::Pick(Picker::Export),
        app.log_path.is_some() || app.demo,
    );
    let home = action(app.t("처음으로", "Home"), Message::Home, true);
    // A finished run needs no restore; Home becomes the main action.
    let footer = if success {
        row![space().width(Fill), export.style(soft_style), home]
    } else {
        row![
            space().width(Fill),
            home.style(soft_style),
            action(app.t("원본 복구", "Recovery"), Message::Recovery, true).style(recovery_style),
            export
        ]
    };
    (content.into(), footer.spacing(10).into())
}
/// DeviceInfo edit setup. Values are chosen later from the live DeviceInfo
/// read in TWRP; only the EDL loader is needed now.
/// A group row with a chosen path, shortened to fit, and its picker button.
fn path_row<'a>(
    app: &'a App,
    name: &'static str,
    path: Option<&std::path::Path>,
    button: &'static str,
    picker: Picker,
) -> Element<'a, Message> {
    group_row(
        name,
        label(path_value(app, path, value_width(app, 120.0)))
            .size(14)
            .color(muted())
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
        Some(action(button, Message::Pick(picker), !app.busy()).into()),
    )
}
/// The EDL loader row shared by the DeviceInfo edit and dump pages.
fn loader_row(app: &App) -> Element<'_, Message> {
    path_row(
        app,
        app.t("EDL 로더", "EDL loader"),
        app.loader.as_deref(),
        app.t("파일 선택", "Choose file"),
        Picker::Loader,
    )
}
/// DeviceInfo dump setup: EDL loader, save location and consent.
fn dump(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let content = column![
        title(
            app,
            "DeviceInfo 덤프",
            "Dump DeviceInfo",
            "TWRP에서 DeviceInfo를 읽어 파일로 저장합니다. 기기에는 쓰지 않습니다.",
            "Read DeviceInfo in TWRP and save it to a file. Nothing is written to the device."
        ),
        group(vec![
            loader_row(app),
            path_row(
                app,
                app.t("저장 위치", "Save location"),
                app.preferences.dump_root.as_deref(),
                app.t("폴더 선택", "Choose folder"),
                Picker::DumpRoot,
            ),
        ]),
        small(app.t(
            "저장 위치에 Deviceinfo_dump_<모델>_<유닉스 시간> 폴더를 만들고 deviceinfo.bin(4096바이트)과 info.json을 저장합니다.",
            "A Deviceinfo_dump_<model>_<unix time> folder with deviceinfo.bin (4096 bytes) and info.json is created in the save location.",
        )),
        note(
            app.t(
                "원본 백업 → TWRP 부팅 → DeviceInfo 읽기·저장 → 원본 구성 복원 → 재부팅 순서로 자동 진행됩니다. 작업이 끝날 때까지 USB 연결을 유지하세요.",
                "Back up originals → boot TWRP → read and save DeviceInfo → restore the original configuration → reboot, all automatically. Keep USB connected until it finishes.",
            ),
            false,
        ),
        checkbox(app.agreed)
            .label(app.t(
                "위 안내를 확인했으며 DeviceInfo 덤프를 진행하겠습니다.",
                "I have read the instructions and want to dump DeviceInfo.",
            ))
            .on_toggle(Message::Agree),
    ]
    .spacing(16);
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.t("이전", "Back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(
                    app.t("덤프 시작", "Start dump"),
                    Message::Start,
                    app.can_start(),
                )
                .into(),
            ],
        ),
    )
}
fn edit(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let content = column![
        title(
            app,
            "DeviceInfo 수정",
            "Edit DeviceInfo",
            "기기의 DeviceInfo를 TWRP에서 읽은 뒤 원하는 값을 직접 수정합니다.",
            "Read the device's DeviceInfo in TWRP, then edit the values directly."
        ),
        group(vec![loader_row(app)]),
    ]
    .spacing(16)
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
        start_footer(
            app,
            [
                action(app.t("이전", "Back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(
                    app.t("DeviceInfo 수정 시작", "Start DeviceInfo edit"),
                    Message::Start,
                    app.can_start(),
                )
                .into(),
            ],
        ),
    )
}
fn recovery(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![
        title(
            app,
            "원본 복구",
            "Restore original",
            "백업한 기기 한 대를 EDL로 연결하세요.",
            "Connect the backed-up device in EDL mode."
        ),
        group(vec![
            path_row(
                app,
                app.t("백업 폴더", "Backup folder"),
                app.recovery_folder.as_deref(),
                app.t("폴더 선택", "Choose folder"),
                Picker::Backup,
            ),
            loader_row(app),
        ]),
    ]
    .spacing(16);
    if app.device.unsupported_model() {
        content = content.push(dashboard_view::connection_note(app));
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
