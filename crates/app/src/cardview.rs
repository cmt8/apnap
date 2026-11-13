//! Drawing cards and mana symbols.
//!
//! A real card is shown as its Scryfall image (whole card, never cropped or recoloured;
//! turned sideways when tapped). A card without an image — the invented test cards, tokens,
//! anything still downloading — is drawn from its definition as a plain "proxy" in the same
//! shape. Game state that the printed card can't show (current power/toughness, damage,
//! counters, combat) is drawn as small badges in the corners.

use crate::art::{Art, Seg, segments};
use card_model::{CardCatalog, CardDef, CardType, Color, describe};
use eframe::egui::emath::GuiRounding as _;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle,
    Vec2, pos2, vec2,
};
use std::sync::Arc;

/// Width / height of a Magic card.
pub const CARD_ASPECT: f32 = 488.0 / 680.0;

/// How far each further card of a pile peeks out, as a fraction of a card's width.
pub const PILE_STEP: f32 = 0.11;

/// The size at which a card about `height` points tall is drawn, and its width in screen
/// pixels. The size is a whole number of pixels (a multiple of 4 wide, so resizing a window
/// doesn't ask for a new texture at every step) and the texture is made for exactly that
/// many, so a card is never resampled on screen.
pub fn card_size(height: f32, pixels_per_point: f32) -> (Vec2, u32) {
    let want = (height * CARD_ASPECT * pixels_per_point).floor() as u32;
    let w = (want / 4 * 4).max(16);
    let h = w * 680 / 488;
    (
        vec2(w as f32 / pixels_per_point, h as f32 / pixels_per_point),
        w,
    )
}

// ---------------------------------------------------------------------------------------
// Text with mana symbols
// ---------------------------------------------------------------------------------------

enum Piece {
    Text(Arc<egui::Galley>),
    Sym(TextureHandle, f32),
}

/// A single line of text in which `{G}`-style symbols are drawn as Scryfall's symbols
/// (or left as text until the symbol has loaded).
pub struct Rich {
    pieces: Vec<Piece>,
    pub size: Vec2,
}

impl std::fmt::Debug for Rich {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rich({} pieces, {:?})", self.pieces.len(), self.size)
    }
}

impl Rich {
    pub fn new(ui: &egui::Ui, art: &mut Art, text: &str, font: FontId) -> Rich {
        let ppp = ui.pixels_per_point();
        let side_px = (font.size * 1.1 * ppp).round().max(4.0) as u32;
        let side = side_px as f32 / ppp;
        let mut pieces = Vec::new();
        let mut plain = String::new();
        let flush = |plain: &mut String, pieces: &mut Vec<Piece>| {
            if !plain.is_empty() {
                let g = ui.painter().layout_no_wrap(
                    std::mem::take(plain),
                    font.clone(),
                    Color32::PLACEHOLDER,
                );
                pieces.push(Piece::Text(g));
            }
        };
        for seg in segments(text) {
            match seg {
                Seg::Text(t) => plain.push_str(t),
                Seg::Symbol(s) => match art.symbol(ui.ctx(), s, side_px) {
                    Some(tex) => {
                        flush(&mut plain, &mut pieces);
                        pieces.push(Piece::Sym(tex, side));
                    }
                    None => plain.push_str(s),
                },
            }
        }
        flush(&mut plain, &mut pieces);
        let mut size = vec2(0.0, font.size);
        for p in &pieces {
            let s = match p {
                Piece::Text(g) => g.size(),
                Piece::Sym(_, side) => vec2(*side + 1.0, *side),
            };
            size.x += s.x;
            size.y = size.y.max(s.y);
        }
        Rich { pieces, size }
    }

    /// Paint with the left-top corner at `pos`.
    pub fn paint(&self, painter: &egui::Painter, pos: Pos2, color: Color32) {
        let mut x = pos.x;
        for p in &self.pieces {
            match p {
                Piece::Text(g) => {
                    let y = pos.y + (self.size.y - g.size().y) / 2.0;
                    painter.galley(pos2(x, y), g.clone(), color);
                    x += g.size().x;
                }
                Piece::Sym(tex, side) => {
                    let y = pos.y + (self.size.y - side) / 2.0;
                    let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(*side));
                    painter.image(tex.id(), rect, full_uv(), Color32::WHITE);
                    x += side + 1.0;
                }
            }
        }
    }
}

fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// A label that draws mana symbols.
pub fn rich_label(
    ui: &mut egui::Ui,
    art: &mut Art,
    text: &str,
    size: f32,
    color: Color32,
) -> Response {
    let rich = Rich::new(ui, art, text, FontId::proportional(size));
    let (rect, resp) = ui.allocate_exact_size(rich.size, Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    if ui.is_rect_visible(rect) {
        rich.paint(ui.painter(), rect.min, color);
    }
    resp
}

/// A button whose label may contain mana symbols. Its accessible name is the plain text.
pub fn rich_button(ui: &mut egui::Ui, art: &mut Art, label: &str, selected: bool) -> Response {
    if !art.enabled() || !label.contains('{') {
        return ui.add(egui::Button::new(label).selected(selected));
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let rich = Rich::new(ui, art, label, font);
    let pad = ui.spacing().button_padding;
    let mut size = rich.size + 2.0 * pad;
    size.y = size.y.max(ui.spacing().interact_size.y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    if ui.is_rect_visible(rect) {
        let vis = ui.style().interact_selectable(&resp, selected);
        ui.painter().rect(
            rect.expand(vis.expansion),
            vis.corner_radius,
            vis.weak_bg_fill,
            vis.bg_stroke,
            StrokeKind::Inside,
        );
        let pos = pos2(rect.min.x + pad.x, rect.center().y - rich.size.y / 2.0);
        rich.paint(ui.painter(), pos, vis.text_color());
    }
    resp
}

// ---------------------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Over the art, just below the title bar, on the left / right: state that must not
    /// hide the card's name (counters, what it attacks or blocks).
    ArtLeft,
    ArtRight,
}

/// The part of a card's height its title bar takes; badges in the art corners start below.
const TITLE_BAR: f32 = 0.13;

/// A small label in a corner of a card: state the printed card can't show.
#[derive(Clone, Debug)]
pub struct Badge {
    pub corner: Corner,
    pub text: String,
    pub bg: Color32,
    pub fg: Color32,
}

impl Badge {
    pub fn new(corner: Corner, text: impl Into<String>, bg: Color32, fg: Color32) -> Badge {
        Badge {
            corner,
            text: text.into(),
            bg,
            fg,
        }
    }
}

/// How one card is to be drawn.
#[derive(Debug)]
pub struct CardLook<'a> {
    pub def: &'a CardDef,
    /// The accessible name of the widget (what a screen reader, or a test, sees).
    pub label: &'a str,
    /// Height of the upright card.
    pub height: f32,
    pub tapped: bool,
    /// Identical cards shown as one pile (1 = a single card).
    pub pile: u32,
    pub outline: Option<Stroke>,
    pub badges: &'a [Badge],
    /// Painted this far from where it is laid out (an attacker stepping forward).
    pub offset: Vec2,
    /// Animate turning and stepping under this id (`None`: changes are instant).
    pub motion: Option<egui::Id>,
    /// Can be dragged onto something (a player to attack, a creature to block).
    pub draggable: bool,
    /// Where the tokens its rules text creates are defined (their descriptions).
    pub tokens: Option<&'a CardCatalog>,
}

/// How long a card takes to turn or step forward, in seconds.
const MOTION_TIME: f32 = 0.14;

fn frame_color(def: &CardDef) -> Color32 {
    if def.card_types.contains(&CardType::Land) {
        return Color32::from_rgb(132, 108, 84);
    }
    match def.printed_colors().as_slice() {
        [] => Color32::from_rgb(140, 144, 152),
        [Color::White] => Color32::from_rgb(222, 214, 184),
        [Color::Blue] => Color32::from_rgb(66, 118, 176),
        [Color::Black] => Color32::from_rgb(74, 68, 76),
        [Color::Red] => Color32::from_rgb(180, 78, 60),
        [Color::Green] => Color32::from_rgb(70, 128, 84),
        _ => Color32::from_rgb(196, 166, 84),
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// A card's rules text, with the tokens it creates described from `tokens`.
pub fn describe_rules(def: &CardDef, tokens: Option<&CardCatalog>) -> Vec<String> {
    match tokens {
        Some(cat) => describe::rules_text_in(def, &|key| cat.find(key).map(|id| cat.get(id))),
        None => describe::rules_text(def),
    }
}

/// Draw a card from its definition (no image): name, cost, type line, rules text where it
/// fits, power/toughness. `rect` may be upright or sideways.
pub fn paint_proxy(
    ui: &egui::Ui,
    art: &mut Art,
    rect: Rect,
    def: &CardDef,
    tokens: Option<&CardCatalog>,
) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let sideways = rect.width() > rect.height();
    // One unit = 1% of the card's width.
    let s = if sideways {
        rect.height() / 100.0
    } else {
        rect.width() / 100.0
    };
    let frame = frame_color(def);
    let ink = Color32::from_rgb(20, 20, 24);
    let paper = mix(frame, Color32::WHITE, 0.72);
    painter.rect_filled(rect, 4.8 * s, ink);
    let inner = rect.shrink(3.5 * s);
    painter.rect_filled(inner, 2.0 * s, frame);

    // Title bar: name on the left, mana cost on the right.
    let title = Rect::from_min_size(
        inner.min + vec2(2.0 * s, 2.0 * s),
        vec2(inner.width() - 4.0 * s, 12.5 * s),
    );
    painter.rect_filled(title, 2.0 * s, paper);
    let font = FontId::proportional((7.6 * s).max(7.0));
    let cost = def
        .mana_cost
        .as_ref()
        .map_or(String::new(), |m| m.to_text());
    let cost = Rich::new(ui, art, &cost, FontId::proportional((7.4 * s).max(7.0)));
    let cost_pos = pos2(
        title.max.x - cost.size.x - 1.5 * s,
        title.center().y - cost.size.y / 2.0,
    );
    cost.paint(&painter, cost_pos, ink);
    let name_clip = Rect::from_min_max(
        title.min,
        pos2((cost_pos.x - s).max(title.min.x), title.max.y),
    );
    painter.with_clip_rect(name_clip).text(
        pos2(title.min.x + 2.0 * s, title.center().y),
        Align2::LEFT_CENTER,
        &def.name,
        font,
        ink,
    );

    let type_line = describe::type_line(def);
    let small = FontId::proportional((6.4 * s).max(7.0));
    let pt = match (def.power, def.toughness) {
        (Some(p), Some(t)) => Some(format!("{p}/{t}")),
        _ => None,
    };
    let type_bar_h = 11.0 * s;
    let (type_bar, text_box) = if sideways {
        let bar = Rect::from_min_size(
            pos2(title.min.x, title.max.y + 2.0 * s),
            vec2(title.width(), type_bar_h),
        );
        (bar, None)
    } else {
        // "Art" box, type bar, text box.
        let art_box = Rect::from_min_max(
            pos2(title.min.x + s, title.max.y + 1.5 * s),
            pos2(title.max.x - s, inner.min.y + 74.0 * s),
        );
        painter.rect_filled(art_box, s, mix(frame, Color32::BLACK, 0.45));
        let initial: String = def
            .name
            .trim_start_matches("Test ")
            .chars()
            .take(1)
            .collect();
        painter.text(
            art_box.center(),
            Align2::CENTER_CENTER,
            initial,
            FontId::proportional(34.0 * s),
            mix(frame, Color32::WHITE, 0.25),
        );
        let bar = Rect::from_min_size(
            pos2(title.min.x, art_box.max.y + 1.5 * s),
            vec2(title.width(), type_bar_h),
        );
        let text = Rect::from_min_max(
            pos2(title.min.x + s, bar.max.y + 1.5 * s),
            pos2(title.max.x - s, inner.max.y - 2.0 * s),
        );
        (bar, Some(text))
    };
    painter.rect_filled(type_bar, 2.0 * s, paper);
    painter.with_clip_rect(type_bar).text(
        pos2(type_bar.min.x + 2.0 * s, type_bar.center().y),
        Align2::LEFT_CENTER,
        type_line,
        small.clone(),
        ink,
    );
    if let Some(text_box) = text_box {
        painter.rect_filled(text_box, s, paper);
        // Rules text only where it would be legible.
        let rules_font = FontId::proportional(6.3 * s);
        if rules_font.size >= 9.0 {
            // One line for each bullet of a modal ability.
            let text = describe_rules(def, tokens)
                .join("\n")
                .replace(" • ", "\n• ")
                .replace("— •", "—\n•");
            let galley = painter.layout(text, rules_font, ink, text_box.width() - 4.0 * s);
            painter.with_clip_rect(text_box).galley(
                text_box.min + vec2(2.0 * s, 1.5 * s),
                galley,
                ink,
            );
        }
    }
    if let Some(pt) = pt {
        let size = vec2(24.0 * s, 12.0 * s);
        let r = Rect::from_min_size(inner.max - size - vec2(1.0 * s, 1.0 * s), size);
        painter.rect_filled(r, 3.0 * s, paper);
        painter.rect_stroke(r, 3.0 * s, Stroke::new(s.max(1.0), ink), StrokeKind::Inside);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            pt,
            FontId::proportional((8.5 * s).max(8.0)),
            ink,
        );
    }
}

/// Paint the card image into `rect`, turned a quarter clockwise if `sideways`.
fn paint_image(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, sideways: bool) {
    paint_image_turned(painter, tex, rect, if sideways { 1.0 } else { 0.0 });
}

/// Paint the card image centred in `rect`, turned `turn` quarters clockwise (0 = upright,
/// 1 = tapped; in between while it is turning). `rect` has the shape of the card at rest.
fn paint_image_turned(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, turn: f32) {
    // On whole pixels, so a texture made for this size maps one to one.
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    if turn <= 0.0 {
        painter.image(tex.id(), rect, full_uv(), Color32::WHITE);
        return;
    }
    let corners = if turn >= 1.0 {
        // The card's top edge ends up on the right.
        [
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ]
    } else {
        // Mid-turn: the upright card rotated about the centre.
        let long = rect.width().max(rect.height());
        let half = vec2(long * CARD_ASPECT / 2.0, long / 2.0);
        let (sin, cos) = (turn * std::f32::consts::FRAC_PI_2).sin_cos();
        let c = rect.center();
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| {
            let (x, y) = (sx * half.x, sy * half.y);
            pos2(c.x + x * cos - y * sin, c.y + x * sin + y * cos)
        })
    };
    // Corners are the card's own top-left, top-right, bottom-right, bottom-left.
    let uvs = [
        pos2(0.0, 0.0),
        pos2(1.0, 0.0),
        pos2(1.0, 1.0),
        pos2(0.0, 1.0),
    ];
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (pos, uv) in corners.into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

/// An arrow from `from` to `to` (an attack, a block, something being dragged).
pub fn paint_arrow(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32, width: f32) {
    let d = to - from;
    let len = d.length();
    if len < 4.0 {
        return;
    }
    let dir = d / len;
    let side = vec2(-dir.y, dir.x);
    let head = (width * 3.2).min(len * 0.5);
    let neck = to - dir * head;
    let shadow = Color32::from_black_alpha(170);
    for (w, c, grow) in [(width + 3.0, shadow, 1.5), (width, color, 0.0)] {
        painter.line_segment([from, neck], Stroke::new(w, c));
        let wing = head * 0.6 + grow;
        painter.add(egui::Shape::convex_polygon(
            vec![to + dir * grow, neck + side * wing, neck - side * wing],
            c,
            Stroke::NONE,
        ));
    }
}

/// The strip along a card's bottom edge with the artist and copyright line, as a fraction
/// of the card's height. Badges stay clear of it (Scryfall's image guidelines).
const CREDIT_STRIP: f32 = 0.075;

/// Paint `badges` in the corners of `rect`, a card that is upright or `sideways` (turned a
/// quarter clockwise, so its bottom edge is on the left).
fn paint_badges(
    painter: &egui::Painter,
    rect: Rect,
    sideways: bool,
    badges: &[Badge],
    font_size: f32,
) {
    let font = FontId::proportional(font_size);
    let pad = vec2(3.0, 1.0);
    let inset = (rect.width().min(rect.height()) * 0.045).max(2.0);
    let (clear_left, clear_bottom) = if sideways {
        (rect.width() * CREDIT_STRIP, 0.0)
    } else {
        (0.0, rect.height() * CREDIT_STRIP)
    };
    // Where the title bar is: along the top of an upright card, along the right edge of a
    // card turned a quarter clockwise.
    let (title_x, title_y) = if sideways {
        (rect.width() * TITLE_BAR, 0.0)
    } else {
        (0.0, rect.height() * TITLE_BAR)
    };
    for corner in [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
        Corner::ArtLeft,
        Corner::ArtRight,
    ] {
        let mut offset = 0.0;
        for b in badges.iter().filter(|b| b.corner == corner) {
            // A badge never gets wider than the card: a long one is shortened, visibly.
            let room = (rect.width() - 2.0 * inset - title_x - 2.0 * pad.x).max(8.0);
            let mut text = b.text.clone();
            let mut galley = painter.layout_no_wrap(text.clone(), font.clone(), b.fg);
            while galley.size().x > room && text.chars().count() > 2 {
                text = text.trim_end_matches('…').to_string();
                text.pop();
                text.push('…');
                galley = painter.layout_no_wrap(text.clone(), font.clone(), b.fg);
            }
            let size = galley.size() + 2.0 * pad;
            let x = match corner {
                Corner::TopLeft | Corner::BottomLeft | Corner::ArtLeft => {
                    rect.min.x + inset + clear_left
                }
                Corner::TopRight | Corner::BottomRight => rect.max.x - inset - size.x,
                Corner::ArtRight => rect.max.x - inset - title_x - size.x,
            };
            let y = match corner {
                Corner::TopLeft | Corner::TopRight => rect.min.y + inset + offset,
                Corner::ArtLeft | Corner::ArtRight => rect.min.y + inset + title_y + offset,
                Corner::BottomLeft | Corner::BottomRight => {
                    rect.max.y - inset - clear_bottom - size.y - offset
                }
            };
            let r = Rect::from_min_size(pos2(x, y), size);
            painter.rect_filled(r, 4.0, b.bg);
            painter.rect_stroke(
                r,
                4.0,
                Stroke::new(1.0, Color32::from_black_alpha(160)),
                StrokeKind::Outside,
            );
            painter.galley(r.min + pad, galley, b.fg);
            offset += size.y + 2.0;
        }
    }
}

/// Draw a card as a clickable widget.
pub fn card_widget(ui: &mut egui::Ui, art: &mut Art, look: &CardLook<'_>) -> Response {
    let (upright, width_px) = card_size(look.height, ui.pixels_per_point());
    let one = if look.tapped {
        vec2(upright.y, upright.x)
    } else {
        upright
    };
    let extra = look.pile.saturating_sub(1).min(4);
    let step = (upright.x * PILE_STEP).round();
    let size = one + vec2(extra as f32 * step, 0.0);
    let sense = if look.draggable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, look.label));
    // Turning and stepping forward are animated; the layout itself changes at once.
    let (turn, offset) = match look.motion {
        Some(id) => {
            let ctx = ui.ctx();
            (
                ctx.animate_bool_with_time(id.with("turn"), look.tapped, MOTION_TIME),
                vec2(
                    ctx.animate_value_with_time(id.with("x"), look.offset.x, MOTION_TIME),
                    ctx.animate_value_with_time(id.with("y"), look.offset.y, MOTION_TIME),
                ),
            )
        }
        None => (if look.tapped { 1.0 } else { 0.0 }, look.offset),
    };
    if !ui.is_rect_visible(rect.expand(look.height)) {
        return resp;
    }
    let tex = if look.def.token {
        art.token(ui.ctx(), look.def, width_px)
    } else {
        art.card(ui.ctx(), &look.def.name, width_px)
    };
    let front = Rect::from_min_size(rect.min, one).translate(offset);
    // The rest of a pile peeks out to the right, underneath.
    for i in (0..=extra).rev() {
        let r = front.translate(vec2(i as f32 * step, 0.0));
        match &tex {
            Some(tex) => paint_image_turned(ui.painter(), tex, r, turn),
            None => paint_proxy(ui, art, r, look.def, look.tokens),
        }
    }
    let radius = upright.x * 0.048;
    let outline = look.outline.or_else(|| {
        resp.hovered()
            .then_some(Stroke::new(1.5, Color32::from_gray(235)))
    });
    if let Some(stroke) = outline {
        ui.painter()
            .rect_stroke(front, radius, stroke, StrokeKind::Outside);
    }
    let font_size = (look.height * 0.1).clamp(9.5, 14.0);
    paint_badges(ui.painter(), front, look.tapped, look.badges, font_size);
    if look.pile > 1 {
        let b = [Badge::new(
            Corner::BottomRight,
            format!("x{}", look.pile),
            Color32::from_rgb(30, 30, 36),
            Color32::WHITE,
        )];
        paint_badges(
            ui.painter(),
            Rect::from_min_max(pos2(rect.max.x - one.x, rect.min.y), rect.max).translate(offset),
            look.tapped,
            &b,
            font_size,
        );
    }
    resp
}

/// The enlarged view of a card, about `height` points tall: its image, or a proxy with the
/// full rules text.
pub fn card_large(
    ui: &mut egui::Ui,
    art: &mut Art,
    def: &CardDef,
    height: f32,
    tokens: Option<&CardCatalog>,
) -> Response {
    let (size, width_px) = card_size(height, ui.pixels_per_point());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Image, true, format!("{} (enlarged)", def.name))
    });
    if ui.is_rect_visible(rect) {
        let tex = if def.token {
            art.token(ui.ctx(), def, width_px)
        } else {
            art.card(ui.ctx(), &def.name, width_px)
        };
        match tex {
            Some(tex) => paint_image(ui.painter(), &tex, rect, false),
            None => paint_proxy(ui, art, rect, def, tokens),
        }
    }
    resp
}

// ---------------------------------------------------------------------------------------
// Player plate and zone counters
// ---------------------------------------------------------------------------------------

/// The small pictures next to a player's zone counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// A fan of cards.
    Hand,
    /// A pile of cards.
    Library,
    /// A headstone.
    Graveyard,
    /// A struck-through circle: removed from the game.
    Exile,
}

/// Draw `icon` in the square `rect`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.min.x + x * w, rect.min.y + y * h);
    let dark = Color32::from_rgb(34, 34, 42);
    let line = Stroke::new((w * 0.075).max(1.2), color);
    match icon {
        Icon::Hand => {
            // Three cards fanned out from a point below them.
            let pivot = at(0.5, 1.0);
            let (cw, ch) = (w * 0.4, h * 0.78);
            for angle in [-0.5_f32, 0.0, 0.5] {
                let (sin, cos) = angle.sin_cos();
                let corners = [(-0.5, -1.0), (0.5, -1.0), (0.5, -0.12), (-0.5, -0.12)].map(
                    |(x, y): (f32, f32)| {
                        let (x, y) = (x * cw, y * ch);
                        pos2(pivot.x + x * cos - y * sin, pivot.y + x * sin + y * cos)
                    },
                );
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), dark, line));
            }
        }
        Icon::Library => {
            // A pile: three cards, each a little further down and right.
            for i in (0..3).rev() {
                let d = i as f32 * 0.11;
                let r = Rect::from_min_max(at(0.12 + d, 0.04 + d), at(0.66 + d, 0.74 + d));
                painter.rect(r, w * 0.08, dark, line, StrokeKind::Middle);
            }
        }
        Icon::Graveyard => {
            // A headstone on the ground, with a cross.
            let top = at(0.5, 0.36);
            painter.circle_filled(top, w * 0.27, color);
            painter.rect_filled(
                Rect::from_min_max(at(0.23, 0.36), at(0.77, 0.9)),
                0.0,
                color,
            );
            painter.line_segment([at(0.08, 0.92), at(0.92, 0.92)], line);
            let cross = Stroke::new((w * 0.085).max(1.2), dark);
            painter.line_segment([at(0.5, 0.24), at(0.5, 0.7)], cross);
            painter.line_segment([at(0.36, 0.42), at(0.64, 0.42)], cross);
        }
        Icon::Exile => {
            let r = w * 0.38;
            painter.circle_stroke(rect.center(), r, line);
            let d = vec2(r, r) * std::f32::consts::FRAC_1_SQRT_2;
            painter.line_segment([rect.center() - d, rect.center() + d], line);
        }
    }
}

/// An icon with a number: how many cards a player has in a zone. `label` is its accessible
/// name. A clickable one (a zone that can be looked through) reacts to the pointer;
/// `outline` marks one that holds something that can be chosen.
pub fn zone_count(
    ui: &mut egui::Ui,
    icon: Icon,
    count: usize,
    label: &str,
    clickable: bool,
    outline: Option<Stroke>,
) -> Response {
    let font = FontId::proportional(15.0);
    let text = ui
        .painter()
        .layout_no_wrap(count.to_string(), font, Color32::PLACEHOLDER);
    let icon_side = 20.0;
    let pad = vec2(5.0, 3.0);
    let size = vec2(icon_side + 4.0 + text.size().x, icon_side) + 2.0 * pad;
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let role = if clickable {
        egui::Role::Button
    } else {
        egui::Role::Label
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(role, true, label));
    if ui.is_rect_visible(rect) {
        let hot = clickable && resp.hovered();
        let color = if count == 0 {
            Color32::from_gray(120)
        } else if hot {
            Color32::WHITE
        } else {
            Color32::from_gray(205)
        };
        if hot {
            ui.painter()
                .rect_filled(rect, 6.0, Color32::from_rgb(54, 54, 66));
        }
        if let Some(stroke) = outline {
            ui.painter()
                .rect_stroke(rect, 6.0, stroke, StrokeKind::Inside);
        }
        let icon_rect = Rect::from_min_size(rect.min + pad, Vec2::splat(icon_side));
        paint_icon(ui.painter(), icon_rect, icon, color);
        let pos = pos2(icon_rect.max.x + 4.0, rect.center().y - text.size().y / 2.0);
        ui.painter().galley(pos, text, color);
    }
    if clickable {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// How a player's plate is drawn.
#[derive(Debug)]
pub struct Plate<'a> {
    /// The accessible name ("P1 — 20 life").
    pub label: &'a str,
    pub name: &'a str,
    pub life: i32,
    /// It is this player's turn.
    pub active: bool,
    /// They have left the game.
    pub out: bool,
    /// Can be clicked (a target, a player to attack).
    pub clickable: bool,
    pub outline: Option<Stroke>,
}

/// A player's name with their life total, large.
pub fn player_plate(ui: &mut egui::Ui, plate: &Plate<'_>) -> Response {
    let gold = Color32::from_rgb(235, 190, 60);
    let name_color = if plate.active {
        gold
    } else {
        Color32::from_gray(225)
    };
    let name = ui.painter().layout_no_wrap(
        plate.name.to_string(),
        FontId::proportional(15.0),
        name_color,
    );
    let life_text = if plate.out {
        "OUT".to_string()
    } else {
        plate.life.to_string()
    };
    let life = ui.painter().layout_no_wrap(
        life_text,
        FontId::proportional(if plate.out { 15.0 } else { 26.0 }),
        Color32::WHITE,
    );
    let height = 36.0;
    let life_box = vec2((life.size().x + 18.0).max(46.0), height - 6.0);
    let size = vec2(10.0 + name.size().x + 10.0 + life_box.x + 3.0, height);
    let sense = if plate.clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let role = if plate.clickable {
        egui::Role::Button
    } else {
        egui::Role::Label
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(role, true, plate.label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if plate.clickable && resp.hovered() {
            Color32::from_rgb(58, 58, 70)
        } else {
            Color32::from_rgb(40, 40, 50)
        };
        p.rect_filled(rect, 9.0, fill);
        let stroke = plate.outline.unwrap_or(Stroke::new(
            1.0,
            if plate.active {
                gold
            } else {
                Color32::from_gray(80)
            },
        ));
        p.rect_stroke(rect, 9.0, stroke, StrokeKind::Inside);
        p.galley(
            pos2(rect.min.x + 10.0, rect.center().y - name.size().y / 2.0),
            name,
            name_color,
        );
        // The life total: the thing to see at a glance.
        let life_rect = Rect::from_min_size(
            pos2(rect.max.x - life_box.x - 3.0, rect.min.y + 3.0),
            life_box,
        );
        let life_fill = if plate.out {
            Color32::from_gray(70)
        } else if plate.life <= 5 {
            Color32::from_rgb(168, 40, 40)
        } else {
            Color32::from_rgb(24, 24, 30)
        };
        p.rect_filled(life_rect, 7.0, life_fill);
        p.galley(life_rect.center() - life.size() / 2.0, life, Color32::WHITE);
    }
    if plate.clickable {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}
