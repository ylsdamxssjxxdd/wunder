#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|argument| argument == "--demo");
    if args.iter().any(|argument| argument == "--demo-tool") {
        return wunder_egui::run_native_demo_workflow(true);
    }
    if args.iter().any(|argument| argument == "--demo-workflow") {
        return wunder_egui::run_native_demo_workflow(false);
    }
    if let Some(category) = args
        .iter()
        .find_map(|argument| argument.strip_prefix("--demo-settings="))
    {
        return wunder_egui::run_native_demo_settings(category.to_owned());
    }
    if demo {
        wunder_egui::run_native_demo()
    } else {
        wunder_egui::run_native()
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    eprintln!("wunder-egui native binary must be built for a native target");
}
