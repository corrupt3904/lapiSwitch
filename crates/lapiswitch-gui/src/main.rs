#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(
    non_snake_case,
    reason = "Match the product executable name lapiSwitch."
)]
mod app;
mod brand;
mod countries;
mod device;
mod platform_installers;
mod progress;
mod settings;
mod transcript;
mod wait_control;
mod worker;
fn main() -> iced::Result {
    if std::env::args().any(|arg| arg == "--install-udev") {
        platform_installers::install_udev_rules();
    }
    app::run()
}
