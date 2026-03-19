use eframe::egui;
use crate::core::state::GLOBAL_STATE;

pub fn spawn_dashboard() {
    std::thread::spawn(|| {
        let mut options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([500.0, 400.0])
                .with_title("sp2p Dashboard"),
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
            "sp2p_dashboard",
            options,
            Box::new(|_cc| Ok(Box::new(DashboardApp::default()))),
        );
    });
}

#[derive(Default)]
struct DashboardApp {}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Redraw constantly so progress bars update
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Active Transfers");
            ui.separator();

            let state = GLOBAL_STATE.read().unwrap();

            if state.active_transfers.is_empty() {
                ui.label("No active transfers.");
            } else {
                for t in &state.active_transfers {
                    let dir_str = if t.is_sending { "Sending" } else { "Receiving" };
                    ui.label(format!("{} - {}", dir_str, t.filename));

                    let progress = if t.total_bytes > 0 {
                        (t.bytes_transferred as f32) / (t.total_bytes as f32)
                    } else {
                        0.0
                    };

                    ui.add(egui::ProgressBar::new(progress).text(format!(
                        "{}/{} MB",
                        t.bytes_transferred / 1_000_000,
                        t.total_bytes / 1_000_000
                    )));
                    ui.add_space(5.0);
                }
            }

            ui.add_space(15.0);
            ui.heading("Recent History");
            ui.separator();

            let mut history = state.history.clone();
            history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            // limit to only show last 10
            for item in history.iter().take(10) {
                let dir_str = if item.is_sending { "Sent" } else { "Received" };
                let status_str = if item.success { "Success" } else { "Failed" };
                ui.label(format!("{} - {} ({})", dir_str, item.filename, status_str));
            }
        });
    }
}
