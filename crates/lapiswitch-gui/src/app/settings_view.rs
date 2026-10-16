//! Settings: one outlined section per option, with headings aligned on the left
//! and each section's main control on the right or directly below.
use super::view::*;
use super::*;
use iced::widget::{button, checkbox, column, container, pick_list, row, space};
use iced::{Alignment, Border, Color, Fill};

const SECTION: Color = Color::from_rgb8(250, 251, 253);

fn section<'a>(
    heading: &'a str,
    description: &'a str,
    control: Option<Element<'a, Message>>,
    body: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut head = row![
        column![label(heading).size(18), small(description)]
            .spacing(5)
            .width(Fill)
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    if let Some(control) = control {
        head = head.push(control);
    }
    let mut content = column![head].spacing(16);
    for item in body {
        content = content.push(item);
    }
    container(content)
        .padding(20)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(SECTION.into()),
            border: Border {
                color: LINE,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn driver_modes(app: &App) -> Element<'_, Message> {
    let mut choices = row![].spacing(12);
    for (kernel, name, description) in [
        (
            true,
            "Kernel",
            app.t("Qualcomm 직렬 드라이버", "Qualcomm serial driver"),
        ),
        (false, "Userspace", "WinUSB / libusb"),
    ] {
        let supported = !kernel || ltbox_device::driver::kernel_mode_supported();
        let selected =
            kernel == (app.preferences.kernel && ltbox_device::driver::kernel_mode_supported());
        choices = choices.push(
            button(
                column![
                    label(format!("{}  {name}", if selected { "●" } else { "○" })).size(17),
                    small(description),
                ]
                .spacing(6),
            )
            .padding(16)
            .width(Fill)
            .on_press_maybe(
                (supported && !app.driver_checking && !app.busy() && app.deferred.is_none())
                    .then_some(Message::Kernel(kernel)),
            )
            .style(move |theme, status| {
                let mut style = button::secondary(theme, status);
                style.background = Some(if selected { PALE } else { Color::WHITE }.into());
                style.text_color = INK;
                style.border = Border {
                    color: if selected { ACCENT } else { LINE },
                    width: if selected { 2.0 } else { 1.0 },
                    radius: 10.0.into(),
                };
                style
            }),
        );
    }
    choices.into()
}

pub(super) fn settings(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let guide = if cfg!(windows) {
        app.t("Windows 장치 관리자에서 ADB / Android Bootloader 드라이버를 확인하세요. Userspace 모드는 9008 장치에 WinUSB가 필요하며, Kernel 모드는 Qualcomm 직렬 드라이버를 사용합니다. 드라이버 변경 후 상태를 다시 확인하세요.", "Check ADB / Android Bootloader drivers in Device Manager. Userspace mode requires WinUSB for the 9008 device; Kernel mode uses the Qualcomm serial driver. Recheck after setup.")
    } else if cfg!(target_os = "linux") {
        app.t("Linux에서는 misc/udev/51-ltbox-qcom.rules를 /etc/udev/rules.d에 설치하고 규칙을 다시 불러온 뒤 USB를 재연결하세요.", "On Linux, install misc/udev/51-ltbox-qcom.rules in /etc/udev/rules.d, reload udev rules and reconnect USB.")
    } else {
        app.t(
            "macOS에서는 Userspace USB 모드를 사용합니다.",
            "macOS uses Userspace USB mode.",
        )
    };
    let default_mode = if ltbox_device::driver::kernel_mode_supported() {
        app.t(
            "LTBox 기본값: Kernel / Qualcomm (저장된 선택은 유지)",
            "LTBox default: Kernel / Qualcomm (saved choice is preserved)",
        )
    } else {
        app.t("기본값: Userspace / WinUSB", "Default: Userspace / WinUSB")
    };
    // Always labelled in English so anyone can find it in any language.
    let language = section(
        "Language",
        app.t("앱 화면에 표시할 언어", "Display language of the app"),
        Some(
            pick_list(
                Language::ALL,
                Some(app.preferences.language),
                Message::Language,
            )
            .text_shaping(iced::widget::text::Shaping::Advanced)
            .padding([9, 14])
            .width(170)
            .style(|theme, status| {
                let mut style = pick_list::default(theme, status);
                style.background = Color::WHITE.into();
                style.text_color = INK;
                style.handle_color = MUTED;
                style.border = Border {
                    color: if matches!(status, pick_list::Status::Active) {
                        LINE
                    } else {
                        ACCENT
                    },
                    width: 1.0,
                    radius: 8.0.into(),
                };
                style
            })
            .into(),
        ),
        vec![],
    );
    let privacy = section(
        app.t("개인정보 보호", "Privacy"),
        app.t(
            "시리얼 필드와 ADB 목록을 가립니다. 복구에 필요한 백업·getvar 원본은 변경하지 않으며, 기존 로그에는 소급 적용되지 않습니다.",
            "Masks serial fields and ADB lists. Recovery backups and original getvar stay intact. Existing transcripts are unchanged.",
        ),
        None,
        vec![
            checkbox(app.preferences.mask_serial)
                .label(app.t(
                    "앱 로그에서 시리얼 번호 숨기기",
                    "Mask serial numbers in app logs",
                ))
                .on_toggle(Message::Mask)
                .into(),
        ],
    );
    let storage = section(
        app.t("로그 및 백업 위치", "Logs and backup location"),
        app.t(
            "선택한 위치 아래 logs에 로그를, backup/region_change에 새 백업을 저장합니다. 기존 파일은 이동하지 않습니다.",
            "Logs go to logs and new backups to backup/region_change under this location. Existing files are not moved.",
        ),
        None,
        vec![
            row![
                container(label(app.output.display().to_string()).size(14))
                    .padding([10, 14])
                    .width(Fill)
                    .style(|_| container::Style {
                        background: Some(Color::WHITE.into()),
                        border: Border {
                            color: LINE,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }),
                action(
                    app.t("폴더 선택", "Choose folder"),
                    Message::Pick(Picker::Output),
                    !app.busy(),
                )
                .style(soft_style),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
        ],
    );
    let drivers = section(
        app.t("USB 드라이버", "USB driver"),
        app.t(
            "기기와 연결할 때 사용할 드라이버 방식을 선택합니다.",
            "Choose the driver mode used to connect to the device.",
        ),
        None,
        vec![
            driver_modes(app),
            small(default_mode).into(),
            row![
                note(driver_status(app), !app.drivers_ready()),
                action(
                    app.t("드라이버 다시 확인", "Recheck drivers"),
                    Message::CheckDrivers,
                    !app.driver_checking,
                )
                .style(soft_style),
                action(
                    app.t("다운로드 및 설치", "Download & install"),
                    Message::InstallDrivers,
                    app.drivers_installable() && !app.busy(),
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            small(guide).into(),
        ],
    );
    let divider = container(space())
        .width(Fill)
        .height(1)
        .style(|_| container::Style {
            background: Some(LINE.into()),
            ..Default::default()
        });
    let content = column![
        title(
            app,
            "앱 설정",
            "App settings",
            "언어·로그 및 백업 위치·USB 연결 방식을 설정합니다.",
            "Language, log and backup location, and USB connection mode."
        ),
        language,
        privacy,
        storage,
        drivers,
        divider,
        small(app.t(
            "핵심 작업 오류와 일부 상세 로그는 한국어로 표시될 수 있습니다.",
            "Core operation errors and some detailed logs may appear in Korean."
        )),
        small("flag-icons · MIT License\nLTBox v3.3.4 (730b30d) · GPL-3.0-or-later\nNoto Sans KR · SIL Open Font License 1.1"),
    ]
    .spacing(16);
    (content.into(), footer(app, None))
}
