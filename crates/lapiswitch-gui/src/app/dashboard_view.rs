use super::view::*;
use super::*;
use crate::device::Mode;
use iced::widget::{column, container, image, row, svg};
use iced::{Alignment, Fill};
use std::sync::{Mutex, OnceLock};

pub(super) fn status(app: &App) -> String {
    if app.device.snapshot.error.is_some() {
        return app.t("연결 확인 실패", "Connection check failed").into();
    }
    if app.device.checked.is_none() {
        return app.t("연결 확인 중…", "Checking connection…").into();
    }
    if !app.demo
        && app
            .device
            .checked
            .is_some_and(|t| t.elapsed() >= crate::device::MAX_AGE)
    {
        return app.t("연결 다시 확인 중…", "Rechecking connection…").into();
    }
    match app.device.snapshot.mode {
        Mode::None => app.t("기기 연결 안 됨", "No device connected"),
        Mode::Adb => "ADB",
        Mode::Recovery => "ADB · Recovery",
        Mode::Unauthorized => app.t("ADB · 승인 필요", "ADB · Unauthorized"),
        Mode::Sideload => "ADB · Sideload",
        Mode::ServerBlocking => app.t("ADB · 서버 충돌", "ADB · Server conflict"),
        Mode::Fastboot => "FASTBOOT",
        Mode::Fastbootd => "FASTBOOTD",
        Mode::Edl => "EDL · 9008",
        Mode::Usb => app.t("ADB USB · 작업 중", "ADB USB · In use"),
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
fn device_name(app: &App) -> &str {
    let d = &app.device.snapshot;
    if !d.name.is_empty() {
        &d.name
    } else if !d.model.is_empty() {
        &d.model
    } else {
        app.t("연결된 기기 없음", "No device")
    }
}
/// The connected device at the top of the navigation, like the account
/// card of Windows Settings.
pub(super) fn device_card(app: &App) -> Element<'_, Message> {
    let d = &app.device.snapshot;
    row![
        portrait(app, 56.0, 42.0),
        column![
            label(device_name(app)).size(14),
            small(if d.model.is_empty() {
                status(app)
            } else if app.device.dump_only() {
                format!(
                    "{} · {} · {}",
                    d.model,
                    status(app),
                    app.t("덤프만 지원", "Dump only")
                )
            } else {
                format!("{} · {}", d.model, status(app))
            })
            .size(12)
        ]
        .spacing(2)
        .width(Fill)
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .padding([0, 6])
    .into()
}
pub(super) fn connection_note(app: &App) -> Element<'_, Message> {
    let s = &app.device.snapshot;
    let unsupported;
    let detail = if let Some(error) = &s.error {
        error.as_str()
    } else if app.device.unsupported_model() {
        unsupported = app
            .t(
                "지원하지 않는 기기입니다. {models}만 사용할 수 있습니다.",
                "Unsupported device. Supported: {models}.",
            )
            .replace("{models}", &super::supported_models());
        unsupported.as_str()
    } else {
        match s.mode {
        Mode::None => app.t("기기가 연결되어 있지 않습니다. 기기를 연결해야 작업을 시작할 수 있습니다.", "No device is connected. Connect a device before starting."),
        Mode::Unauthorized => app.t("태블릿에서 USB 디버깅 허용 창을 승인해 주세요.", "Approve the USB debugging prompt on the tablet."),
        Mode::Sideload => app.t("ADB Sideload에서는 시작할 수 없습니다. 시스템 또는 지원하는 연결 모드로 전환해 주세요.", "ADB Sideload is not ready for conversion. Use the system or another supported mode."),
        Mode::ServerBlocking => app.t("다른 프로그램의 ADB 서버를 종료한 뒤 다시 연결해 주세요.", "Close the other program's ADB server and reconnect."),
        Mode::Fastboot | Mode::Fastbootd => app.fastboot_start_text(),
        _ if !app.device.reachable() => app.t("기기 연결 상태를 확인하고 있습니다. 잠시 기다려 주세요.", "Checking device readiness. Please wait."),
        _ if !app.device.startable() => app.t("기기 모델을 확인할 수 없습니다. 기기 연결 상태를 확인해 주세요.", "The device model is unknown. Check the device connection."),
        _ => app.t("기기 연결을 확인했습니다. 시작 직전에 한 번 더 확인합니다.", "Device detected. The connection is checked again immediately before starting."),
    }
    };
    note(
        format!("{}\n{detail}", status(app)),
        !app.device.startable(),
    )
}
pub(super) fn driver_banner(app: &App) -> Option<Element<'_, Message>> {
    if app.drivers_ready() && app.activity != Some(Activity::Install) {
        return app.driver_restart.then(|| notification_note(app.t("드라이버 설치 후 USB를 재연결하세요. 기기가 감지되지 않으면 PC를 재시작해 주세요.", "Reconnect USB after driver installation. Restart the PC if the device is not detected.")));
    }
    let installing = app.activity == Some(Activity::Install);
    // Same shape as the other banners: message on the left, action on the right.
    let mut text = column![
        label(if installing {
            app.t(
                "드라이버 설치 중… 권한 요청 창을 확인해 주세요.",
                "Installing drivers… Check the permission prompt.",
            )
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
            app.t("다운로드 및 설치", "Download & install"),
            Message::InstallDrivers,
            !app.busy() && app.deferred.is_none() && app.drivers_installable(),
        ));
    } else if !app.driver_checking {
        content = content.push(
            action(
                app.t("드라이버 다시 확인", "Recheck drivers"),
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
            format!("{} · {device}", app.t("덤프만 지원", "Dump only"))
        }
        Some(device) => format!("{} · {device}", app.t("지원 기기", "Supported")),
        None if app.device.unsupported_model() => {
            app.t("지원하지 않는 기기", "Not supported").into()
        }
        None => "—".into(),
    };
    let details = [
        (
            app.t("RAM · 저장 공간", "RAM · Storage"),
            format!("{} / {}", value(&d.ram), value(&d.storage)),
        ),
        (app.t("활성 슬롯", "Active slot"), value(&d.slot).into()),
        (app.t("펌웨어", "Firmware"), value(&d.firmware).into()),
        (
            app.t("롤백 방지", "Rollback protection"),
            if app.device.supported().is_some() {
                app.t("사용 · 다운그레이드 차단", "Enabled · Downgrades blocked")
            } else {
                "—"
            }
            .into(),
        ),
        (
            app.t("시리얼 번호", "Serial number"),
            value(&d.serial).into(),
        ),
        (app.t("지원 상태", "Support"), support),
    ];
    let support_name = app.t("지원 상태", "Support");
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
            "기기",
            "Device",
            "연결된 기기 정보와 현재 상태를 확인합니다.",
            "Device information and live connection status."
        ),
        hero,
    ]
    .spacing(8);
    if !app.device.startable() {
        content = content.push(connection_note(app));
    } else if d.mode == Mode::Edl && d.model.is_empty() {
        content = content.push(note(
            app.t(
                "EDL에서는 작업 준비 단계에서 모델과 슬롯을 확인합니다.",
                "In EDL, model and slot are verified when the operation starts.",
            ),
            false,
        ));
    }
    content = content
        .push(section(app.t("기기 정보", "Device details")))
        .push(grid);
    if d.cached {
        content = content.push(small(app.t(
            "일부 기기 정보는 마지막으로 읽은 값입니다. 현재 연결 모드는 계속 갱신됩니다.",
            "Some details are from the last successful read. Connection mode continues to update.",
        )));
    }
    if !app.logs.is_empty() {
        content = content.push(section(app.t("로그", "Log"))).push(logs(app));
    }
    (
        content.into(),
        small(app.t(
            "연결 상태는 약 3초마다 자동 확인됩니다.",
            "Connection status refreshes about every 3 seconds.",
        ))
        .into(),
    )
}
