#[cfg(all(target_arch = "wasm32", feature = "web"))]
fn main() {
    use wasm_bindgen::JsCast as _;
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(element) = document.get_element_by_id("wunder-egui-canvas") else {
            return;
        };
        let Ok(canvas) = element.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        let _ = wunder_egui::run_web(canvas).await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("wunder-egui-web must be built for wasm32-unknown-unknown");
}

#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
fn main() {
    // Keep `cargo check --target wasm32-unknown-unknown` useful without
    // accidentally pulling browser-only dependencies into the native feature.
}
