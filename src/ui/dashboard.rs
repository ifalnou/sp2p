use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use eframe::egui;

use crate::core::state::GLOBAL_STATE;

static DASHBOARD_RUNNING: AtomicBool = AtomicBool::new(false);
static DASHBOARD_REQUESTED: AtomicBool = AtomicBool::new(false);
static HOST_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

pub fn spawn_dashboard() {
    DASHBOARD_REQUESTED.store(true, Ordering::SeqCst);

    // If the host is already spinning, just wake it up
    if DASHBOARD_RUNNING.swap(true, Ordering::SeqCst) {
        if let Some(ctx) = HOST_CTX.lock().unwrap().as_ref() {
            ctx.request_repaint(); // Wake up the host to render the child viewport
        }
        return;
    }

    std::thread::spawn(|| {
        let mut options = eframe::NativeOptions {
            // The Host window must remain "visible" so the event loop doesn't sleep permanently,
            // but we move it far off-screen so the user never sees its 1px bounding box frame.
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([0.0, 0.0])
                .with_position([-10000.0, -10000.0])
                .with_decorations(false)
                .with_transparent(true)
                .with_taskbar(false)
                .with_visible(true),
            run_and_return: false,
            ..Default::default()
        };

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            options.event_loop_builder = Some(Box::new(|builder| {
                builder.with_any_thread(true);
            }));
        }

        let _ = eframe::run_native(
            "sp2p_dashboard_host",
            options,
            Box::new(|cc| {
                *HOST_CTX.lock().unwrap() = Some(cc.egui_ctx.clone());
                Ok(Box::new(DashboardHost::new()))
            }),
        );
        DASHBOARD_RUNNING.store(false, Ordering::SeqCst);
    });
}

#[derive(PartialEq)]
enum Tab {
    Progress,
    History,
    Settings,
}

fn is_auto_start_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(path) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
            if let Ok(val) = path.get_value::<String, _>("sp2p") {
                return val.contains("sp2p.exe");
            }
        }
    }
    false
}

fn set_auto_start(enable: bool) {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_str) = exe_path.to_str() {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                if let Ok((key, _)) = hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
                    if enable {
                        let val = format!("\"{}\"", exe_str);
                        let _ = key.set_value("sp2p", &val);
                    } else {
                        let _ = key.delete_value("sp2p");
                    }
                }
            }
        }
    }
}

struct DashboardHost {
    active_tab: Tab,
    auto_start: bool,
}

impl DashboardHost {
    fn new() -> Self {
        Self {
            active_tab: Tab::Progress,
            auto_start: is_auto_start_enabled(),
        }
    }
}

impl eframe::App for DashboardHost {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // If the user closed the child, we wait patiently
        if !DASHBOARD_REQUESTED.load(Ordering::SeqCst) {
            return;
        }

        let mut close_requested = false;

        // Draw the real UI as a standalone OS child window
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("sp2p_dashboard_ui"),
            egui::ViewportBuilder::default()
                .with_title("sp2p Dashboard")
                .with_inner_size([550.0, 450.0])
                .with_taskbar(true)
                .with_decorations(true)
                .with_active(true)
                .with_window_level(egui::WindowLevel::Normal),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                // Keep repainting fluidly while the dashboard is actually open
                ctx.request_repaint_after(std::time::Duration::from_millis(500));

                egui::TopBottomPanel::top("tabs_panel").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.active_tab, Tab::Progress, "Transfer Progress");
                        ui.selectable_value(&mut self.active_tab, Tab::History, "Transfer History");
                        ui.selectable_value(&mut self.active_tab, Tab::Settings, "Settings");
                    });
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(5.0);
                    match self.active_tab {
                        Tab::Progress => {
                            ui.heading("Active Transfers");
                            ui.separator();
                            ui.add_space(5.0);

                            let state = GLOBAL_STATE.read().unwrap();

                            if state.active_transfers.is_empty() {
                                ui.label("No active transfers.");
                            } else {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for t in &state.active_transfers {
                                        let dir_str = if t.is_sending { "Sending" } else { "Receiving" };
                                        ui.label(format!("{} - {}", dir_str, t.filename));

                                        let progress = if t.total_bytes > 0 {
                                            (t.bytes_transferred as f64) / (t.total_bytes as f64)
                                        } else {
                                            0.0
                                        };

                                        ui.add(egui::ProgressBar::new(progress as f32).text(format!(
                                            "{}/{} MB",
                                            t.bytes_transferred / 1_000_000,
                                            t.total_bytes / 1_000_000
                                        )));
                                        ui.add_space(8.0);
                                    }
                                });
                            }
                        }
                        Tab::History => {
                            ui.heading("Recent History");
                            ui.separator();
                            ui.add_space(5.0);

                            let state = GLOBAL_STATE.read().unwrap();
                            let mut history = state.history.clone();
                            history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                            if history.is_empty() {
                                ui.label("No recent transfers.");
                            } else {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for item in history.iter().take(50) {
                                        let dir_str = if item.is_sending { "Sent" } else { "Received" };
                                        let status_str = if item.success { "Success" } else { "Failed" };
                                        ui.label(format!("{} - {} ({})", dir_str, item.filename, status_str));
                                        ui.add_space(4.0);
                                    }
                                });
                            }
                        }
                        Tab::Settings => {
                            ui.heading("Application Settings");
                            ui.separator();
                            ui.add_space(5.0);

                            if ui.checkbox(&mut self.auto_start, "Run sp2p on Windows startup").changed() {
                                set_auto_start(self.auto_start);
                            }
                        }
                    }
                });
            },
        );

        if close_requested {
            DASHBOARD_REQUESTED.store(false, Ordering::SeqCst);
        }
    }
}
