//! StructureCraft in the browser. Build with `trunk build --release` from this directory.

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;

    const CANVAS_ID: &str = "structurecraft_canvas";

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        wasm_bindgen_futures::spawn_local(async {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let options = eframe::WebOptions { depth_buffer: 32, ..Default::default() };
            let started = eframe::WebRunner::new()
                .start(canvas, options, Box::new(|cc| Ok(Box::new(structurecraft_ui_egui::StructureCraftApp::new(cc, 1)))))
                .await;
            if let Err(e) = started {
                log::error!("could not start: {e:?}");
            }
        });
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    web::start();
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("structurecraft-web runs in the browser: build it with `trunk build` from apps/structurecraft-web");
}
