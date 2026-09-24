// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args_os().any(|argument| argument == "--check-integrity") {
        let report = transportal_lib::integrity::check_current_executable();
        println!(
            "{}",
            serde_json::to_string(&report).expect("integrity report is serializable")
        );
        std::process::exit(if report.blocks_startup() { 2 } else { 0 });
    }
    transportal_lib::run()
}
