//! Fluent 2 设计语言 —— egui 主题实现。
//!
//! 设计 token 对齐 Fluent 2（WinUI 3）：
//! - 色板：深/浅两套，accent 为 Windows 蓝 #0078D4
//! - 字阶：Type Ramp（Caption 12 / Body 14 / Body Strong / Subtitle 20 / Title 28）
//! - 圆角：Control 4px / Overlay 8px（Fluent 的 2 级圆角体系）
//! - 层级：底色 Layering（solid base + card 提亮），而非浓重阴影
//! - 交互：hover 提亮 5%、按下 3%，与 Fluent 控件行为一致

use egui::{Color32, CornerRadius, FontId, Margin, Style, Vec2, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD4);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x1A, 0x86, 0xDA);

pub struct FluentTheme {
    pub dark: bool,
}

impl FluentTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = Style::default();
        let (bg, card, stroke, text, _subtle_text) = if self.dark {
            (
                Color32::from_rgb(0x20, 0x20, 0x20), // Mica-like base
                Color32::from_rgb(0x2B, 0x2B, 0x2B), // card layer
                Color32::from_rgb(0x3D, 0x3D, 0x3D),
                Color32::from_rgb(0xFF, 0xFF, 0xFF),
                Color32::from_rgb(0xC8, 0xC8, 0xC8),
            )
        } else {
            (
                Color32::from_rgb(0xF3, 0xF3, 0xF3),
                Color32::from_rgb(0xFF, 0xFF, 0xFF),
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
                Color32::from_rgb(0x1A, 0x1A, 0x1A),
                Color32::from_rgb(0x60, 0x60, 0x60),
            )
        };

        let v = &mut style.visuals;
        *v = Visuals {
            dark_mode: self.dark,
            override_text_color: Some(text),
            window_fill: card,
            panel_fill: bg,
            extreme_bg_color: if self.dark {
                Color32::from_rgb(0x14, 0x14, 0x14)
            } else {
                Color32::WHITE
            },
            faint_bg_color: if self.dark {
                Color32::from_rgb(0x33, 0x33, 0x33)
            } else {
                Color32::from_rgb(0xE8, 0xE8, 0xE8)
            },
            widgets: egui::style::Widgets {
                noninteractive: widget_style(bg, stroke, text, 4.0),
                inactive: widget_style(card, stroke, text, 4.0),
                hovered: widget_style(
                    if self.dark {
                        Color32::from_rgb(0x35, 0x35, 0x35)
                    } else {
                        Color32::from_rgb(0xF5, 0xF5, 0xF5)
                    },
                    stroke,
                    text,
                    4.0,
                ),
                active: widget_style(
                    if self.dark {
                        Color32::from_rgb(0x3D, 0x3D, 0x3D)
                    } else {
                        Color32::from_rgb(0xEB, 0xEB, 0xEB)
                    },
                    stroke,
                    text,
                    4.0,
                ),
                open: widget_style(card, stroke, text, 4.0),
            },
            selection: egui::style::Selection {
                bg_fill: ACCENT,
                stroke: egui::Stroke::new(1.0_f32, ACCENT_HOVER),
            },
            window_stroke: egui::Stroke::new(1.0_f32, stroke),
            window_corner_radius: CornerRadius::same(8),
            menu_corner_radius: CornerRadius::same(8),
            ..Visuals::dark()
        };

        // 字阶：Type Ramp —— 优先加载系统中文黑体，避免 CJK 显示为豆腐块。
        // Proportional 与 Monospace 分别取首选字体（此前 mono 字体被插到
        // Proportional 首位，正文会变成等宽渲染）。
        let mut fonts = egui::FontDefinitions::default();
        let find_font = |candidates: &[&str]| -> Option<Vec<u8>> {
            for p in [
                "C:\\Windows\\Fonts\\",
                "/usr/share/fonts/",
                "/System/Library/Fonts/",
            ] {
                for n in candidates {
                    if let Ok(data) = std::fs::read(format!("{p}{n}")) {
                        return Some(data);
                    }
                }
            }
            None
        };
        if let Some(data) = find_font(&[
            "msyh.ttc",
            "msyh.ttf",
            "simhei.ttf",
            "simsum.ttc",
            "PingFang SC.ttf",
        ]) {
            fonts
                .font_data
                .insert("cn".into(), egui::FontData::from_owned(data).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cn".into());
        }
        if let Some(data) = find_font(&["msyh.ttc", "simhei.ttf", "PingFang SC.ttf"]) {
            fonts
                .font_data
                .insert("cn_mono".into(), egui::FontData::from_owned(data).into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cn_mono".into());
        }
        ctx.set_fonts(fonts);
        let mut text_styles = style.text_styles.clone();
        text_styles.insert(
            egui::TextStyle::Body,
            FontId::new(14.0, egui::FontFamily::Proportional),
        );
        text_styles.insert(
            egui::TextStyle::Button,
            FontId::new(14.0, egui::FontFamily::Proportional),
        );
        text_styles.insert(
            egui::TextStyle::Small,
            FontId::new(12.0, egui::FontFamily::Proportional),
        );
        text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(20.0, egui::FontFamily::Proportional), // Subtitle
        );
        style.text_styles = text_styles;
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.menu_margin = Margin::same(6);

        ctx.set_style(style);
    }
}

fn widget_style(
    bg: Color32,
    stroke: Color32,
    text: Color32,
    radius: f32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: egui::Stroke::new(1.0_f32, stroke),
        fg_stroke: egui::Stroke::new(1.0_f32, text),
        corner_radius: CornerRadius::same(radius as u8),
        expansion: 0.0,
    }
}

/// Fluent 风格的 accent 主按钮（Filled Button）
pub fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(text)
        .fill(ACCENT)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CornerRadius::same(4))
        .min_size(Vec2::new(0.0, 32.0));
    ui.add_sized(Vec2::new(160.0, 32.0), btn)
}

/// 副文本（subtle text）
pub fn subtle(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(Color32::GRAY));
}
