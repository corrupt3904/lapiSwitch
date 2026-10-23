//! Read saved work without connecting to a device.
use super::USAGE;
use lapiswitch_core::{
    backup::VerifiedBackup,
    state::{RunState, StateStore},
};
use std::path::PathBuf;

fn state_store(args: &[String]) -> Result<StateStore, String> {
    let path = match args {
        [] => StateStore::default_path()?,
        [flag, path] if flag == "--state" => PathBuf::from(path),
        _ => return Err(USAGE.into()),
    };
    Ok(StateStore::new(path))
}
fn print_state(state: &RunState) {
    println!(
        "{}",
        lapiswitch_core::msg!(
            "cli_status_run",
            run_id = state.run_id,
            phase = format!("{:?}", state.recovery.phase),
            stage = format!("{:?}", state.stage),
            backup = state.backup_dir.display()
        )
    );
    for warning in &state.warnings {
        println!(
            "{}",
            lapiswitch_core::msg!("cli_status_warning", warning = warning)
        );
    }
}
pub(super) fn status(args: &[String]) -> Result<(), String> {
    let store = state_store(args)?;
    match store.load()? {
        None => println!(
            "{}",
            lapiswitch_core::msg!("cli_status_none", path = store.path().display())
        ),
        Some(state) => {
            let transaction = VerifiedBackup::open(&state.backup_dir)
                .and_then(|backup| backup.transaction_stage());
            print_state(&state);
            match transaction {
                Ok(stage) => {
                    println!(
                        "{}",
                        lapiswitch_core::msg!(
                            "cli_status_backup_stage",
                            stage = format!("{stage:?}")
                        )
                    );
                    println!(
                        "{}",
                        lapiswitch_core::msg!(
                            "cli_status_next",
                            action = format!("{:?}", state.resume_action(Some(stage)))
                        )
                    );
                }
                Err(error) => println!(
                    "{}",
                    lapiswitch_core::msg!("cli_status_backup_check", error = error)
                ),
            }
        }
    }
    Ok(())
}
