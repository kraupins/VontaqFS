#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn install_startup_panic_log() {
    use std::{backtrace::Backtrace, fs::OpenOptions, io::Write, panic, path::PathBuf};

    let Some(path) = std::env::var_os("VONTAQFS_STARTUP_TRACE_PATH") else {
        return;
    };
    let path = PathBuf::from(path);

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "START pid={}", std::process::id());
    }

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "PANIC: {info}");
            let _ = writeln!(file, "BACKTRACE:\n{}", Backtrace::force_capture());
        }
        previous_hook(info);
    }));
}

fn main() {
    #[cfg(target_os = "windows")]
    install_startup_panic_log();

    vontaqfs_desktop_lib::run();
}
