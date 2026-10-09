fn main() {
    // Tauri's build-time context tracks frontendDist, including new or removed assets.
    let attributes = tauri_build::Attributes::new().codegen(tauri_build::CodegenContext::new());
    tauri_build::try_build(attributes).expect("failed to build Kaleidoscope application context");
}
