//! Drawing primitives: rounded rectangles, suit symbols, playing cards,
//! chips and text. Everything is drawn with macroquad shapes, so the game
//! ships without any image assets.

use crate::cards::{Card, Suit};
use macroquad::prelude::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

pub fn shade(c: Color, f: f32) -> Color {
    Color::new(
        (c.r * f).min(1.0),
        (c.g * f).min(1.0),
        (c.b * f).min(1.0),
        c.a,
    )
}

pub mod pal {
    use super::rgb;
    use macroquad::prelude::Color;

    pub const BACKDROP: Color = rgb(13, 15, 20);
    pub const FELT: Color = rgb(20, 98, 70);
    pub const RIM: Color = rgb(96, 60, 38);
    pub const RIM_DARK: Color = rgb(52, 32, 22);
    pub const CARD: Color = rgb(250, 248, 243);
    pub const CARD_EDGE: Color = rgb(176, 172, 164);
    pub const INK_RED: Color = rgb(204, 38, 46);
    pub const INK_BLACK: Color = rgb(24, 26, 33);
    pub const BACK: Color = rgb(34, 56, 128);
    pub const BACK_LIGHT: Color = rgb(70, 98, 178);
    pub const GOLD: Color = rgb(246, 196, 72);
    /// Colour used for everything the peek options reveal.
    pub const XRAY: Color = rgb(88, 214, 232);
    pub const PLATE: Color = rgb(18, 20, 26);
    pub const PLATE_EDGE: Color = rgb(58, 62, 74);
    pub const TEXT: Color = rgb(238, 240, 244);
    pub const MUTED: Color = rgb(150, 156, 170);
    pub const CHIP_RED: Color = rgb(206, 58, 62);
    pub const CHIP_BLUE: Color = rgb(52, 110, 204);
    pub const YOU: Color = rgb(96, 170, 255);
    pub const BOT: Color = rgb(255, 138, 110);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
}

pub struct Gfx {
    pub bold: Font,
    pub heavy: Font,
}

impl Gfx {
    pub fn load() -> Gfx {
        let bold = load_ttf_font_from_bytes(include_bytes!("../assets/Inter-Bold.otf"))
            .expect("embedded font Inter-Bold");
        let heavy = load_ttf_font_from_bytes(include_bytes!("../assets/Inter-ExtraBold.otf"))
            .expect("embedded font Inter-ExtraBold");
        Gfx { bold, heavy }
    }

    pub fn width(&self, s: &str, size: f32, heavy: bool) -> f32 {
        let font = if heavy { &self.heavy } else { &self.bold };
        measure_text(s, Some(font), size.round().max(1.0) as u16, 1.0).width
    }

    /// Draws text vertically centred on `y`. Returns the text width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
        heavy: bool,
    ) -> f32 {
        let font = if heavy { &self.heavy } else { &self.bold };
        let font_size = size.round().max(1.0) as u16;
        let width = measure_text(s, Some(font), font_size, 1.0).width;
        let x0 = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
        };
        // Inter's capitals are ~0.73 em tall, so this centres them on `y`.
        let baseline = y + size * 0.36;
        draw_text_ex(
            s,
            x0.round(),
            baseline.round(),
            TextParams {
                font: Some(font),
                font_size,
                color,
                ..Default::default()
            },
        );
        width
    }
}

pub fn disc(x: f32, y: f32, r: f32, color: Color) {
    let sides = (r * 1.2).clamp(14.0, 72.0) as u8;
    draw_poly(x, y, sides, r, 0.0, color);
}

fn rrect_outline(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Vec2> {
    let r = r.max(0.0).min(w * 0.5).min(h * 0.5);
    let seg = ((r * 0.55).ceil() as usize).clamp(3, 18);
    let corners = [
        (x + w - r, y + r, -90.0f32),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 90.0),
        (x + r, y + r, 180.0),
    ];
    let mut pts = Vec::with_capacity(4 * (seg + 1));
    for (cx, cy, start) in corners {
        for i in 0..=seg {
            let a = (start + 90.0 * i as f32 / seg as f32).to_radians();
            pts.push(vec2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    pts
}

/// Filled rounded rectangle. Drawn as one fan without overlapping pieces,
/// so translucent colours blend evenly.
pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let pts = rrect_outline(x, y, w, h, r);
    let center = vec2(x + w / 2.0, y + h / 2.0);
    for i in 0..pts.len() {
        draw_triangle(center, pts[i], pts[(i + 1) % pts.len()], color);
    }
}

pub fn rrect_stroke(x: f32, y: f32, w: f32, h: f32, r: f32, thickness: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let pts = rrect_outline(x, y, w, h, r);
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        draw_line(a.x, a.y, b.x, b.y, thickness, color);
    }
}

/// A suit symbol centred on (cx, cy), roughly `2u` tall. `sx` squeezes it
/// horizontally (used while a card is flipping over).
pub fn suit(s: Suit, cx: f32, cy: f32, u: f32, sx: f32, color: Color) {
    let p = |x: f32, y: f32| vec2(cx + x * u * sx, cy + y * u);
    // Circles become slightly narrow ellipses mid-flip; a plain disc with a
    // scaled radius is close enough for the few frames that lasts.
    let lobe =
        |x: f32, y: f32, r: f32| disc(cx + x * u * sx, cy + y * u, r * u * sx.max(0.3), color);

    match s {
        Suit::Diamonds => {
            draw_triangle(p(0.0, -1.0), p(0.72, 0.0), p(-0.72, 0.0), color);
            draw_triangle(p(0.0, 1.0), p(0.72, 0.0), p(-0.72, 0.0), color);
        }
        Suit::Hearts => {
            lobe(-0.5, -0.3, 0.52);
            lobe(0.5, -0.3, 0.52);
            // The two lower edges are tangent to the lobes.
            let (tl, tr) = (p(-0.879, 0.054), p(0.879, 0.054));
            draw_triangle(tl, tr, p(0.0, 1.0), color);
            draw_triangle(tl, p(-0.5, -0.3), p(0.5, -0.3), color);
            draw_triangle(tl, p(0.5, -0.3), tr, color);
        }
        Suit::Spades => {
            lobe(-0.5, 0.2, 0.52);
            lobe(0.5, 0.2, 0.52);
            let (bl, br) = (p(-0.879, -0.154), p(0.879, -0.154));
            draw_triangle(bl, br, p(0.0, -1.1), color);
            draw_triangle(bl, p(-0.5, 0.2), p(0.5, 0.2), color);
            draw_triangle(bl, p(0.5, 0.2), br, color);
            draw_triangle(p(0.0, 0.1), p(-0.42, 1.0), p(0.42, 1.0), color);
        }
        Suit::Clubs => {
            lobe(0.0, -0.5, 0.47);
            lobe(-0.52, 0.2, 0.47);
            lobe(0.52, 0.2, 0.47);
            lobe(0.0, 0.0, 0.3);
            draw_triangle(p(0.0, -0.1), p(-0.4, 1.0), p(0.4, 1.0), color);
        }
    }
}

/// How a card should be drawn. `sx` in 0..=1 is the horizontal squeeze used
/// for the flip animation; `compact` picks a layout that stays legible on
/// very small cards.
#[derive(Clone, Copy)]
pub struct CardLook {
    pub sx: f32,
    pub compact: bool,
    pub glow: Option<Color>,
    pub dim: f32,
}

impl Default for CardLook {
    fn default() -> Self {
        CardLook {
            sx: 1.0,
            compact: false,
            glow: None,
            dim: 0.0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn card_body(
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    look: &CardLook,
    edge: Color,
    fill: Color,
    inset: f32,
) {
    let ww = w * look.sx.max(0.03);
    let (x, y) = (cx - ww / 2.0, cy - h / 2.0);
    let r = w * 0.09;
    if let Some(glow) = look.glow {
        let g = w * 0.045;
        rrect(
            x - g * 2.2,
            y - g * 2.2,
            ww + g * 4.4,
            h + g * 4.4,
            r + g * 2.2,
            alpha(glow, 0.22),
        );
        rrect(x - g, y - g, ww + g * 2.0, h + g * 2.0, r + g, glow);
    } else {
        rrect(
            x + w * 0.02,
            y + w * 0.045,
            ww,
            h,
            r,
            Color::new(0.0, 0.0, 0.0, 0.30),
        );
    }
    rrect(x, y, ww, h, r, edge);
    let b = inset.min(ww * 0.4);
    rrect(
        x + b,
        y + b,
        ww - 2.0 * b,
        h - 2.0 * b,
        (r - b).max(0.0),
        fill,
    );
}

fn card_dim(cx: f32, cy: f32, w: f32, h: f32, look: &CardLook) {
    if look.dim > 0.0 {
        let ww = w * look.sx.max(0.03);
        rrect(
            cx - ww / 2.0,
            cy - h / 2.0,
            ww,
            h,
            w * 0.09,
            Color::new(0.03, 0.05, 0.06, look.dim),
        );
    }
}

pub fn card_face(g: &Gfx, card: Card, cx: f32, cy: f32, w: f32, h: f32, look: CardLook) {
    card_body(
        cx,
        cy,
        w,
        h,
        &look,
        pal::CARD_EDGE,
        pal::CARD,
        (w * 0.014).max(1.0),
    );
    let sx = look.sx;
    if sx > 0.5 {
        let ink = if card.suit.is_red() {
            pal::INK_RED
        } else {
            pal::INK_BLACK
        };
        let rank = card.rank_str();
        let narrow = if rank.len() > 1 { 0.86 } else { 1.0 };
        let top = cy - h / 2.0;
        if look.compact {
            g.text(
                rank,
                cx,
                top + h * 0.29,
                h * 0.40 * narrow,
                ink,
                Align::Center,
                true,
            );
            suit(card.suit, cx, top + h * 0.70, h * 0.165, sx, ink);
        } else {
            let corner_x = cx - w * 0.27 * sx;
            g.text(
                rank,
                corner_x,
                top + h * 0.165,
                h * 0.25 * narrow,
                ink,
                Align::Center,
                true,
            );
            suit(card.suit, corner_x, top + h * 0.375, h * 0.068, sx, ink);
            suit(
                card.suit,
                cx + w * 0.10 * sx,
                cy + h * 0.16,
                h * 0.20,
                sx,
                ink,
            );
        }
    }
    card_dim(cx, cy, w, h, &look);
}

pub fn card_back(cx: f32, cy: f32, w: f32, h: f32, look: CardLook) {
    card_body(
        cx,
        cy,
        w,
        h,
        &look,
        rgb(232, 234, 240),
        pal::BACK,
        w * 0.055,
    );
    let sx = look.sx;
    if sx > 0.5 {
        let (iw, ih) = (w * 0.74, h - w * 0.26);
        rrect_stroke(
            cx - iw * sx / 2.0,
            cy - ih / 2.0,
            iw * sx,
            ih,
            w * 0.04,
            (w * 0.016).max(1.0),
            alpha(pal::BACK_LIGHT, 0.9),
        );
        let (cols, rows) = (3, 5);
        for row in 0..rows {
            for col in 0..cols {
                let fx = (col as f32 + 0.5) / cols as f32 - 0.5;
                let fy = (row as f32 + 0.5) / rows as f32 - 0.5;
                let (px, py) = (cx + fx * iw * 0.86 * sx, cy + fy * ih * 0.90);
                let r = w * 0.075;
                draw_triangle(
                    vec2(px, py - r),
                    vec2(px + r * 0.8 * sx, py),
                    vec2(px - r * 0.8 * sx, py),
                    pal::BACK_LIGHT,
                );
                draw_triangle(
                    vec2(px, py + r),
                    vec2(px + r * 0.8 * sx, py),
                    vec2(px - r * 0.8 * sx, py),
                    pal::BACK_LIGHT,
                );
            }
        }
    }
    card_dim(cx, cy, w, h, &look);
}

pub fn chip(cx: f32, cy: f32, r: f32, color: Color) {
    disc(cx, cy + r * 0.16, r, Color::new(0.0, 0.0, 0.0, 0.35));
    disc(cx, cy, r, shade(color, 0.62));
    disc(cx, cy, r * 0.93, color);
    for i in 0..6 {
        let a = (i as f32 * 60.0 + 15.0).to_radians();
        disc(
            cx + a.cos() * r * 0.76,
            cy + a.sin() * r * 0.76,
            r * 0.13,
            rgb(245, 245, 245),
        );
    }
    disc(cx, cy, r * 0.56, rgb(245, 245, 245));
    disc(cx, cy, r * 0.47, color);
}

/// A little pile of up to five chips, growing upwards from (cx, cy).
pub fn chip_stack(cx: f32, cy: f32, r: f32, count: u32, color: Color) {
    for i in 0..count.min(5) {
        chip(cx, cy - i as f32 * r * 0.30, r, color);
    }
}

pub fn pill(x: f32, y: f32, w: f32, h: f32, fill: Color, edge: Option<(f32, Color)>) {
    rrect(x, y, w, h, h / 2.0, fill);
    if let Some((t, c)) = edge {
        rrect_stroke(x, y, w, h, h / 2.0, t, c);
    }
}
