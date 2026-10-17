//! Windows 资源嵌入：应用图标（对非 Windows 目标为 no-op）。
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/app_icon.ico")
            .compile()
            .expect("embed app icon");
    }
}
