//! Design tokens: every color and size the components use. No component
//! writes a color or a spacing by hand, so light and dark themes are only a
//! change of values here.

use apifast_domain::Method;
use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, ThemePreference};

/// Spacing, in points.
pub mod space {
    /// Between tightly related items.
    pub const XS: f32 = 2.0;
    /// Inside small controls.
    pub const S: f32 = 4.0;
    /// Between controls.
    pub const M: f32 = 8.0;
    /// Between groups.
    pub const L: f32 = 12.0;
    /// Around panels.
    pub const XL: f32 = 16.0;
}

/// Corner radii, in points.
pub mod radius {
    /// Badges and chips.
    pub const SMALL: u8 = 3;
    /// Buttons, inputs and panels.
    pub const MEDIUM: u8 = 5;
}

/// Font sizes, in points.
pub mod text {
    /// Badges and secondary labels.
    pub const SMALL: f32 = 11.0;
    /// Body text.
    pub const BODY: f32 = 13.0;
    /// Headings.
    pub const HEADING: f32 = 17.0;
}

/// Opacity of a badge's background tint, made of its own color.
pub const BADGE_TINT: f32 = 0.14;

/// Width of a method badge, so request names line up in lists.
pub const METHOD_BADGE_WIDTH: f32 = 52.0;

/// Colors of one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Window background.
    pub background: Color32,
    /// Panels and inputs.
    pub surface: Color32,
    /// Borders and separators.
    pub border: Color32,
    /// Main text.
    pub text: Color32,
    /// Secondary text.
    pub text_muted: Color32,
    /// Focus, selection and primary actions.
    pub accent: Color32,
    /// `GET`
    pub get: Color32,
    /// `POST`
    pub post: Color32,
    /// `PUT`
    pub put: Color32,
    /// `PATCH`
    pub patch: Color32,
    /// `DELETE`
    pub delete: Color32,
    /// Other methods.
    pub other_method: Color32,
    /// 2xx
    pub success: Color32,
    /// 1xx and 3xx
    pub info: Color32,
    /// 4xx
    pub warning: Color32,
    /// 5xx and errors
    pub danger: Color32,
    /// A `{{variable}}` that is defined, drawn on a tint of itself.
    pub variable: Color32,
    /// A `{{variable}}` that is not defined, underlined.
    pub variable_missing: Color32,
    /// A tag whose value is made when sending: `{{$random}}`, `{{?Ask}}`,
    /// `{{process.env.X}}` and functions.
    pub variable_generated: Color32,
    /// Code: JSON keys, XML attribute names and SPARQL variables.
    pub syntax_key: Color32,
    /// Code: strings and attribute values.
    pub syntax_string: Color32,
    /// Code: numbers.
    pub syntax_number: Color32,
    /// Code: `true`, `false`, `null`, XML tag names and SPARQL keywords.
    pub syntax_keyword: Color32,
}

/// The light theme.
pub const LIGHT: Palette = Palette {
    background: Color32::from_rgb(0xf7, 0xf7, 0xf8),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xd9, 0xdb, 0xe0),
    text: Color32::from_rgb(0x1f, 0x23, 0x28),
    text_muted: Color32::from_rgb(0x5c, 0x63, 0x6e),
    accent: Color32::from_rgb(0x1b, 0x55, 0xb3),
    get: Color32::from_rgb(0x15, 0x69, 0x2b),
    post: Color32::from_rgb(0x8a, 0x51, 0x00),
    put: Color32::from_rgb(0x1b, 0x55, 0xb3),
    patch: Color32::from_rgb(0x7a, 0x3e, 0xc0),
    delete: Color32::from_rgb(0xa8, 0x24, 0x24),
    other_method: Color32::from_rgb(0x4d, 0x56, 0x63),
    success: Color32::from_rgb(0x15, 0x69, 0x2b),
    info: Color32::from_rgb(0x1b, 0x55, 0xb3),
    warning: Color32::from_rgb(0x8a, 0x51, 0x00),
    danger: Color32::from_rgb(0xa8, 0x24, 0x24),
    variable: Color32::from_rgb(0x0b, 0x6e, 0x74),
    variable_missing: Color32::from_rgb(0xa8, 0x24, 0x24),
    variable_generated: Color32::from_rgb(0x7a, 0x3e, 0xc0),
    syntax_key: Color32::from_rgb(0x1b, 0x55, 0xb3),
    syntax_string: Color32::from_rgb(0x15, 0x69, 0x2b),
    syntax_number: Color32::from_rgb(0x8a, 0x51, 0x00),
    syntax_keyword: Color32::from_rgb(0x9b, 0x2c, 0x6b),
};

/// The dark theme.
pub const DARK: Palette = Palette {
    background: Color32::from_rgb(0x16, 0x18, 0x1c),
    surface: Color32::from_rgb(0x1f, 0x22, 0x27),
    border: Color32::from_rgb(0x33, 0x37, 0x3e),
    text: Color32::from_rgb(0xe6, 0xe8, 0xeb),
    text_muted: Color32::from_rgb(0x9a, 0xa1, 0xac),
    accent: Color32::from_rgb(0x6c, 0xa6, 0xff),
    get: Color32::from_rgb(0x5f, 0xd0, 0x7e),
    post: Color32::from_rgb(0xf2, 0xb5, 0x4a),
    put: Color32::from_rgb(0x6c, 0xa6, 0xff),
    patch: Color32::from_rgb(0xc3, 0x9b, 0xff),
    delete: Color32::from_rgb(0xff, 0x7b, 0x72),
    other_method: Color32::from_rgb(0xa8, 0xb0, 0xbb),
    success: Color32::from_rgb(0x5f, 0xd0, 0x7e),
    info: Color32::from_rgb(0x6c, 0xa6, 0xff),
    warning: Color32::from_rgb(0xf2, 0xb5, 0x4a),
    danger: Color32::from_rgb(0xff, 0x7b, 0x72),
    variable: Color32::from_rgb(0x5c, 0xcf, 0xd4),
    variable_missing: Color32::from_rgb(0xff, 0x7b, 0x72),
    variable_generated: Color32::from_rgb(0xc3, 0x9b, 0xff),
    syntax_key: Color32::from_rgb(0x6c, 0xa6, 0xff),
    syntax_string: Color32::from_rgb(0x8f, 0xd1, 0x9e),
    syntax_number: Color32::from_rgb(0xf2, 0xb5, 0x4a),
    syntax_keyword: Color32::from_rgb(0xff, 0x8f, 0xc8),
};

/// The palette for the theme `ui` is drawn with.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

/// A color written as `#rrggbb`, such as an environment's.
pub fn parse_color(color: &str) -> Option<Color32> {
    let hex = color.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// Color of a method badge.
pub fn method_color(palette: &Palette, method: &Method) -> Color32 {
    match method {
        Method::Get => palette.get,
        Method::Post => palette.post,
        Method::Put => palette.put,
        Method::Patch => palette.patch,
        Method::Delete => palette.delete,
        _ => palette.other_method,
    }
}

/// Color of a status code, by class.
pub fn status_color(palette: &Palette, status: u16) -> Color32 {
    match status {
        200..=299 => palette.success,
        400..=499 => palette.warning,
        500..=599 => palette.danger,
        _ => palette.info,
    }
}

/// egui visuals built from a palette.
pub fn visuals(theme: Theme) -> egui::Visuals {
    let (mut visuals, p) = match theme {
        Theme::Dark => (egui::Visuals::dark(), &DARK),
        Theme::Light => (egui::Visuals::light(), &LIGHT),
    };
    visuals.panel_fill = p.background;
    // Windows take the background, so the fields in them (on `surface`)
    // stand out; with both on `surface` they vanish in the light theme.
    visuals.window_fill = p.background;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.extreme_bg_color = p.surface;
    visuals.override_text_color = Some(p.text);
    visuals.hyperlink_color = p.accent;
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    // Text fields draw their frame with this stroke: without it, an empty
    // field cannot be seen.
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(radius::MEDIUM);
    }
    visuals
}

/// Which theme the user wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    /// The one the system uses.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl ThemeChoice {
    /// As egui takes it.
    pub fn preference(self) -> ThemePreference {
        match self {
            Self::System => ThemePreference::System,
            Self::Light => ThemePreference::Light,
            Self::Dark => ThemePreference::Dark,
        }
    }
}

/// Uses `choice` from the next frame on.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(choice.preference());
}

/// Installs both themes and follows the system's choice.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light));
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark));
    apply(ctx, ThemeChoice::System);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(space::M, space::S);
        style.spacing.button_padding = egui::vec2(space::M, space::S);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 relative luminance.
    fn luminance(c: Color32) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Text colors must reach WCAG AA (4.5:1) on both backgrounds of their
    /// theme. Colors never carry meaning alone (badges show text too), but
    /// that text must be readable.
    #[test]
    fn text_colors_reach_wcag_aa_in_both_themes() {
        for (name, p) in [("light", LIGHT), ("dark", DARK)] {
            let texts = [
                ("text", p.text),
                ("text_muted", p.text_muted),
                ("accent", p.accent),
                ("get", p.get),
                ("post", p.post),
                ("put", p.put),
                ("patch", p.patch),
                ("delete", p.delete),
                ("other_method", p.other_method),
                ("success", p.success),
                ("info", p.info),
                ("warning", p.warning),
                ("danger", p.danger),
                ("variable", p.variable),
                ("variable_missing", p.variable_missing),
                ("variable_generated", p.variable_generated),
                ("syntax_key", p.syntax_key),
                ("syntax_string", p.syntax_string),
                ("syntax_number", p.syntax_number),
                ("syntax_keyword", p.syntax_keyword),
            ];
            for (text, color) in texts {
                for (bg, background) in [("background", p.background), ("surface", p.surface)] {
                    let ratio = contrast(color, background);
                    assert!(ratio >= 4.5, "{name}: {text} on {bg} is {ratio:.2}:1");
                }
            }
        }
    }

    /// `color` at `alpha` over an opaque `background`, as egui blends a
    /// `gamma_multiply`'d fill.
    fn over(color: Color32, alpha: f32, background: Color32) -> Color32 {
        let tint = color.gamma_multiply(alpha);
        let mix = |fg: u8, bg: u8| {
            // Premultiplied: `fg` already carries the alpha.
            let blended = u16::from(fg) + u16::from(bg) * (255 - u16::from(tint.a())) / 255;
            u8::try_from(blended).unwrap_or(u8::MAX)
        };
        Color32::from_rgb(
            mix(tint.r(), background.r()),
            mix(tint.g(), background.g()),
            mix(tint.b(), background.b()),
        )
    }

    /// Status badges draw their text on a tint of the same color, which
    /// lowers the contrast: it must still reach WCAG AA.
    #[test]
    fn badge_text_reaches_wcag_aa_on_its_tint() {
        for (name, p) in [("light", LIGHT), ("dark", DARK)] {
            for (status, color) in [
                ("success", p.success),
                ("info", p.info),
                ("warning", p.warning),
                ("danger", p.danger),
                ("variable", p.variable),
            ] {
                for background in [p.background, p.surface] {
                    let ratio = contrast(color, over(color, BADGE_TINT, background));
                    assert!(ratio >= 4.5, "{name}: {status} badge is {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn colors_are_read_from_hex() {
        assert_eq!(
            parse_color(" #22aa5F "),
            Some(Color32::from_rgb(0x22, 0xaa, 0x5f))
        );
        for bad in ["22aa55", "#22aa5", "#gg0000", "#ééé", ""] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn each_method_and_status_class_has_its_color() {
        let p = &LIGHT;
        assert_eq!(method_color(p, &Method::Get), p.get);
        assert_eq!(
            method_color(p, &Method::Custom("PURGE".into())),
            p.other_method
        );
        assert_eq!(status_color(p, 204), p.success);
        assert_eq!(status_color(p, 302), p.info);
        assert_eq!(status_color(p, 404), p.warning);
        assert_eq!(status_color(p, 503), p.danger);
    }
}
