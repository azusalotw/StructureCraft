//! The StructureCraft desktop app.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn")).init();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("StructureCraft")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        multisampling: 4,
        ..Default::default()
    };
    match eframe::run_native("StructureCraft", options, Box::new(|cc| Ok(Box::new(structurecraft_ui_egui::StructureCraftApp::new(cc))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("could not open the window: {e}");
            ExitCode::FAILURE
        }
    }
}
