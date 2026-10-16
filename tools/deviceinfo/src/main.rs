use lapiswitch_deviceinfo::{Options, phase, self_test};
#[cfg(all(target_os = "android", target_arch = "aarch64"))]
mod android;
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--self-test" {
        return match self_test() {
            Ok(()) => {
                phase("SELF_TEST_PASSED_NO_QSEE_ACCESS");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return 2.into();
        }
    };
    #[cfg(all(target_os = "android", target_arch = "aarch64"))]
    {
        android::run(options).into()
    }
    #[cfg(not(all(target_os = "android", target_arch = "aarch64")))]
    {
        let _ = options;
        eprintln!(
            "DeviceInfo I/O requires the Android ARM64 build. --self-test is available on this host."
        );
        std::process::ExitCode::FAILURE
    }
}
