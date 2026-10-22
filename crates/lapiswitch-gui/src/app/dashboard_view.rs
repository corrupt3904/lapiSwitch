use super::view::*;
use super::*;
use crate::device::Mode;
use iced::widget::{column, container, image, row, svg};
use iced::{Alignment, Fill};

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
    container(
        label(format!("●  {}", status(app)))
            .size(13)
            .color(if app.device.reachable() {
                ACCENT
            } else {
                MUTED
            }),
    )
    .padding([8, 14])
    .style(pill_style)
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
        Mode::None => app.t("기기가 연결되어 있지 않습니다. 기기를 연결해야 변환을 시작할 수 있습니다.", "No device is connected. Connect a device before starting conversion."),
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
pub(super) fn dashboard(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let d = &app.device.snapshot;
    let name = if d.name.is_empty() {
        app.t("연결된 기기", "Connected device")
    } else {
        &d.name
    };
    // devices/<MODEL>/image.png of the connected supported device.
    let portrait: Element<'_, Message> =
        if let Some(bytes) = app.device.supported().and_then(|device| device.image()) {
            image(image::Handle::from_bytes(bytes))
                .width(92)
                .height(70)
                .into()
        } else {
            svg(svg::Handle::from_memory(
                include_bytes!("../../assets/generic_tablet.svg").as_slice(),
            ))
            .width(92)
            .height(70)
            .into()
        };
    let mut content = column![
        title(
            app,
            "Dashboard",
            "Dashboard",
            "연결된 기기 정보와 현재 상태를 확인합니다.",
            "Device information and live connection status."
        ),
        row![
            portrait,
            column![
                label(name).size(25),
                small(format!(
                    "{}  {}",
                    value(&d.model),
                    if d.android.is_empty() {
                        String::new()
                    } else {
                        format!("· Android {}", d.android)
                    }
                ))
            ]
            .spacing(5)
            .width(Fill),
            badge(app)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        field(
            app.t("RAM · 저장 공간", "RAM · Storage"),
            format!("{} / {}", value(&d.ram), value(&d.storage))
        ),
        field(app.t("활성 슬롯", "Active slot"), value(&d.slot)),
        field(
            app.t("롤백 방지", "Rollback protection"),
            if app.device.supported().is_some() {
                app.t("사용 · 다운그레이드 차단", "Enabled · Downgrades blocked")
            } else {
                "—"
            }
        ),
        field(app.t("펌웨어", "Firmware"), value(&d.firmware)),
        field(app.t("시리얼 번호", "Serial number"), value(&d.serial)),
    ]
    .spacing(18);
    if d.cached {
        content = content.push(small(app.t(
            "일부 기기 정보는 마지막으로 읽은 값입니다. 현재 연결 모드는 계속 갱신됩니다.",
            "Some details are from the last successful read. Connection mode continues to update.",
        )));
    }
    if !app.device.startable() {
        content = content.push(connection_note(app));
    } else if d.mode == Mode::Edl && d.model.is_empty() {
        content = content.push(small(app.t(
            "EDL에서는 변환 준비 단계에서 모델과 슬롯을 확인합니다.",
            "In EDL, model and slot are verified during conversion preparation.",
        )));
    }
    content = content.push(
        row![
            iced::widget::space().width(Fill),
            action(
                app.t("원본 복구", "Recovery"),
                Message::Recovery,
                !app.busy()
            )
            .style(recovery_style),
            action(
                app.t("리전 변환", "Region conversion"),
                Message::Convert,
                !app.busy() && !app.device.unsupported_model()
            )
            .style(soft_style),
            action(
                app.t("DeviceInfo 수정", "Edit DeviceInfo"),
                Message::Edit,
                !app.busy() && !app.device.unsupported_model()
            )
        ]
        .spacing(12),
    );
    if !app.logs.is_empty() {
        content = content.push(logs(app));
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
