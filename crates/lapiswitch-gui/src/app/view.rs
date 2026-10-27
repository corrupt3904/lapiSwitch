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
    heading: &'static str,
    description: &'static str,
) -> Element<'a, Message> {
    column![
        label(app.tr(heading)).size(28),
        label(app.tr(description)).size(14).color(muted())
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
        Page::Bootloader => 7,
        Page::Recovery => 3,
        Page::Settings => 4,
        Page::Dump => 5,
        Page::Parse | Page::ParseView => 6,
        Page::Progress | Page::Result => match app.run_kind {
            RunKind::Convert => 1,
            RunKind::Edit => 2,
            RunKind::Bootloader => 7,
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
            app.tr("view_firmware_flashing"),
            Message::Convert,
            idle && writable,
            false
        ),
        item(
            7,
            glyph::LOCK,
            app.tr("view_bootloader_unlock_relock"),
            Message::Bootloader,
            idle && writable,
            false
        ),
        item(
            2,
            glyph::EDIT,
            app.tr("view_edit_deviceinfo"),
            Message::Edit,
            idle && writable,
            false
        ),
        item(
            5,
            glyph::SAVE,
            app.tr("view_dump_deviceinfo"),
            Message::Dump,
            idle && supported,
            false
        ),
        // Reads a saved file only, so it needs no connected device.
        item(
            6,
            glyph::SEARCH,
            app.tr("view_parse_deviceinfo"),
            Message::Parse,
            idle,
            false
        ),
        space().height(Fill),
        container(divider).padding([6, 6]),
        item(
            3,
            glyph::HISTORY,
            app.tr("view_restore_original"),
            Message::Recovery,
            idle,
            true
        ),
        item(
            4,
            glyph::SETTINGS,
            app.tr("view_settings"),
            Message::Settings,
            idle,
            false
        ),
        container(small(format!("v{} · GPL-3.0", env!("CARGO_PKG_VERSION"))).size(12))
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
        app.tr("view_firmware_check"),
        app.tr("view_final_check"),
        app.tr("view_progress_result"),
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
        lapiswitch_core::deviceinfo::RollbackMode::Reset => app.tr("view_reset_default").into(),
        lapiswitch_core::deviceinfo::RollbackMode::Keep => app.tr("view_keep").into(),
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
            app.tr("view_reset_default"),
            app.tr("view_all_32_are_written_as_0"),
        ),
        (
            RollbackMode::Keep,
            app.tr("view_keep"),
            app.tr("view_values_are_kept_if_the_firmware"),
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
            label(app.tr("view_choose_rollback_handling")).size(25),
            choices,
            row![
                space().width(Fill),
                action(app.tr("archive_cancel"), Message::RollbackCancel, true).style(soft_style),
                action(app.tr("country_select"), Message::RollbackApply, true)
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
                label(app.tr("view_about_the_region_change")).size(25)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            label(app.tr("view_this_tool_works_by_changing_the")).size(15),
            label(app.tr("view_if_you_installed_global_firmware_on")).size(15),
            row![
                space().width(Fill),
                action(app.tr("archive_confirm"), Message::RegionNoticeClose, true)
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
        .unwrap_or_else(|| app.tr("view_not_selected").into())
}
/// The chosen country code for the flashing options and final check.
fn country_value(app: &App) -> String {
    match app.country.as_deref() {
        None => app.tr("view_choose").into(),
        Some("—") => app.tr("country_do_not_change").into(),
        Some(code) => match crate::countries::find(code) {
            Some(country) => format!("{} · {}", country.code, country.name),
            None => code.into(),
        },
    }
}
/// The start footer: a problem in an InfoBar right above the buttons, or
/// a step not done yet as a muted hint beside them.
fn start_footer<'a>(app: &'a App, buttons: [Element<'a, Message>; 2]) -> Element<'a, Message> {
    issue_footer(app.start_issue(), buttons)
}
/// `start_footer` for a given reason, e.g. the original restore's.
fn issue_footer<'a>(
    issue: Option<(Option<Severity>, &'static str)>,
    buttons: [Element<'a, Message>; 2],
) -> Element<'a, Message> {
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
    let flow = label(app.tr("view_firmware_flashing_region_change"))
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
        Page::Edit
        | Page::Bootloader
        | Page::Dump
        | Page::Parse
        | Page::ParseView
        | Page::Recovery => true,
        Page::Progress | Page::Result => matches!(
            app.run_kind,
            RunKind::Edit | RunKind::Bootloader | RunKind::Dump | RunKind::Restore
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
        body = body.push(notification_note(
            app.tr("view_preview_no_device_connections_or_file"),
        ));
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
                    label(app.tr("view_an_interrupted_run_was_found")),
                    small(format!(
                        "{} · {:?}",
                        request_label(app, &pending.state),
                        pending.state.stage
                    ))
                ]
                .spacing(3)
                .width(Fill),
                action(app.tr("view_review_run"), Message::Resume, !app.busy()),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ));
    }
    if let Some(error) = &app.startup_error {
        body = body.push(notification_note(format!(
            "{}\n{error}",
            app.tr("view_saved_state_needs_attention_before_a")
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
        Page::Bootloader => bootloader(app),
        Page::Dump => dump(app),
        Page::Parse => parse_page(app),
        Page::ParseView => parse_result_page(app),
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
        Some(
            panel(
                column![
                    label(app.tr("view_close_the_running_operation")).size(24),
                    label(if app.activity == Some(Activity::Install) {
                        app.tr("view_the_driver_installer_is_running_please")
                    } else {
                        app.tr("view_closing_during_writes_can_leave_the")
                    }),
                    row![
                        space().width(Fill),
                        action(app.tr("view_keep_running"), Message::Stay, true),
                        action(app.tr("view_close_app"), Message::Exit, true)
                            .style(super::fluent::danger_style)
                    ]
                    .spacing(12),
                ]
                .spacing(20),
            )
            .max_width(560),
        )
    } else if app.firmware_confirmation.is_some() {
        Some(
            panel(
                column![
                    label(app.tr("view_stock_firmware_could_not_be_verified"))
                        .size(25)
                        .color(danger()),
                    label(app.tr("view_the_device_firmware_could_not_pass")).color(danger()),
                    row![
                        space().width(Fill),
                        action(
                            app.tr("archive_cancel"),
                            Message::ConfirmUnrecognizedFirmware(false),
                            true
                        )
                        .style(soft_style),
                        action(
                            app.tr("view_continue"),
                            Message::ConfirmUnrecognizedFirmware(true),
                            true
                        ),
                    ]
                    .spacing(12),
                ]
                .spacing(20),
            )
            .style(fluent::warning_dialog_style)
            .max_width(620),
        )
    } else if let Some((details, _)) = &app.rollback_confirmation {
        Some(
            panel(
                column![
                    label(app.tr("view_rollback_index_warning"))
                        .size(25)
                        .color(danger()),
                    label(app.tr("view_the_firmware_s_rollback_indices_are")).color(danger()),
                    small(details.clone()),
                    label(app.tr("view_reset_to_0_and_continue_writes")),
                    label(app.tr("view_cancel_leaves_deviceinfo_unchanged_switches_to")),
                    row![
                        space().width(Fill),
                        action(
                            app.tr("editor_cancel_restore_originals"),
                            Message::ConfirmRollbackReset(false),
                            true
                        )
                        .style(soft_style),
                        action(
                            app.tr("view_reset_to_0_and_continue"),
                            Message::ConfirmRollbackReset(true),
                            true
                        )
                        .style(super::fluent::danger_style),
                    ]
                    .spacing(12),
                ]
                .spacing(20),
            )
            .style(fluent::warning_dialog_style)
            .max_width(660),
        )
    } else if let Some(editor) = &app.editor {
        // The country picker and a field's help open over the editor, which
        // stays visible behind them. The editor keeps the same place in the
        // widget tree either way, so its scroll position survives.
        let over = if app.country_open {
            Some(country_view::modal(app))
        } else {
            super::editor::unlock_modal(app, editor)
                .or_else(|| super::editor::help_modal(app, editor))
        };
        Some(
            container(stack![
                container(super::editor::modal(app, editor)).center(Fill),
                overlay(over),
            ])
            .width(Fill)
            .height(Fill),
        )
    } else if app.edl_confirmation.is_some() {
        Some(
            panel(
                column![
                    label(app.tr("view_start_from_edl")).size(25),
                    label(app.tr("view_the_device_must_be_able_to")),
                    label(app.tr("view_select_confirm_if_normal_boot_is")),
                    row![
                        space().width(Fill),
                        action(
                            app.tr("archive_cancel"),
                            Message::ConfirmEdlStart(false),
                            true
                        )
                        .style(soft_style),
                        action(
                            app.tr("archive_confirm"),
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
        Some(
            panel(
                column![
                    small("TWRP · DeviceInfo"),
                    label(app.tr("view_before_rebooting")).size(25),
                    label(transition_text(app)),
                    label(app.tr("view_keep_the_tablet_connected_to_the")).color(danger()),
                    row![
                        space().width(Fill),
                        small(app.tr("view_confirms_automatically")),
                        reboot_countdown(app),
                        action(
                            app.tr("archive_confirm"),
                            Message::ConfirmRecoveryReboot,
                            true
                        )
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                ]
                .spacing(20),
            )
            .style(fluent::warning_dialog_style)
            .max_width(620),
        )
    } else if let Some(ticket) = app.stop_confirmation {
        Some(wait_ui::modal(app, ticket))
    } else if app.archive_confirmation.is_some() {
        Some(archive::modal(app))
    } else if app.fix.confirm {
        Some(conflicts::modal(app))
    } else if app.country_open {
        Some(country_view::modal(app))
    } else if let Some(help) = app
        .parsed
        .as_ref()
        .and_then(|parsed| parsed.view.as_ref())
        .and_then(|(_, view)| super::editor::help_modal(app, view))
    {
        Some(help)
    } else if app.rollback_open {
        Some(rollback_modal(app))
    } else if app.region_notice_open {
        Some(region_notice(app))
    } else if let Some(agreed) = app.task_notice {
        Some(task_notice(app, agreed))
    } else if app.license_open {
        Some(licenses_view::modal(app))
    } else {
        None
    };
    // The page keeps the same place in the widget tree with or without a
    // dialog, so its scroll position survives one opening and closing.
    stack![root, overlay(modal)].into()
}
/// The seconds left before a reboot prompt confirms itself, in a ring that
/// fills clockwise from twelve o'clock.
fn reboot_countdown(app: &App) -> Element<'_, Message> {
    const SIZE: f32 = 34.0;
    let Some((deadline, now)) = app.reboot_deadline else {
        return space().into();
    };
    let left = deadline.saturating_duration_since(now);
    let progress = 1.0 - left.as_secs_f32() / REBOOT_AUTO_CONFIRM.as_secs_f32();
    let ring = countdown_svg(progress, line(), accent());
    stack![
        widget::svg(widget::svg::Handle::from_memory(ring.into_bytes()))
            .width(SIZE)
            .height(SIZE),
        container(label(left.as_secs_f32().ceil().to_string()).size(14))
            .center_x(SIZE)
            .center_y(SIZE),
    ]
    .into()
}
/// A ring in `base` with the elapsed part, `progress` of it, in `fill`.
fn countdown_svg(progress: f32, base: Color, fill: Color) -> String {
    let hex = |color: Color| {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02X}{g:02X}{b:02X}")
    };
    // In steps, so a countdown renders a bounded set of images.
    let progress = (progress.clamp(0.0, 1.0) * 120.0).round() / 120.0;
    let ring = |color: &str| {
        format!(r#"<circle cx="12" cy="12" r="10" fill="none" stroke="{color}" stroke-width="2"/>"#)
    };
    let elapsed = if progress >= 1.0 {
        ring(&hex(fill))
    } else if progress <= 0.0 {
        String::new()
    } else {
        let angle = progress * std::f32::consts::TAU;
        let (x, y) = (12.0 + 10.0 * angle.sin(), 12.0 - 10.0 * angle.cos());
        let large = u8::from(angle > std::f32::consts::PI);
        format!(
            r#"<path d="M12 2A10 10 0 {large} 1 {x:.3} {y:.3}" fill="none" stroke="{}" stroke-width="2.4" stroke-linecap="round"/>"#,
            hex(fill)
        )
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">{}{elapsed}</svg>"#,
        ring(&hex(base))
    )
}
/// A dialog over the page with the scrim behind it, or nothing.
fn overlay<'a>(dialog: Option<widget::Container<'a, Message>>) -> Element<'a, Message> {
    match dialog {
        Some(dialog) => opaque(container(dialog).center(Fill).style(|_| container::Style {
            background: Some(Background::Color(fluent::p().scrim)),
            ..Default::default()
        })),
        None => space().into(),
    }
}
pub(super) fn footer<'a>(app: &App, next: Option<(Message, bool)>) -> Element<'a, Message> {
    let mut buttons = row![
        space().width(Fill),
        action(app.tr("editor_back"), Message::Back, !app.busy()).style(soft_style),
    ]
    .spacing(12);
    if let Some((message, enabled)) = next {
        buttons = buttons.push(action(
            app.tr("view_continue_2"),
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
            label(app.tr("view_choose_the_firmware_folder_the_model"))
                .size(14)
                .color(muted())
        ]
        .spacing(4),
        group(vec![group_row(
            app.tr("view_firmware_folder"),
            label(folder_value(app, value_width(app, 120.0)))
                .size(14)
                .color(muted())
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
            Some(
                action(
                    app.tr("settings_choose_folder"),
                    Message::Pick(Picker::Firmware),
                    !app.busy()
                )
                .into()
            ),
        )]),
    ]
    .spacing(12);
    if app.busy() {
        content = content.push(note(app.tr("view_checking_firmware"), false));
    }
    if let Some(report) = &app.report {
        content = content
            .push(infobar(
                Severity::Success,
                app.tr("view_firmware_check_passed_flashing_changes_the"),
            ))
            .push(group(vec![
                group_row(
                    app.tr("view_region_vendor_boot_dtb"),
                    label(report.region.token()).size(18).color(accent()).into(),
                    None,
                ),
                group_row(
                    app.tr("view_model"),
                    label(report.device.model()).size(14).into(),
                    None,
                ),
                group_row(
                    "Build fingerprint",
                    label(report.fingerprint.clone())
                        .size(13)
                        .color(muted())
                        .into(),
                    None,
                ),
                group_row(
                    app.tr("view_signing_key"),
                    label(format!("{:?}", report.key_class)).size(14).into(),
                    None,
                ),
                group_row(
                    app.tr("view_rollback_indices"),
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
                    app.tr("view_these_missing_images_will_be_skipped"),
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
    content = content.push(small(app.tr("view_encrypted_x_files_are_decrypted_to")));
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
        return app.tr("view_reboot_to_twrp_deviceinfo_is_read");
    }
    if app.run_kind == RunKind::Bootloader {
        return match app.recovery_transition {
            Some(Transition::Twrp) => app.tr("view_reboot_to_twrp_bootloader_flags"),
            Some(Transition::Edl) => app.tr("view_bootloader_flags_verified_enter_edl"),
            None => app.tr("view_restore_the_original_configuration_after_bootloader"),
        };
    }
    let edit = app.run_kind == RunKind::Edit;
    match (app.recovery_transition, edit) {
        (Some(Transition::Twrp), false) => app.tr("view_reboot_to_twrp_to_back_up"),
        (Some(Transition::Twrp), true) => app.tr("view_reboot_to_twrp_deviceinfo_is_read_2"),
        (Some(Transition::Edl), false) => {
            app.tr("view_deviceinfo_verified_enter_edl_restore_original")
        }
        (Some(Transition::Edl), true) => app.tr("view_deviceinfo_verified_enter_edl_and_restore"),
        (None, false) => app.tr("view_restore_the_original_configuration_after_twrp"),
        (None, true) => app.tr("view_restore_the_original_configuration_after_twrp_2"),
    }
}
/// One line naming what a saved run changes.
pub(super) fn request_label(app: &App, state: &lapiswitch_core::state::RunState) -> String {
    match state.request {
        Request::Convert { target, .. } => {
            format!("{} · {}", app.tr("view_firmware_flashing"), target.token())
        }
        Request::Dump => app.tr("view_deviceinfo_dump").into(),
        Request::Bootloader { unlock: true } => app.tr("view_bootloader_unlock_task").into(),
        Request::Bootloader { unlock: false } => app.tr("view_bootloader_relock_task").into(),
        Request::Edit => match &state.firmware {
            Some(binding) => format!(
                "{} · {}",
                app.tr("view_deviceinfo_edit"),
                binding.region.token()
            ),
            None => app.tr("view_deviceinfo_edit").into(),
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
            app.tr("view_target_region"),
            region.map_or("—", Region::token)
        ),
        field(app.tr("view_user_data"), app.tr("view_erase")),
        field(
            app.tr("editor_country_code"),
            country
                .filter(|c| *c != "—")
                .unwrap_or(app.tr("view_unchanged"))
        ),
        field(app.tr("view_firmware_folder"), firmware)
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
            label(app.tr("view_connect_one_device_and_keep_usb"))
                .size(14)
                .color(muted())
        ]
        .spacing(4),
        warning_note(app.tr("view_firmware_flashing_always_erases_user_data")),
        group(vec![
            group_row(
                app.tr("view_target_region"),
                row![
                    label(app.region.map_or("—", Region::token))
                        .size(18)
                        .color(accent()),
                    label(app.tr("view_from_the_firmware_s_vendor_boot"))
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
                app.tr("view_user_data"),
                label(app.tr("view_erased")).size(18).color(danger()).into(),
                None,
            ),
            link_row(
                app.tr("editor_country_code"),
                country_value(app),
                Message::CountryOpen,
                idle,
            ),
            link_row(
                app.tr("view_firmware_folder"),
                folder_value(app, value_width(app, 30.0)),
                Message::Pick(Picker::Firmware),
                idle,
            ),
            link_row(
                app.tr("view_rollback_indices"),
                rollback_value(app),
                Message::RollbackOpen,
                idle,
            ),
            group_row(
                app.tr("view_backup_location"),
                label(short_path(
                    &app.output.join("backup").join("firmware_flashing"),
                    value_width(app, 150.0),
                ))
                .size(14)
                .color(muted())
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
                Some(small(app.tr("view_change_in_settings")).into()),
            ),
        ]),
        checkbox(app.agreed)
            .label(app.tr("view_i_understand_that_user_data_will"))
            .on_toggle(Message::Agree),
    ]
    .spacing(12);
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.tr("editor_back"), Message::Back, true)
                    .style(soft_style)
                    .into(),
                action(
                    if app.activity == Some(Activity::Archive) {
                        app.tr("view_archiving")
                    } else if app.deferred.is_some() {
                        app.tr("dashboard_checking_connection")
                    } else {
                        app.tr("view_start_flashing")
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
            label(app.tr("view_overall_progress")).size(15).width(Fill),
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
        content = content.push(small(
            app.tr("view_weighted_by_expected_work_firmware_installation"),
        ));
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
            "view_restoration_in_progress",
            "view_keep_usb_connected_each_step_advances",
        )
    } else if app.run_kind == RunKind::Edit {
        title(
            app,
            "view_editing_deviceinfo",
            "view_keep_usb_connected_each_step_advances",
        )
    } else if app.run_kind == RunKind::Bootloader && app.bootloader == Some(true) {
        title(
            app,
            "view_unlocking_bootloader",
            "view_keep_usb_connected_each_step_advances",
        )
    } else if app.run_kind == RunKind::Bootloader {
        title(
            app,
            "view_relocking_bootloader",
            "view_keep_usb_connected_each_step_advances",
        )
    } else if app.run_kind == RunKind::Dump {
        title(
            app,
            "view_dumping_deviceinfo",
            "view_keep_usb_connected_after_the_dump",
        )
    } else {
        title(
            app,
            "view_progress_result",
            "view_firmware_flashing_in_progress_keep_usb",
        )
    };
    let mut content = column![heading].spacing(18);
    if app.run_kind == RunKind::Restore {
        content = content.push(note(
            app.tr("view_started_in_twrp_deviceinfo_is_read"),
            false,
        ));
    }
    content = content.push(overall_progress(app, true));
    if let Some(detail) = &app.recovery_detail {
        content = content.push(note(detail.clone(), false));
    }
    if !app.comparison.is_empty() {
        let mut table = column![row![
            small(app.tr("view_image")).width(Fill),
            small(app.tr("view_device_rollback_index")).width(Fill),
            small(app.tr("view_firmware_rollback_index")).width(Fill)
        ]]
        .spacing(8);
        // Presentation only; the core has already made the rollback decision.
        for (image, firmware, device) in &app.comparison {
            table = table.push(row![
                label(image.as_str()).size(13).width(Fill),
                label(device.as_str()).size(13).width(Fill),
                label(firmware.as_str()).size(13).width(Fill)
            ]);
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
            app.tr("view_firmware_transfer"),
            if progress.operation_total_bytes > 0 {
                format!(
                    "{:.0}%",
                    (progress.operation_completed_bytes as f64
                        / progress.operation_total_bytes as f64
                        * 100.0)
                        .clamp(0.0, 100.0)
                )
            } else {
                app.tr("view_calculating").into()
            }
        )));
    }
    content = content.push(logs(app));
    (
        content.into(),
        row![
            small(app.tr("view_live_logs_are_saved_automatically_keep")).width(Fill),
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
            RunKind::Restore => app.tr("view_original_backup_restored"),
            RunKind::Edit => app.tr("view_deviceinfo_edit_completed"),
            RunKind::Bootloader if app.bootloader == Some(true) => {
                app.tr("view_bootloader_unlock_completed")
            }
            RunKind::Bootloader => app.tr("view_bootloader_relock_completed"),
            RunKind::Dump => app.tr("view_deviceinfo_dump_complete"),
            _ => app.tr("view_firmware_flashing_completed"),
        }
    } else {
        app.tr("view_operation_stopped")
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
            content = content.push(infobar(
                Severity::Success,
                match app.run_kind {
                    RunKind::Restore => {
                        app.tr("view_original_region_and_configuration_were_verified")
                    }
                    RunKind::Edit => {
                        app.tr("view_deviceinfo_changes_and_the_original_configuration")
                    }
                    RunKind::Bootloader => app.tr("view_bootloader_and_the_original_configuration"),
                    RunKind::Dump => app.tr("view_deviceinfo_was_saved_and_the_original"),
                    _ => app.tr("view_firmware_writes_and_final_steps_are"),
                },
            ));
            for warning in warnings {
                content = content.push(note(warning, true));
            }
        }
        Some(Err(error)) => {
            content = content
                .push(error_note(error))
                .push(small(app.tr("view_review_the_backup_and_saved_run")));
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
                    app.tr("view_open_folder"),
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
        places.push(open_row(app.tr("view_dump_folder"), folder));
    }
    if let Some(path) = &app.backup_path {
        places.push(open_row(app.tr("view_backup"), path));
    }
    if let Some(path) = &app.log_path {
        places.push(open_row(app.tr("view_saved_transcript"), path));
    }
    if !places.is_empty() {
        content = content.push(group(places));
    }
    content = content.push(logs(app));
    let export = action(
        app.tr("view_export_log"),
        Message::Pick(Picker::Export),
        app.log_path.is_some() || app.demo,
    );
    let home = action(app.tr("view_home"), Message::Home, true);
    // A finished run needs no restore; Home becomes the main action.
    let footer = if success {
        row![space().width(Fill), export.style(soft_style), home]
    } else {
        row![
            space().width(Fill),
            home.style(soft_style),
            action(app.tr("view_recovery"), Message::Recovery, true).style(recovery_style),
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
        app.tr("view_edl_loader"),
        app.loader.as_deref(),
        app.tr("view_choose_file"),
        Picker::Loader,
    )
}
/// The recently parsed DeviceInfo files: a row opens its file, × removes
/// it from the list. A file that is gone can only be removed.
fn recent_deviceinfo(app: &App) -> Option<Element<'_, Message>> {
    let recent = &app.preferences.recent_deviceinfo;
    if recent.is_empty() {
        return None;
    }
    let width = value_width(app, 0.0) + FIELD_KEY - 80.0;
    let mut rows = Vec::new();
    for path in recent {
        let exists = path.is_file();
        let mut line = row![
            icon(glyph::DOCUMENT)
                .size(14)
                .color(if exists { muted() } else { line() }),
            container(
                label(short_path(path, width))
                    .size(13)
                    .color(if exists { ink() } else { muted() })
                    .wrapping(iced::widget::text::Wrapping::None)
            )
            .width(Fill)
            .clip(true),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if !exists {
            line = line.push(small(app.tr("view_recent_file_missing")).color(danger()));
        }
        let open = button(line)
            .padding([11, 16])
            .width(Fill)
            .on_press_maybe((exists && !app.busy()).then(|| Message::ParseRecent(path.clone())))
            .style(|theme, status| {
                let mut style = fluent::subtle_style(theme, status);
                style.border.radius = 0.0.into();
                style
            });
        let remove = button(icon(glyph::CLOSE).size(11).color(muted()))
            .padding([8, 10])
            .on_press(Message::ParseForget(path.clone()))
            .style(fluent::subtle_style);
        rows.push(
            row![open, remove]
                .spacing(4)
                .padding(pad(0, 8, 0, 0))
                .align_y(Alignment::Center)
                .into(),
        );
    }
    Some(
        column![
            row![
                icon(glyph::HISTORY).size(12).color(muted()),
                small(app.tr("view_recent_files")),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            group(rows),
        ]
        .spacing(8)
        .into(),
    )
}
/// DeviceInfo parsing: a saved file shown field by field with the map of
/// its device, read-only.
fn parse_page(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    use lapiswitch_core::deviceinfo::parse::Source;
    let parsed = app.parsed.as_ref();
    let mut rows = vec![path_row(
        app,
        app.tr("view_deviceinfo_file"),
        parsed.map(|p| p.opened.path.as_path()),
        app.tr("view_choose_file"),
        Picker::DeviceInfoFile,
    )];
    if let Some(parsed) = parsed {
        // The model is always determined automatically.
        let model: Element<'_, Message> = match &parsed.view {
            Some((source, view)) => row![
                label(view.layout.model()).size(14),
                small(match source {
                    Source::Record => app.tr("view_model_from_record"),
                    Source::Detected => app.tr("view_model_detected"),
                }),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            None => label(app.tr("view_not_determined"))
                .size(14)
                .color(muted())
                .into(),
        };
        rows.push(group_row(app.tr("view_model"), model, None));
        rows.push(group_row(
            "SHA-256",
            label(parsed.opened.sha256.clone())
                .size(13)
                .color(muted())
                .into(),
            None,
        ));
    }
    let mut content = column![
        title(app, "view_parse_deviceinfo", "view_parse_description"),
        group(rows),
    ]
    .spacing(16)
    .extend(recent_deviceinfo(app));
    let mut ready = false;
    if let Some(parsed) = parsed {
        if parsed.view.is_some() {
            ready = true;
        } else {
            content = content.push(error_note(app.tr("view_model_not_detected")));
        }
    }
    (
        content.into(),
        issue_footer(
            None,
            [
                action(app.tr("editor_back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(app.tr("archive_confirm"), Message::ParseShow, ready).into(),
            ],
        ),
    )
}
/// The fields of the parsed file, read-only, selectable and searchable.
fn parse_result_page(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    use lapiswitch_core::deviceinfo::parse::Source;
    let Some((parsed, (source, view))) = app
        .parsed
        .as_ref()
        .and_then(|parsed| parsed.view.as_ref().map(|view| (parsed, view)))
    else {
        return (space().into(), footer(app, None));
    };
    let source = match source {
        Source::Record => app.tr("view_model_from_record"),
        Source::Detected => app.tr("view_model_detected"),
    };
    let name = parsed
        .opened
        .path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut content = column![
        column![
            label(app.tr("view_parse_deviceinfo")).size(28),
            label(format!("{name} · {} · {source}", view.layout.model()))
                .size(14)
                .color(muted()),
        ]
        .spacing(4)
    ]
    .spacing(16);
    content = content
        .push(
            iced::widget::text_input(app.tr("editor_search_fields"), &view.query)
                .on_input(|text| Message::Editor(super::editor::EditorMessage::Query(text)))
                .padding(8),
        )
        .push(super::editor::field_list(app, view));
    (content.into(), footer(app, None))
}
/// DeviceInfo dump setup: EDL loader, save location and consent.
fn dump(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let content = column![
        title(
            app,
            "view_dump_deviceinfo",
            "view_read_deviceinfo_in_twrp_and_save"
        ),
        group(vec![
            loader_row(app),
            path_row(
                app,
                app.tr("view_save_location"),
                app.preferences.dump_root.as_deref(),
                app.tr("settings_choose_folder"),
                Picker::DumpRoot,
            ),
        ]),
        small(app.tr("view_a_deviceinfo_dump_model_unix_time")),
        note(app.tr("view_back_up_originals_boot_twrp_read"), false,),
        checkbox(app.agreed)
            .label(app.tr("view_i_have_read_the_instructions_and"))
            .on_toggle(Message::Agree),
    ]
    .spacing(16);
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.tr("editor_back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(app.tr("view_start_dump"), Message::Start, app.can_start()).into(),
            ],
        ),
    )
}
fn edit(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let content = column![
        title(
            app,
            "view_edit_deviceinfo",
            "view_read_the_device_s_deviceinfo_in"
        ),
        group(vec![loader_row(app)]),
        notice_agreement(app),
    ]
    .spacing(16);
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.tr("editor_back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(
                    app.tr("view_start_deviceinfo_edit"),
                    Message::Start,
                    app.can_start(),
                )
                .into(),
            ],
        ),
    )
}
/// The risks of a DeviceInfo edit or a bootloader unlock or relock, agreed
/// to over its page.
fn task_notice(app: &App, agreed: bool) -> iced::widget::Container<'_, Message> {
    let (heading, lines, scope, agreement) = if app.page == Page::Edit {
        (
            app.tr("view_before_editing_deviceinfo"),
            app.tr("view_deviceinfo_edit_risks"),
            app.tr("view_deviceinfo_edit_what_the_task_does"),
            app.tr("view_deviceinfo_edit_agree"),
        )
    } else {
        (
            app.tr("view_before_changing_the_bootloader_lock"),
            app.tr("view_bootloader_risks"),
            app.tr("view_bootloader_what_the_task_does"),
            app.tr("view_bootloader_agree"),
        )
    };
    let mut risks = column![].spacing(10);
    for line in lines.lines() {
        risks = risks.push(label(line).size(15));
    }
    panel(
        column![
            row![
                icon(glyph::ALERT).size(22).color(danger()),
                label(heading).size(25)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            risks,
            small(scope),
            checkbox(agreed)
                .label(agreement)
                .on_toggle(Message::TaskNoticeAgree),
            row![
                space().width(Fill),
                action(
                    app.tr("archive_cancel"),
                    Message::TaskNoticeClose(false),
                    true
                )
                .style(soft_style),
                action(
                    app.tr("archive_confirm"),
                    Message::TaskNoticeClose(true),
                    agreed
                ),
            ]
            .spacing(12),
        ]
        .spacing(16),
    )
    .style(fluent::warning_dialog_style)
    .max_width(720)
}
/// The page's own agreement to the risks shown before it; the ? shows them
/// again and checks it when they are confirmed.
fn notice_agreement(app: &App) -> Element<'_, Message> {
    row![
        checkbox(app.agreed)
            .label(app.tr("view_agreed_to_the_notice"))
            .on_toggle(Message::Agree),
        button(icon('\u{E9CE}').size(14).color(muted()))
            .padding(2)
            .style(|_, _| button::Style::default())
            .on_press_maybe((!app.busy()).then_some(Message::TaskNoticeReview)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}
/// What the chosen unlock or relock changes on the connected device.
fn bootloader_summary(app: &App, unlock: bool) -> Element<'_, Message> {
    let d = &app.device.snapshot;
    let flag = |name: &'static str| {
        group_row(
            name,
            label(if unlock { "1" } else { "0" })
                .size(18)
                .color(accent())
                .into(),
            Some(small(if unlock { "Unlocked" } else { "Locked" }).into()),
        )
    };
    group(vec![
        group_row(
            app.tr("dashboard_device"),
            row![
                label(dashboard_view::device_name(app)).size(14),
                small(d.model.clone())
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            None,
        ),
        group_row(
            app.tr("view_user_data"),
            label(app.tr("view_erased")).size(18).color(danger()).into(),
            None,
        ),
        flag(app.tr("view_bootloader_unlock_flag")),
        flag(app.tr("view_critical_unlock_flag")),
        group_row(
            app.tr("view_backup_location"),
            label(short_path(
                &app.output
                    .join("backup")
                    .join(lapiswitch_core::backup::folder(Request::Bootloader {
                        unlock,
                    })),
                value_width(app, 150.0),
            ))
            .size(14)
            .color(muted())
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
            Some(small(app.tr("view_change_in_settings")).into()),
        ),
    ])
}
/// Bootloader unlock/relock setup: EDL loader and unlock or relock; once
/// one is chosen and a device is connected, what it changes.
fn bootloader(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let choice = |unlock: bool, name: &'static str, description: &'static str| {
        let selected = app.bootloader == Some(unlock);
        button(
            row![
                icon(if unlock { glyph::UNLOCK } else { glyph::LOCK })
                    .size(20)
                    .color(if selected { accent() } else { muted() }),
                column![label(name).size(15), small(description)].spacing(4),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding(16)
        .width(Fill)
        .on_press_maybe((!app.busy()).then_some(Message::BootloaderChoice(unlock)))
        .style(move |theme, status| {
            let mut style = button::secondary(theme, status);
            style.background = Some(if selected { pale() } else { card_color() }.into());
            style.text_color = ink();
            style.border = Border {
                color: if selected { accent() } else { line() },
                width: if selected { 2.0 } else { 1.0 },
                radius: fluent::CARD_RADIUS.into(),
            };
            style
        })
    };
    let mut content = column![
        title(
            app,
            "view_bootloader_unlock_relock",
            "view_bootloader_unlock_relock_description"
        ),
        warning_note(app.tr("view_bootloader_always_erases_user_data")),
        group(vec![loader_row(app)]),
        row![
            choice(
                true,
                app.tr("view_bootloader_unlock"),
                app.tr("view_bootloader_unlock_sets_both_flags")
            ),
            choice(
                false,
                app.tr("view_bootloader_relock"),
                app.tr("view_bootloader_relock_clears_both_flags")
            ),
        ]
        .spacing(12),
    ]
    .spacing(16);
    let connected = app.device.reachable() && app.device.snapshot.mode != crate::device::Mode::None;
    if let Some(unlock) = app.bootloader.filter(|_| connected) {
        content = content.push(bootloader_summary(app, unlock));
    }
    content = content.push(notice_agreement(app));
    let start = match app.bootloader {
        Some(true) => app.tr("view_start_bootloader_unlock"),
        Some(false) => app.tr("view_start_bootloader_relock"),
        None => app.tr("view_start_bootloader_task"),
    };
    (
        content.into(),
        start_footer(
            app,
            [
                action(app.tr("editor_back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(start, Message::Start, app.can_start()).into(),
            ],
        ),
    )
}
fn recovery(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![
        title(
            app,
            "view_restore_original",
            "view_connect_the_backed_up_device_in"
        ),
        group(vec![
            path_row(
                app,
                app.tr("view_backup_folder"),
                app.recovery_folder.as_deref(),
                app.tr("settings_choose_folder"),
                Picker::Backup,
            ),
            loader_row(app),
        ]),
    ]
    .spacing(16);
    if let Some(summary) = &app.recovery_summary {
        content = content.push(note(
            format!(
                "{}\n{summary}",
                app.tr("view_backup_files_and_hashes_verified")
            ),
            false,
        ));
    }
    if app.busy() {
        content = content.push(small(app.tr("view_verifying_backup")));
    }
    content = content
        .push(warning_note(app.tr("view_this_does_not_roll_back_the")))
        .push(
            checkbox(app.agreed)
                .label(app.tr("view_this_is_the_device_that_created"))
                .on_toggle(Message::Agree),
        );
    (
        content.into(),
        issue_footer(
            app.restore_issue(),
            [
                action(app.tr("editor_back"), Message::Back, !app.busy())
                    .style(soft_style)
                    .into(),
                action(
                    app.tr("view_restore_original_2"),
                    Message::RestoreStart,
                    app.can_restore(),
                )
                .into(),
            ],
        ),
    )
}
pub(super) fn driver_status(app: &App) -> String {
    use ltbox_device::driver::DriverStatus::*;
    if app.driver_checking {
        return app.tr("view_checking_drivers").into();
    }
    match &app.driver_status {
        Some(Present) => app.tr("view_required_drivers_are_present").into(),
        Some(NotWindows) => app.tr("view_no_additional_driver_is_required_on").into(),
        Some(Missing(files)) => format!("{}: {}", app.tr("view_missing_drivers"), files.join(", ")),
        Some(UdevRulesMissing) => app.tr("view_install_udev_rules_for_usb_access").into(),
        Some(UdevRulesStale) => app.tr("view_update_the_installed_udev_rules").into(),
        Some(UdevRulesNoPermission) => app.tr("view_check_read_permissions_for_udev_rules").into(),
        Some(KernelDriverMissing) => app.tr("view_install_the_qualcomm_kernel_driver").into(),
        Some(KernelDriverUnsupported) => app.tr("view_use_userspace_mode_on_this_platform").into(),
        None => app.tr("view_check_driver_status").into(),
    }
}
fn resume(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let mut content = column![title(
        app,
        "view_review_saved_run",
        "view_review_the_saved_settings_and_current"
    )]
    .spacing(22);
    if app.device.unsupported_model() {
        content = content.push(dashboard_view::connection_note(app));
    }
    if let Some(pending) = &app.pending {
        let state = &pending.state;
        content = content.push(field(app.tr("view_operation"), request_label(app, state)));
        content = match &state.firmware {
            Some(binding) => content.push(summary(
                app,
                Some(binding.region),
                state.options.country.as_deref(),
                binding.firmware_dir.display().to_string(),
            )),
            None => content.push(field(
                app.tr("dashboard_firmware"),
                app.tr("view_not_installed_user_data_kept"),
            )),
        };
        content = content.push(field(
            app.tr("view_backup"),
            state.backup_dir.display().to_string(),
        ));
        content = content.push(field(
            app.tr("view_saved_stage"),
            format!("{:?}", state.recovery.phase),
        ));
        if let Request::Convert { rollback_mode, .. } = state.request {
            content = content.push(field(
                app.tr("view_rollback_indices"),
                if rollback_mode == lapiswitch_core::deviceinfo::RollbackMode::Reset {
                    app.tr("view_reset_32_0")
                } else {
                    app.tr("view_keep_2")
                },
            ));
        }
        let guidance = match pending.action {
            ResumeAction::Recovery => app.tr("view_this_checkpoint_cannot_be_resumed_review"),
            ResumeAction::Flash => app.tr("view_firmware_will_be_installed_again_and"),
            ResumeAction::Finalize => app.tr("view_check_the_final_reboot_handoff_after"),
            _ => app.tr("view_recheck_the_saved_twrp_deviceinfo_stage"),
        };
        content = content.push(note(guidance, true)).push(
            checkbox(app.agreed)
                .label(app.tr("view_i_have_reviewed_the_saved_settings"))
                .on_toggle(Message::Agree),
        );
    }
    (
        content.into(),
        row![
            space().width(Fill),
            action(app.tr("editor_back"), Message::Back, !app.busy()).style(soft_style),
            action(app.tr("view_recovery"), Message::Recovery, !app.busy()).style(recovery_style),
            action(
                app.tr("view_resume"),
                Message::ResumeStart,
                app.can_resume()
            )
        ]
        .spacing(10)
        .into(),
    )
}
