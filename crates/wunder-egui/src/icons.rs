//! The same Font Awesome glyphs used by the Hive web workbench.
//!
//! Keep icons in a dedicated family: private-use characters must never be
//! resolved through the user's text font. The existing OFL font is embedded
//! once and egui only rasterizes glyphs that are actually visible.

use egui::{Align2, Color32, FontFamily, FontId, Painter, Rect, Response, Sense, Ui, Vec2};

pub(crate) const FONT_NAME: &str = "HiveIcons";
pub(crate) const FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../frontend/public/third/fontawesome-free-6.7.2-web/webfonts/fa-solid-900.ttf"
));

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    ChevronRight,
    ChevronDown,
    Folder,
    FolderOpen,
    Workspace,
    Cloud,
    Settings,
    More,
    Copy,
    Send,
    ArrowUp,
    Stop,
    Paperclip,
    Microphone,
    Refresh,
    Check,
    CircleCheck,
    Search,
    Close,
    File,
    Archive,
    Link,
    Tool,
    Clock,
    Brain,
    User,
    Agent,
    Desktop,
    Camera,
    Terminal,
    Moon,
    Sun,
    Palette,
    Help,
    Bolt,
    Shield,
    Pen,
    Paw,
    Database,
    Sliders,
    Warning,
    Upload,
    Download,
    Star,
    List,
    IdCard,
    ScrewdriverWrench,
    Plug,
    SatelliteDish,
    ExternalLink,
    Eye,
}

impl Icon {
    fn glyph(self) -> &'static str {
        match self {
            Self::Plus => "+",
            Self::ChevronRight => "\u{f054}",
            Self::ChevronDown => "\u{f078}",
            Self::Folder | Self::Workspace => "\u{f07b}",
            Self::FolderOpen => "\u{f07c}",
            Self::Cloud => "\u{f0c2}",
            Self::Settings => "\u{f013}",
            Self::More => "\u{f141}",
            Self::Copy => "\u{f0c5}",
            Self::Send => "\u{f1d8}",
            Self::ArrowUp => "\u{f062}",
            Self::Stop => "\u{f04d}",
            Self::Paperclip => "\u{f0c6}",
            Self::Microphone => "\u{f130}",
            Self::Refresh => "\u{f2f9}",
            Self::Check => "\u{f00c}",
            Self::CircleCheck => "\u{f058}",
            Self::Search => "\u{f002}",
            Self::Close => "\u{f00d}",
            Self::File => "\u{f15c}",
            Self::Archive => "\u{f187}",
            Self::Link => "\u{f0c1}",
            Self::Tool => "\u{f0ad}",
            Self::Clock => "\u{f017}",
            Self::Brain => "\u{f5dc}",
            Self::User => "\u{f007}",
            Self::Agent => "\u{f544}",
            Self::Desktop => "\u{f390}",
            Self::Camera => "\u{f030}",
            Self::Terminal => "\u{f120}",
            Self::Moon => "\u{f186}",
            Self::Sun => "\u{f185}",
            Self::Palette => "\u{f53f}",
            Self::Help => "\u{f059}",
            Self::Bolt => "\u{f0e7}",
            Self::Shield => "\u{f3ed}",
            Self::Pen => "\u{f304}",
            Self::Paw => "\u{f1b0}",
            Self::Database => "\u{f1c0}",
            Self::Sliders => "\u{f1de}",
            Self::Warning => "\u{f06a}",
            Self::Upload => "\u{f093}",
            Self::Download => "\u{f019}",
            Self::Star => "\u{f005}",
            Self::List => "\u{f0ae}",
            Self::IdCard => "\u{f2c2}",
            Self::ScrewdriverWrench => "\u{f7d9}",
            Self::Plug => "\u{f1e6}",
            Self::SatelliteDish => "\u{f7c0}",
            Self::ExternalLink => "\u{f35d}",
            Self::Eye => "\u{f06e}",
        }
    }
}

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::new(
            rect.height().min(rect.width()),
            FontFamily::Name(FONT_NAME.into()),
        ),
        color,
    );
}

pub fn button(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, crate::theme::tokens().hover);
    }
    paint(
        ui.painter(),
        rect.shrink((size - 13.0).max(0.0) / 2.0),
        icon,
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
