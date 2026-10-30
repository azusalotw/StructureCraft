//! The StructureCraft desktop app.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

/// Multisampling for the window; the 3D view's pipelines are built to match.
const MSAA_SAMPLES: u16 = 4;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn")).init();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("StructureCraft")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        multisampling: MSAA_SAMPLES,
        ..Default::default()
    };
    // A model file given on the command line (or by the system's "open with") opens at start.
    let path = std::env::args().nth(1);
    let start = move |cc: &eframe::CreationContext<'_>| {
        let mut app = structurecraft_ui_egui::StructureCraftApp::new(cc, u32::from(MSAA_SAMPLES));
        if let Some(path) = &path {
            app.open(path);
        }
        Ok(Box::new(app) as Box<dyn eframe::App>)
    };
    match eframe::run_native("StructureCraft", options, Box::new(start)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("could not open the window: {e}");
            ExitCode::FAILURE
        }
    }
}
