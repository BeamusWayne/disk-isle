#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod platform;
mod scan;
mod ui;
use std::{path::PathBuf, time::Duration};

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let path = scan::cache_root();
        let _ = std::fs::create_dir_all(&path);
        let _ = std::fs::write(path.join("last-error.txt"), format!("{info}"));
    }));
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|v| v == "--scan") {
        if let Err(error) = command_scan(&args) {
            let output = args
                .windows(2)
                .find(|p| p[0] == "--report")
                .map(|p| PathBuf::from(&p[1]));
            if let Some(output) = output {
                let _ = std::fs::write(
                    output,
                    serde_json::json!({"error":error.to_string()}).to_string(),
                );
            }
            std::process::exit(1);
        }
        return;
    }
    let initial = args.first().map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([820.0, 600.0])
            .with_title("盘屿 · Disk Isle"),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "Disk Isle",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, initial)))),
    ) {
        let path = scan::cache_root();
        let _ = std::fs::create_dir_all(&path);
        let _ = std::fs::write(path.join("last-error.txt"), error.to_string());
        rfd::MessageDialog::new()
            .set_title("盘屿")
            .set_description(format!("窗口无法启动。{error}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
fn command_scan(
    args: &[std::ffi::OsString],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = PathBuf::from(args.get(1).ok_or("缺少扫描目录")?);
    let profile = if args.iter().any(|v| v == "balanced") {
        scan::Profile::Balanced
    } else {
        scan::Profile::Gentle
    };
    let report = args
        .windows(2)
        .find(|p| p[0] == "--report")
        .map(|p| PathBuf::from(&p[1]))
        .ok_or("请指定 --report 输出路径")?;
    let stop_after = args
        .windows(2)
        .find(|p| p[0] == "--stop-after-ms")
        .and_then(|p| p[1].to_str()?.parse::<u64>().ok());
    let job = scan::start(root, profile)?;
    if let Some(ms) = stop_after {
        let control = job.control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            control.stop();
        });
    }
    loop {
        match job.events.recv()? {
            scan::Event::Progress(_) => {}
            scan::Event::Finished(stats, error) => {
                job.worker.join().map_err(|_| "扫描线程退出异常")?;
                let result = serde_json::json!({"stats":stats,"error":error,"index":job.path});
                std::fs::write(report, serde_json::to_vec_pretty(&result)?)?;
                if error.is_some() {
                    return Err("扫描失败，详见报告".into());
                }
                return Ok(());
            }
        }
    }
}
