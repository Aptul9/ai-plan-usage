use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use once_cell::sync::OnceCell;
use std::f32::consts::PI;
use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::color::{
    color_for_window, parse_iso_secs, HEX_AMBER, HEX_GREEN, HEX_RED, PARAMS_SESSION, PARAMS_WEEKLY,
    SESSION_5H_SECS, WEEKLY_7D_SECS,
};

const SIZE: u32 = 64;

#[derive(Debug, Clone)]
pub struct IconPayload {
    pub session_pct: Option<f64>,
    pub session_color: u32,
    pub weekly_pct: Option<f64>,
    pub weekly_color: u32,
    pub error: bool,
}

fn pick_critical(a: u32, b: u32) -> u32 {
    let rank = |c: u32| match c {
        HEX_RED => 3,
        HEX_AMBER => 2,
        HEX_GREEN => 1,
        _ => 0,
    };
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

fn rgb(hex: u32) -> Color {
    Color::from_rgba8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        255,
    )
}

fn font_bytes() -> &'static [u8] {
    static CACHE: OnceCell<&'static [u8]> = OnceCell::new();
    CACHE.get_or_init(|| {
        let path = std::path::Path::new("C:\\Windows\\Fonts\\segoeuib.ttf");
        match std::fs::read(path) {
            Ok(v) => Box::leak(v.into_boxed_slice()),
            Err(_) => &[],
        }
    })
}

fn render_text_center(pixmap: &mut Pixmap, text: &str, cx: f32, cy: f32, px: f32, color: u32) {
    let bytes = font_bytes();
    if bytes.is_empty() {
        return;
    }
    let Ok(font) = FontRef::try_from_slice(bytes) else {
        return;
    };
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);

    let mut total_w = 0.0f32;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        total_w += scaled.h_advance(id);
    }
    // Center digit cap-box at (cx, cy). Cap height is ~ 0.72 * ascent for Segoe UI.
    let ascent = scaled.ascent();
    let cap_h = ascent * 0.72;
    let baseline_x = cx - total_w / 2.0;
    let baseline_y = cy + cap_h / 2.0;

    let (r, g, b) = (
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    );

    let mut caret_x = baseline_x;
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(caret_x, baseline_y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px_x = (bounds.min.x as i32) + gx as i32;
                let px_y = (bounds.min.y as i32) + gy as i32;
                if px_x < 0 || px_y < 0 || px_x as u32 >= SIZE || px_y as u32 >= SIZE {
                    return;
                }
                let alpha = (cov * 255.0).clamp(0.0, 255.0) as u8;
                if alpha == 0 {
                    return;
                }
                let data = pixmap.pixels_mut();
                let idx = (px_y as u32 * SIZE + px_x as u32) as usize;
                // simple over-compositing
                let existing = data[idx];
                let er = existing.red();
                let eg = existing.green();
                let eb = existing.blue();
                let ea = existing.alpha();
                let a = alpha as f32 / 255.0;
                let nr = (r as f32 * a + er as f32 * (1.0 - a)) as u8;
                let ng = (g as f32 * a + eg as f32 * (1.0 - a)) as u8;
                let nb = (b as f32 * a + eb as f32 * (1.0 - a)) as u8;
                let na = ea.max(alpha);
                data[idx] =
                    tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na).unwrap_or(existing);
            });
        }
        caret_x += scaled.h_advance(glyph_id);
    }
}

fn filled_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: u32) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(rgb(color));
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn ring(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, width: f32, color: u32) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(rgb(color));
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn arc(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, width: f32, color: u32, pct: f32) {
    if pct <= 0.0 {
        return;
    }
    let pct = pct.clamp(0.0, 100.0);
    let steps = (60.0 * pct / 100.0).max(2.0) as usize;
    let mut pb = PathBuilder::new();
    let start = -PI / 2.0;
    let end = start + (pct / 100.0) * PI * 2.0;
    pb.move_to(cx + r * start.cos(), cy + r * start.sin());
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let a = start + (end - start) * t;
        pb.line_to(cx + r * a.cos(), cy + r * a.sin());
    }
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(rgb(color));
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn filled_rect(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: u32) {
    let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(rgb(color));
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn stroked_rect(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, width: f32, color: u32) {
    let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let path = PathBuilder::from_rect(rect);
    let mut paint = Paint::default();
    paint.set_color(rgb(color));
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

pub fn render(style: &str, payload: &IconPayload) -> Vec<u8> {
    let mut pixmap = Pixmap::new(SIZE, SIZE).expect("alloc pixmap");
    pixmap.fill(Color::TRANSPARENT);
    let cx = SIZE as f32 / 2.0;
    let cy = SIZE as f32 / 2.0;

    if payload.error {
        filled_circle(&mut pixmap, cx, cy, 28.0, 0x666666);
        // draw X via two stroked lines
        let mut pb = PathBuilder::new();
        pb.move_to(20.0, 20.0);
        pb.line_to(44.0, 44.0);
        pb.move_to(44.0, 20.0);
        pb.line_to(20.0, 44.0);
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(rgb(0xffffff));
            paint.anti_alias = true;
            let stroke = Stroke {
                width: 4.0,
                line_cap: LineCap::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
        return pixmap.encode_png().unwrap_or_default();
    }

    let s_pct = payload.session_pct.unwrap_or(0.0) as f32;
    let w_pct = payload.weekly_pct.unwrap_or(0.0) as f32;
    let s_col = payload.session_color;
    let w_col = payload.weekly_color;

    match style {
        "solid" => {
            let col = pick_critical(payload.session_color, payload.weekly_color);
            filled_circle(&mut pixmap, cx, cy, 28.0, col);
        }
        "number" => {
            filled_circle(&mut pixmap, cx, cy, 30.0, s_col);
            render_text_center(
                &mut pixmap,
                &format!("{}", s_pct.round() as i32),
                cx,
                cy,
                44.0,
                0xffffff,
            );
        }
        "ring" => {
            ring(&mut pixmap, cx, cy, 24.0, 9.0, 0xd4d4d4);
            arc(&mut pixmap, cx, cy, 24.0, 9.0, w_col, w_pct);
        }
        "ring+number" => {
            ring(&mut pixmap, cx, cy, 30.0, 4.0, 0xd4d4d4);
            arc(&mut pixmap, cx, cy, 30.0, 4.0, w_col, w_pct);
            // Red session value: low-luminance red digits vanish on a dark taskbar.
            // Back them with a filled red disc and render the digits white.
            let num_color = if s_col == HEX_RED {
                filled_circle(&mut pixmap, cx, cy, 27.0, HEX_RED);
                0xffffff
            } else {
                s_col
            };
            render_text_center(
                &mut pixmap,
                &format!("{}", s_pct.round() as i32),
                cx,
                cy,
                46.0,
                num_color,
            );
        }
        "bar" => {
            let pad_x = 12.0;
            let pad_y = 6.0;
            let bar_w = SIZE as f32 - pad_x * 2.0;
            let bar_h = SIZE as f32 - pad_y * 2.0;
            let fill_h = (s_pct / 100.0 * bar_h).round();
            filled_rect(&mut pixmap, pad_x, pad_y, bar_w, bar_h, 0xe5e5e5);
            filled_rect(
                &mut pixmap,
                pad_x,
                pad_y + (bar_h - fill_h),
                bar_w,
                fill_h,
                s_col,
            );
            stroked_rect(&mut pixmap, pad_x, pad_y, bar_w, bar_h, 2.0, 0x333333);
        }
        _ => {}
    }
    pixmap.encode_png().unwrap_or_default()
}

pub fn payload_from_state(state: &crate::state::AppState) -> IconPayload {
    let mut sess_pct = state.session.used_pct;
    let week_pct = state.weekly.used_pct;
    if sess_pct.is_none() && week_pct.is_some() {
        sess_pct = week_pct;
    }
    let (session_total, weekly_total) = window_totals_for(&state.primary_provider);
    let now_secs = chrono::Utc::now().timestamp();
    let sess_resets = state.session.resets_at.as_deref().and_then(parse_iso_secs);
    let week_resets = state.weekly.resets_at.as_deref().and_then(parse_iso_secs);
    IconPayload {
        session_pct: sess_pct,
        session_color: color_for_window(
            sess_pct,
            sess_resets,
            now_secs,
            session_total,
            PARAMS_SESSION,
        ),
        weekly_pct: week_pct,
        weekly_color: color_for_window(
            week_pct,
            week_resets,
            now_secs,
            weekly_total,
            PARAMS_WEEKLY,
        ),
        error: state.error.is_some(),
    }
}

fn window_totals_for(provider: &str) -> (i64, i64) {
    match provider {
        // Claude five_hour + seven_day windows.
        "claude" => (SESSION_5H_SECS, WEEKLY_7D_SECS),
        // Codex API does not expose window length; assume same shape as Claude.
        "codex" => (SESSION_5H_SECS, WEEKLY_7D_SECS),
        // Copilot is monthly only; coloring stays on overage signal (no caller here).
        _ => (SESSION_5H_SECS, WEEKLY_7D_SECS),
    }
}

#[cfg(test)]
mod render_tests {
    use super::{font_bytes, render, IconPayload};
    use crate::color::{HEX_GREEN, HEX_RED};
    use tiny_skia::Pixmap;

    fn count_white_red(png: &[u8]) -> (usize, usize) {
        let pm = Pixmap::decode_png(png).expect("decode png");
        let (mut white, mut red) = (0usize, 0usize);
        for p in pm.pixels() {
            if p.alpha() != 255 {
                continue; // skip anti-aliased edges
            }
            let (r, g, b) = (p.red(), p.green(), p.blue());
            if r > 230 && g > 230 && b > 230 {
                white += 1;
            }
            if r > 170 && g < 80 && b < 80 {
                red += 1;
            }
        }
        (white, red)
    }

    fn payload(session_color: u32) -> IconPayload {
        IconPayload {
            session_pct: Some(88.0),
            session_color,
            weekly_pct: Some(70.0),
            weekly_color: HEX_GREEN,
            error: false,
        }
    }

    #[test]
    fn ring_number_red_session_renders_white_digits_on_red_disc() {
        if font_bytes().is_empty() {
            return; // Segoe UI absent on this host; skip the glyph assertion.
        }
        let (white, red) = count_white_red(&render("ring+number", &payload(HEX_RED)));
        assert!(red > 200, "expected a filled red disc, got {red} red px");
        assert!(white > 50, "expected white digits over the disc, got {white} white px");
    }

    #[test]
    fn ring_number_nonred_session_keeps_colored_digits() {
        // A green session value must not introduce a white-filled badge.
        let (white, _red) = count_white_red(&render("ring+number", &payload(HEX_GREEN)));
        assert_eq!(white, 0, "non-red path must not paint white digits, got {white}");
    }
}
