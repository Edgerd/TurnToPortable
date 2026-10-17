//! pxe-gui —— egui + Fluent 2 主题。任务在工作线程跑，UI 线程只消费结果（原版在
//! 工作线程直接调 messagebox 导致 tkinter 线程不安全的问题不会出现）。

// release 构建隐藏控制台黑框
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fluent;

use eframe::egui;
use egui::Color32;
use pxe_core::{builder, config::Config, pipeline};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

#[derive(Default)]
struct App {
    input_file: String,
    app_name: String,
    output_dir: String,
    pack: builder::PackKind,
    /// 深度解包开关
    deep_extract: bool,
    log: Vec<String>,
    progress: f32,
    status: String,
    busy: bool,
    tx: Option<mpsc::Sender<Msg>>,
    rx: Option<mpsc::Receiver<Msg>>,
    /// 界内 logo 纹理（首次绘制时加载）
    logo: Option<egui::TextureHandle>,
}

enum Msg {
    Log(String),
    Progress(f32, String),
    Done(Result<builder::BuildOutcome, String>),
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages(ctx);
        self.draw(ctx);
    }
}

impl App {
    fn drain_messages(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Log(s) => self.log.push(s),
                    Msg::Progress(p, s) => {
                        self.progress = p;
                        self.status = s;
                    }
                    Msg::Done(r) => {
                        self.busy = false;
                        match r {
                            Ok(o) => {
                                self.log.push(format!("✔ 已创建：{}", o.portable_dir.display()));
                                if let Some(p) = &o.packed {
                                    self.log.push(format!("✔ 已打包：{}", p.display()));
                                }
                            }
                            Err(e) => self.log.push(format!("✖ 出错：{e}")),
                        }
                    }
                }
                ctx.request_repaint();
            }
        }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(20.0))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // 界内 logo（纸箱图标）
                    let logo = self.logo.get_or_insert_with(|| {
                        let icon = app_icon();
                        let tex = egui::ColorImage::from_rgba_unmultiplied(
                            [icon.width as usize, icon.height as usize],
                            &icon.rgba,
                        );
                        ctx.load_texture("pxe_logo", tex, egui::TextureOptions::default())
                    });
                    ui.add(
                        egui::Image::new(&*logo).fit_to_exact_size(egui::vec2(40.0, 40.0)),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("PortableXE").size(26.0).strong());
                        fluent::subtle(ui, "把 Windows 安装包变成便携应用 — Rust 版");
                    });
                });
                ui.add_space(16.0);

                // 输入区
                egui::Grid::new("inputs").num_columns(3).spacing([12.0, 10.0]).show(ui, |ui| {
                    let w = 400.0;
                    ui.label("安装包文件");
                    ui.add(egui::TextEdit::singleline(&mut self.input_file).desired_width(w).hint_text("选择 .exe / .msi / 压缩包…"));
                    if ui.button("浏览…").clicked() {
                        if let Some(f) = rfd::FileDialog::new()
                            .add_filter("可执行与安装包", &["exe", "msi", "msix", "appx", "7z", "zip", "rar"])
                            .pick_file()
                        {
                            self.input_file = f.display().to_string();
                            if self.app_name.is_empty() {
                                self.app_name = derive_name(&f);
                            }
                        }
                    }
                    ui.end_row();

                    ui.label("应用名称");
                    ui.add(egui::TextEdit::singleline(&mut self.app_name).desired_width(w).hint_text("便携应用的文件夹名"));
                    ui.end_row();

                    ui.label("输出目录");
                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(w));
                    if ui.button("浏览…").clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.output_dir = d.display().to_string();
                        }
                    }
                    ui.end_row();
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("打包为");
                    for (k, label) in [
                        (builder::PackKind::Dir, "文件夹"),
                        (builder::PackKind::Zip, "ZIP"),
                        (builder::PackKind::SevenZip, "7z"),
                    ] {
                        ui.selectable_value(&mut self.pack, k, label);
                    }
                });

                ui.add_space(6.0);
                ui.checkbox(
                    &mut self.deep_extract,
                    "深度解包（自动解内层最大的压缩档，解出主程序即停止）",
                );

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let btn = fluent::accent_button(ui, if self.busy { "正在处理…" } else { "创建便携应用" });
                    if btn.clicked() && !self.busy {
                        self.start_build();
                    }
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.add(egui::ProgressBar::new(self.progress).show_percentage().desired_height(14.0).desired_width(240.0));
                        ui.label(egui::RichText::new(&self.status).size(11.0).color(Color32::from_rgb(0x88, 0x88, 0x88)));
                    });
                });

                ui.add_space(16.0);
                if !self.log.is_empty() {
                    egui::Frame::group(ui.style())
                        .fill(Color32::from_rgb(0x14, 0x14, 0x14))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                for l in &self.log {
                                    let color = if l.starts_with("✔") {
                                        Color32::from_rgb(0x6C, 0xC6, 0x44)
                                    } else if l.starts_with("✖") {
                                        Color32::from_rgb(0xE8, 0x11, 0x23)
                                    } else {
                                        Color32::from_rgb(0xCC, 0xCC, 0xCC)
                                    };
                                    ui.label(egui::RichText::new(l).monospace().size(12.0).color(color));
                                }
                            });
                        });
                }
            });
    }

    fn start_build(&mut self) {
        if self.input_file.is_empty() || self.app_name.is_empty() || self.output_dir.is_empty() {
            self.log.push("✖ 请先填写安装包文件、应用名称和输出目录".into());
            return;
        }
        self.busy = true;
        self.log.clear();
        let (tx, rx) = mpsc::channel();
        self.tx = Some(tx.clone());
        self.rx = Some(rx);

        let file = PathBuf::from(&self.input_file);
        let name = self.app_name.clone();
        let out = PathBuf::from(&self.output_dir);
        let pack_kind = self.pack;
        let mut cfg = Config::default();
        cfg.deep_extract = self.deep_extract;
        std::thread::spawn(move || {
            let send = |m: Msg| tx.send(m).ok();
            let result = pipeline::run(&file, &name, &out, pack_kind, None, &cfg, &mut |p, msg| {
                send(Msg::Progress(p, msg.to_string()));
            })
            .map_err(|e| e.to_string());
            send(Msg::Done(result));
        });
    }
}

fn derive_name(file: &Path) -> String {
    pipeline::derive_name(file)
}

fn main() -> eframe::Result {
    let theme = fluent::FluentTheme { dark: true };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_title("PortableXE (Rust)")
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "PortableXE",
        opts,
        Box::new(move |_cc| {
            theme.apply(&_cc.egui_ctx);
            Ok(Box::new(App {
                output_dir: Config::default().output_dir.display().to_string(),
                deep_extract: true,
                ..Default::default()
            }))
        }),
    )
}

/// 内嵌的应用图标（纸箱）。窗口图标 + 界内 logo 共用同一份解码结果。
fn app_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../assets/app_icon.png");
    let img = image::load_from_memory(PNG).expect("embedded app icon must decode");
    let rgba = img.to_rgba8();
    egui::IconData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}
