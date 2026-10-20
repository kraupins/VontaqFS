#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn install_startup_panic_log() {
    use std::{backtrace::Backtrace, fs::OpenOptions, io::Write, panic, path::PathBuf};

    // Installed Windows builds have no console. Always keep a bounded panic log
    // in the user's own application data even without diagnostic environment flags.
    let path = std::env::var_os("VONTAQFS_STARTUP_TRACE_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("APPDATA").map(|base| {
                PathBuf::from(base)
                    .join("fs.vontaq.desktop")
                    .join("diagnostics")
                    .join("desktop-crash.log")
            })
        });
    let Some(path) = path else {
        return;
    };
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    // Diagnostic output is best-effort and must never interfere with startup.
    if std::fs::metadata(&path).is_ok_and(|info| info.len() > 2_097_152) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }

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
