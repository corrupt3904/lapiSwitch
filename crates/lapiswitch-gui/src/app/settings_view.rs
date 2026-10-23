//! Settings: one outlined section per option, with headings aligned on the left
//! and each section's main control on the right or directly below.
use super::view::*;
use super::*;
use iced::widget::{button, checkbox, column, container, pick_list, row, space};
use iced::{Alignment, Border, Fill};

fn section<'a>(
    heading: &'a str,
    description: &'a str,
    control: Option<Element<'a, Message>>,
    body: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut head = row![
        column![label(heading).size(15), small(description)]
            .spacing(3)
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
        .padding([16, 18])
        .width(Fill)
        .style(super::fluent::card_style)
        .into()
}

fn driver_modes(app: &App) -> Element<'_, Message> {
    let mut choices = row![].spacing(12);
    for (kernel, name, description) in [
        (true, "Kernel", app.tr("settings_qualcomm_serial_driver")),
        (false, "Userspace", "WinUSB / libusb"),
    ] {
        let supported = !kernel || ltbox_device::driver::kernel_mode_supported();
        let selected =
            kernel == (app.preferences.kernel && ltbox_device::driver::kernel_mode_supported());
        choices = choices.push(
            button(
                column![
                    label(format!("{}  {name}", if selected { "●" } else { "○" })).size(15),
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
                style.background = Some(if selected { pale() } else { card_color() }.into());
                style.text_color = ink();
                style.border = Border {
                    color: if selected { accent() } else { line() },
                    width: if selected { 2.0 } else { 1.0 },
                    radius: super::fluent::CARD_RADIUS.into(),
                };
                style
            }),
        );
    }
    choices.into()
}

/// Segmented choice of the app theme.
fn themes(app: &App) -> Element<'_, Message> {
    use crate::settings::ThemeChoice;
    let mut choices = row![].spacing(4);
    for (choice, name) in [
        (ThemeChoice::System, app.tr("settings_windows_setting")),
        (ThemeChoice::Light, app.tr("settings_light")),
        (ThemeChoice::Dark, app.tr("settings_dark")),
    ] {
        let selected = app.preferences.theme == choice;
        choices = choices.push(
            button(label(name).size(14).color(if selected {
                super::fluent::p().on_accent
            } else {
                ink()
            }))
            .padding([7, 16])
            .on_press(Message::Theme(choice))
            .style(if selected {
                super::fluent::accent_style
            } else {
                super::fluent::standard_style
            }),
        );
    }
    choices.into()
}

pub(super) fn settings(app: &App) -> (Element<'_, Message>, Element<'_, Message>) {
    let guide = if cfg!(windows) {
        app.tr("settings_check_adb_android_bootloader_drivers_in")
    } else if cfg!(target_os = "linux") {
        app.tr("settings_on_linux_install_misc_udev_51")
    } else {
        app.tr("settings_macos_uses_userspace_usb_mode")
    };
    let default_mode = if ltbox_device::driver::kernel_mode_supported() {
        app.tr("settings_ltbox_default_kernel_qualcomm_saved_choice")
    } else {
        app.tr("settings_default_userspace_winusb")
    };
    // Always labelled in English so anyone can find it in any language.
    let language = section(
        "Language",
        app.tr("settings_display_language_of_the_app"),
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
                style.background = card_color().into();
                style.text_color = ink();
                style.handle_color = muted();
                style.border = Border {
                    color: if matches!(status, pick_list::Status::Active) {
                        line()
                    } else {
                        accent()
                    },
                    width: 1.0,
                    radius: super::fluent::CONTROL_RADIUS.into(),
                };
                style
            })
            .into(),
        ),
        vec![],
    );
    let theme = section(
        app.tr("settings_theme"),
        app.tr("settings_light_dark_or_follow_the_windows"),
        Some(themes(app)),
        vec![],
    );
    let privacy = section(
        app.tr("settings_privacy"),
        app.tr("settings_masks_serial_fields_and_adb_lists"),
        None,
        vec![
            checkbox(app.preferences.mask_serial)
                .label(app.tr("settings_mask_serial_numbers_in_app_logs"))
                .on_toggle(Message::Mask)
                .into(),
        ],
    );
    let storage = section(
        app.tr("settings_logs_and_backup_location"),
        app.tr("settings_logs_go_to_logs_and_new"),
        None,
        vec![
            row![
                container(label(app.output.display().to_string()).size(14))
                    .padding([10, 14])
                    .width(Fill)
                    .style(|_| container::Style {
                        background: Some(card_color().into()),
                        border: Border {
                            color: line(),
                            width: 1.0,
                            radius: super::fluent::CONTROL_RADIUS.into(),
                        },
                        ..Default::default()
                    }),
                action(
                    app.tr("settings_choose_folder"),
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
        app.tr("settings_usb_driver"),
        app.tr("settings_choose_the_driver_mode_used_to"),
        None,
        vec![
            driver_modes(app),
            small(default_mode).into(),
            row![
                note(driver_status(app), !app.drivers_ready()),
                action(
                    app.tr("dashboard_recheck_drivers"),
                    Message::CheckDrivers,
                    !app.driver_checking,
                )
                .style(soft_style),
                action(
                    app.tr("dashboard_download_install"),
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
            background: Some(line().into()),
            ..Default::default()
        });
    let content = column![
        title(
            app,
            "settings_app_settings",
            "settings_language_log_and_backup_location_and"
        ),
        language,
        theme,
        privacy,
        storage,
        drivers,
        divider,
        small(app.tr("settings_core_operation_errors_and_some_detailed")),
        section(
            app.tr("licenses_open_source_licenses"),
            app.tr("settings_read_license_and_copyright_notices_for"),
            Some(
                action(app.tr("settings_view_licenses"), Message::LicenseOpen, true)
                    .style(soft_style)
                    .into()
            ),
            vec![],
        ),
    ]
    .spacing(16);
    (
        content.into(),
        small(format!(
            "{} v{}",
            crate::brand::NAME,
            env!("CARGO_PKG_VERSION")
        ))
        .into(),
    )
}
