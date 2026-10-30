use super::view::*;
use super::*;
use crate::device::Mode;
use iced::widget::{column, container, image, row, svg};
use iced::{Alignment, Fill};
use std::sync::{Mutex, OnceLock};

pub(super) fn status(app: &App) -> String {
    if app.device.snapshot.error.is_some() {
        return app.tr("dashboard_connection_check_failed").into();
    }
    if app.device.checked.is_none() {
        return app.tr("dashboard_checking_connection").into();
    }
    if !app.demo
        && app
            .device
            .checked
            .is_some_and(|t| t.elapsed() >= crate::device::MAX_AGE)
    {
        return app.tr("dashboard_rechecking_connection").into();
    }
    match app.device.snapshot.mode {
        Mode::None => app.tr("dashboard_no_device_connected"),
        Mode::Adb => "ADB",
        Mode::Recovery => "ADB · Recovery",
        Mode::Unauthorized => app.tr("dashboard_adb_unauthorized"),
        Mode::Sideload => "ADB · Sideload",
        Mode::ServerBlocking => app.tr("dashboard_adb_server_conflict"),
        Mode::Fastboot => "FASTBOOT",
        Mode::Fastbootd => "FASTBOOTD",
        Mode::Edl => "EDL · 9008",
        Mode::Usb => app.tr("dashboard_adb_usb_in_use"),
    }
    .into()
}
pub(super) fn badge(app: &App) -> Element<'_, Message> {
    let p = super::fluent::p();
    let color = if app.device.reachable() {
        p.success
    } else {
        p.text_secondary
    };
    container(
        row![
            label("●").size(10).color(color),
            label(status(app)).size(12)
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 10])
    .style(pill_style)
    .into()
}
/// devices/<MODEL>/image.png of the connected supported device, or a
/// generic tablet.
fn portrait(app: &App, width: f32, height: f32) -> Element<'_, Message> {
    if let Some(bytes) = app.device.supported().and_then(|device| device.image()) {
        image(device_image(bytes))
            .width(width)
            .height(height)
            .into()
    } else {
        static GENERIC: OnceLock<svg::Handle> = OnceLock::new();
        svg(GENERIC
            .get_or_init(|| {
                svg::Handle::from_memory(
                    include_bytes!("../../assets/generic_tablet.svg").as_slice(),
                )
            })
            .clone())
        .width(width)
        .height(height)
        .into()
    }
}
/// One handle per device picture. A new handle gets a new id, so building
/// it in every view would decode and scale the PNG again on every frame.
fn device_image(bytes: &'static [u8]) -> image::Handle {
    static HANDLES: Mutex<Vec<(usize, image::Handle)>> = Mutex::new(Vec::new());
    let key = bytes.as_ptr() as usize;
    let mut handles = HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, handle)) = handles.iter().find(|(k, _)| *k == key) {
        return handle.clone();
    }
    let handle = image::Handle::from_bytes(bytes);
    handles.push((key, handle.clone()));
    handle
}
pub(super) fn device_name(app: &App) -> &str {
    let d = &app.device.snapshot;
    if !d.name.is_empty() {
        &d.name
    } else if !d.model.is_empty() {
        &d.model
    } else if d.mode != Mode::None {
        // Connected, as in EDL, without a known model.
        "—"
    } else {
        app.tr("dashboard_no_device")
    }
}
/// The connected device at the top of the navigation, like the account
/// card of Windows Settings.
pub(super) fn device_card(app: &App) -> Element<'_, Message> {
    let d = &app.device.snapshot;
    let mut info = column![label(device_name(app)).size(14)]
        .spacing(4)
        .width(Fill);
    if app.device.dump_only() {
        info =
            info.push(small(format!("{} · {}", d.model, app.tr("dashboard_dump_only"))).size(12));
    } else if !d.model.is_empty() {
        info = info.push(small(d.model.clone()).size(12));
    }
    // The connection state, always shown, in the dashboard's badge.
    // A fixed height keeps the navigation still with or without a model.
    row![portrait(app, 56.0, 42.0), info.push(badge(app))]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding([0, 6])
        .height(68)
        .into()
}
pub(super) fn connection_note(app: &App) -> Element<'_, Message> {
    let s = &app.device.snapshot;
    let detail = if let Some(error) = &s.error {
        error.as_str()
    } else if app.device.unsupported_model() {
        app.tr("dashboard_unsupported_device_check_the_supported_devices")
    } else {
        match s.mode {
            Mode::None => app.tr("dashboard_no_device_is_connected_connect_a"),
            Mode::Unauthorized => app.tr("dashboard_approve_the_usb_debugging_prompt_on"),
            Mode::Sideload => app.tr("dashboard_adb_sideload_is_not_ready_for"),
            Mode::ServerBlocking => app.tr("dashboard_close_the_other_program_s_adb"),
            Mode::Fastboot | Mode::Fastbootd => app.fastboot_start_text(),
            _ if !app.device.reachable() => {
                app.tr("dashboard_checking_device_readiness_please_wait")
            }
            _ if !app.device.startable() => app.tr("app_the_device_model_is_unknown_check"),
            _ => app.tr("dashboard_device_detected_the_connection_is_checked"),
        }
    };
    note(
        format!("{}\n{detail}", status(app)),
        !app.device.startable(),
    )
}
pub(super) fn driver_banner(app: &App) -> Option<Element<'_, Message>> {
    if app.drivers_ready() && app.activity != Some(Activity::Install) {
        return app.driver_restart.then(|| {
            notification_note(app.tr("dashboard_reconnect_usb_after_driver_installation_restart"))
        });
    }
    let installing = app.activity == Some(Activity::Install);
    // Same shape as the other banners: message on the left, action on the right.
    let mut text = column![
        label(if installing {
            app.tr("dashboard_installing_drivers_check_the_permission_prompt")
                .into()
        } else {
            driver_status(app)
        })
        .size(14)
    ]
    .spacing(4)
    .width(Fill);
    if let Some(error) = &app.driver_error {
        text = text.push(small(error));
    }
    let mut content = row![text].spacing(12).align_y(Alignment::Center);
    if app.drivers_installable() || installing {
        content = content.push(action(
            app.tr("dashboard_download_install"),
            Message::InstallDrivers,
            !app.busy() && app.deferred.is_none() && app.drivers_installable(),
        ));
    } else if !app.driver_checking {
        content = content.push(
            action(
                app.tr("dashboard_recheck_drivers"),
                Message::CheckDrivers,
                !app.busy(),
            )
            .style(soft_style),
        );
    }
    Some(notification(content).into())
}
fn value(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}
/// One device detail as a compact card: name above, value below.
/// A detail card, optionally outlined in a status color.
fn detail<'a>(
    name: &'static str,
    value: String,
    outline: Option<iced::Color>,
) -> Element<'a, Message> {
    container(
        column![
            small(name).size(12),
            container(
                label(value)
                    .size(14)
                    .wrapping(iced::widget::text::Wrapping::None)
            )
            .clip(true)
        ]
        .spacing(2),
    )
    .padding([10, 14])
    .width(Fill)
    .style(move |theme| {
        let mut style = super::fluent::card_style(theme);
        if let Some(color) = outline {
            style.border.color = color;
            style.border.width = 2.0;
        }
        style
    })
    .into()
}
/// Support outline: green when DeviceInfo can be written, yellow when it
/// can only be dumped, red for an unsupported model; none without a model.
fn support_outline(app: &App) -> Option<iced::Color> {
    let p = super::fluent::p();
    match app.device.supported() {
        Some(device) if device.writable() => Some(p.success),
        Some(_) => Some(p.caution),
        None if app.device.unsupported_model() => Some(p.danger),
        None => None,
    }
}
pub(super) fn dashboard(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let d = &app.device.snapshot;
    let hero = container(
        row![
            portrait(app, 112.0, 84.0),
            column![
                label(device_name(app)).size(20),
                small(format!(
                    "{}{}",
                    value(&d.model),
                    if d.android.is_empty() {
                        String::new()
                    } else {
                        format!(" · Android {}", d.android)
                    }
                )),
                badge(app)
            ]
            .spacing(6)
            .width(Fill)
        ]
        .spacing(20)
        .align_y(Alignment::Center),
    )
    .padding(20)
    .width(Fill)
    .style(super::fluent::card_style);
    let support = match app.device.supported() {
        Some(device) if !device.writable() => {
            format!("{} · {device}", app.tr("dashboard_dump_only"))
        }
        Some(device) => format!("{} · {device}", app.tr("dashboard_supported")),
        None if app.device.unsupported_model() => app.tr("dashboard_not_supported").into(),
        None => "—".into(),
    };
    let details = [
        (
            app.tr("dashboard_ram_storage"),
            format!("{} / {}", value(&d.ram), value(&d.storage)),
        ),
        (app.tr("dashboard_active_slot"), value(&d.slot).into()),
        (app.tr("dashboard_firmware"), value(&d.firmware).into()),
        (
            app.tr("dashboard_rollback_protection"),
            if app.device.supported().is_some() {
                app.tr("dashboard_enabled_downgrades_blocked")
            } else {
                "—"
            }
            .into(),
        ),
        (app.tr("dashboard_serial_number"), value(&d.serial).into()),
        (app.tr("dashboard_support"), support),
    ];
    let support_name = app.tr("dashboard_support");
    // Each cell keeps one line: long values end in an ellipsis.
    let cell_width = ((app.window_width - 290.0 - 36.0).min(1000.0) - 26.0) / 2.0 - 2.0 - 28.0;
    let budget = ((cell_width / 8.2) as usize).max(8);
    let cell = |name: &'static str, text: String| {
        let text = if text.chars().count() > budget {
            let mut short: String = text.chars().take(budget - 1).collect();
            short.push('…');
            short
        } else {
            text
        };
        detail(
            name,
            text,
            (name == support_name)
                .then(|| support_outline(app))
                .flatten(),
        )
    };
    let mut grid = column![].spacing(4);
    let mut pairs = details.into_iter();
    while let Some((name, text)) = pairs.next() {
        let mut line = row![cell(name, text)].spacing(4);
        if let Some((name, text)) = pairs.next() {
            line = line.push(cell(name, text));
        }
        grid = grid.push(line);
    }
    let mut content = column![
        title(
            app,
            "dashboard_device",
            "dashboard_device_information_and_live_connection_status"
        ),
        hero,
    ]
    .spacing(8);
    if !app.device.startable() {
        content = content.push(connection_note(app));
    } else if d.mode == Mode::Edl && d.model.is_empty() {
        content = content.push(note(app.tr("dashboard_in_edl_model_and_slot_are"), false));
    }
    content = content
        .push(section(app.tr("dashboard_device_details")))
        .push(grid);
    if d.cached {
        content = content.push(small(app.tr("dashboard_some_details_are_from_the_last")));
    }
    if !app.logs.is_empty() {
        content = content
            .push(section(app.tr("dashboard_log")))
            .push(logs(app));
    }
    (
        content.into(),
        small(app.tr("dashboard_connection_status_refreshes_about_every_3")).into(),
    )
}
