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
        "작업 {}: {:?}\n백업: {}",
        state.run_id,
        state.stage,
        state.backup_dir.display()
    );
    for warning in &state.warnings {
        println!("경고: {warning}");
    }
}
pub(super) fn status(args: &[String]) -> Result<(), String> {
    let store = state_store(args)?;
    match store.load()? {
        None => println!(
            "저장된 작업이 없습니다. 상태 파일: {}",
            store.path().display()
        ),
        Some(state) => {
            let transaction = VerifiedBackup::open(&state.backup_dir)
                .and_then(|backup| backup.transaction_stage());
            print_state(&state);
            match transaction {
                Ok(stage) => {
                    println!("백업 작업 상태: {stage:?}");
                    if state.recovery.is_none() {
                        println!("{}", lapiswitch_core::state::LEGACY_CONVERSION_ENDED);
                    } else {
                        println!("다음 동작: {:?}", state.resume_action(Some(stage)));
                    }
                }
                Err(error) => println!("백업 확인 필요: {error}"),
            }
        }
    }
    Ok(())
}
