//! The egui side of the interface: the sidebar (peek options, match
//! settings, hand log) and the action bar under the table.

use std::sync::Arc;

use egui_macroquad::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, ScrollArea, Stroke,
    TextStyle, Vec2,
};

use crate::app::{App, LogKind, Tone, STACK_CHOICES};
use crate::game::{Action, Seat, MAX_BETS};

/// The panels are laid out for a window of this size (in logical pixels).
/// At any other size egui is zoomed by [`ui_scale`], so the sidebar and the
/// action bar always take the same share of the window and nothing can end
/// up outside it.
const DESIGN_SIZE: (f32, f32) = (1320.0, 840.0);

const SIDEBAR_W: f32 = 318.0;
const ACTION_BAR_H: f32 = 112.0;

/// How much to zoom the panels for a window of the given logical size.
pub fn ui_scale(window: (f32, f32)) -> f32 {
    let fit = (window.0 / DESIGN_SIZE.0).min(window.1 / DESIGN_SIZE.1);
    // Rounded down to steps of 0.02 so that dragging the window edge does
    // not re-rasterise the fonts on every single pixel of movement.
    ((fit * 50.0).floor() / 50.0).clamp(0.3, 3.0)
}

const PANEL: Color32 = Color32::from_rgb(19, 21, 27);
const PANEL_EDGE: Color32 = Color32::from_rgb(40, 43, 53);
const TEXT: Color32 = Color32::from_rgb(226, 229, 236);
const MUTED: Color32 = Color32::from_rgb(138, 144, 160);
const XRAY: Color32 = Color32::from_rgb(88, 214, 232);
const GOLD: Color32 = Color32::from_rgb(246, 196, 72);
const YOU: Color32 = Color32::from_rgb(120, 182, 255);
const BOT: Color32 = Color32::from_rgb(255, 150, 124);
const GOOD: Color32 = Color32::from_rgb(120, 220, 150);

/// What the player asked for this frame.
#[derive(Default)]
pub struct UiOut {
    pub action: Option<Action>,
    pub next_hand: bool,
    pub new_match: bool,
    /// Screen area left over for the table, in macroquad's logical pixels
    /// (x, y, w, h).
    pub table: (f32, f32, f32, f32),
}

pub fn install_style(ctx: &egui::Context) {
    // Inter for the panels too, so they match the table. egui's built-in
    // fonts stay in the list as fallbacks; they supply the suit symbols.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter-medium".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/Inter-Medium.otf"
        ))),
    );
    fonts.font_data.insert(
        "inter-bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/Inter-Bold.otf"
        ))),
    );
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut regular = vec!["inter-medium".to_owned()];
    regular.extend(fallbacks.iter().cloned());
    let mut bold = vec!["inter-bold".to_owned()];
    bold.extend(fallbacks);
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    visuals.selection.bg_fill = Color32::from_rgb(40, 108, 128);
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    ctx.set_visuals(visuals);
    // The zoom is driven by the window size (see `build`), not by Ctrl +/-.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);

    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.5));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(24.0));
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.icon_width = 18.0;
        style.interaction.selectable_labels = false;
    });
}

/// Builds both panels. `window` is the window size in logical pixels and
/// `dpi` the display's scale factor, both as reported by macroquad.
pub fn build(ctx: &egui::Context, app: &mut App, window: (f32, f32), dpi: f32) -> UiOut {
    // Keep the panels in proportion to the window. A change made here takes
    // effect on the next frame.
    let wanted = ui_scale(window) * dpi;
    if (ctx.pixels_per_point() - wanted).abs() > 0.001 {
        ctx.set_pixels_per_point(wanted);
    }

    let mut out = UiOut::default();
    sidebar(ctx, app, &mut out);
    action_bar(ctx, app, &mut out);

    // egui measures in zoomed points, macroquad in logical pixels. Convert
    // with the zoom this frame was actually laid out with.
    let k = ctx.pixels_per_point() / dpi;
    let free = ctx.available_rect();
    out.table = (
        free.left() * k,
        free.top() * k,
        free.width() * k,
        free.height() * k,
    );
    out
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

fn section(ui: &mut egui::Ui, title: &str, color: Color32) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title)
            .font(bold(11.5))
            .color(color)
            .extra_letter_spacing(1.2),
    );
    ui.add_space(1.0);
}

/// A peek checkbox with its keyboard shortcut shown at the right edge.
fn peek_row(ui: &mut egui::Ui, value: &mut bool, key: &str, text: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(value, text).on_hover_text(hint);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            Frame::new()
                .stroke(Stroke::new(1.0_f32, PANEL_EDGE))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(6, 1))
                .show(ui, |ui| ui.label(RichText::new(key).small().color(MUTED)));
        });
    });
}

fn sidebar(ctx: &egui::Context, app: &mut App, out: &mut UiOut) {
    let frame = Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(18, 14))
        .stroke(Stroke::new(1.0_f32, PANEL_EDGE));

    egui::SidePanel::right("sidebar")
        .exact_width(SIDEBAR_W)
        .resizable(false)
        .frame(frame)
        .show(ctx, |ui| {
            ui.label(
                RichText::new("X-Ray Hold'em")
                    .font(bold(24.0))
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new("Heads-up · fixed limit · you against the bot")
                    .small()
                    .color(MUTED),
            );

            // ---- score ----------------------------------------------------
            section(ui, "MATCH", MUTED);
            let g = &app.game;
            egui::Grid::new("score")
                .num_columns(3)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new("You").font(bold(14.0)).color(YOU));
                    ui.label(RichText::new("Bot").font(bold(14.0)).color(BOT));
                    ui.end_row();
                    ui.label(RichText::new("Points").color(MUTED));
                    ui.label(g.stacks[Seat::Human.idx()].to_string());
                    ui.label(g.stacks[Seat::Bot.idx()].to_string());
                    ui.end_row();
                    ui.label(RichText::new("Hands won").color(MUTED));
                    ui.label(g.hands_won[Seat::Human.idx()].to_string());
                    ui.label(g.hands_won[Seat::Bot.idx()].to_string());
                    ui.end_row();
                    ui.label(RichText::new("Matches won").color(MUTED));
                    ui.label(app.matches_won[Seat::Human.idx()].to_string());
                    ui.label(app.matches_won[Seat::Bot.idx()].to_string());
                    ui.end_row();
                });

            ui.add_space(2.0);
            ui.checkbox(&mut app.quick, "Quick play (shorter pauses)");

            // ---- peeks ----------------------------------------------------
            section(ui, "X-RAY OPTIONS", XRAY);
            let p = &mut app.peeks;
            peek_row(
                ui,
                &mut p.bottom1,
                "1",
                "Bottom card of the deck",
                "The last card of the shuffled deck. It never gets dealt.",
            );
            peek_row(
                ui,
                &mut p.bottom5,
                "2",
                "Five bottom cards of the deck",
                "The last five cards of the deck. None of them gets dealt.",
            );
            peek_row(
                ui,
                &mut p.opp_card,
                "3",
                "One of the bot's cards",
                "Turns one of the bot's two hole cards face up.",
            );
            peek_row(
                ui,
                &mut p.candidates,
                "4",
                "Four possible bot hands",
                "The bot's real hand mixed in with three decoys. The decoys are made from \
                 cards that are still in the deck and will not be dealt this hand.",
            );
            peek_row(
                ui,
                &mut p.burned,
                "5",
                "Burned cards",
                "Shows the cards burned before the flop, turn and river.",
            );
            peek_row(
                ui,
                &mut p.odds,
                "6",
                "Chance of winning",
                "Your chance of winning at showdown, worked out from exactly what you can \
                 see: your cards, the board, and whatever the other options are showing \
                 right now.",
            );
            ui.horizontal(|ui| {
                if ui.small_button("All on").clicked() {
                    p.set_all(true);
                }
                if ui.small_button("All off").clicked() {
                    p.set_all(false);
                }
                let note = if p.any() {
                    "only you see these"
                } else {
                    "playing it straight"
                };
                ui.label(RichText::new(note).small().color(MUTED));
            });

            // ---- new match ------------------------------------------------
            section(ui, "NEW MATCH", MUTED);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Starting points").color(MUTED));
                for n in STACK_CHOICES {
                    ui.selectable_value(&mut app.pending_stack, n, n.to_string());
                }
            });
            if ui
                .add(
                    egui::Button::new("Start new match")
                        .min_size(Vec2::new(ui.available_width(), 30.0)),
                )
                .clicked()
            {
                out.new_match = true;
            }

            // ---- rules ----------------------------------------------------
            ui.add_space(6.0);
            egui::CollapsingHeader::new(RichText::new("How the betting works").color(MUTED))
                .id_salt("rules")
                .show(ui, |ui| {
                    let rules = [
                        "No small blind. One player posts a 1-point blind, the other deals (D).",
                        "Every bet and raise is 1 point: calling costs 1, raising costs 2 \
                         (1 to call + 1 more). Opening the betting costs 1.",
                        &format!(
                            "At most {MAX_BETS} bets per round; the blind counts as the first."
                        ),
                        "The dealer acts first before the flop, the blind acts first after it.",
                        "The match ends when someone runs out of points.",
                        "Keys: F fold · C check/call · R bet/raise · Space next hand · 1–6 X-ray.",
                    ];
                    for rule in rules {
                        ui.label(RichText::new(format!("•  {rule}")).small().color(MUTED));
                    }
                });

            // ---- log ------------------------------------------------------
            section(ui, "HAND LOG", MUTED);
            Frame::new()
                .fill(Color32::from_rgb(13, 14, 19))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            for line in &app.log {
                                let text = RichText::new(&line.text).size(13.5);
                                let text = match line.kind {
                                    LogKind::Header => {
                                        ui.add_space(6.0);
                                        text.font(bold(13.5)).color(GOLD)
                                    }
                                    LogKind::Info => text.color(MUTED),
                                    LogKind::You => text.color(YOU),
                                    LogKind::Bot => text.color(BOT),
                                    LogKind::Board => text.color(TEXT),
                                    LogKind::Result => text.font(bold(13.5)).color(GOOD),
                                };
                                ui.label(text);
                            }
                        });
                });
        });
}

fn big_button(ui: &mut egui::Ui, label: &str, fill: Color32, enabled: bool, hint: &str) -> bool {
    let text = RichText::new(label).font(bold(16.5)).color(Color32::WHITE);
    let button = egui::Button::new(text)
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(40)))
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(BTN_W, 52.0));
    let response = ui.add_enabled(enabled, button);
    let response = if hint.is_empty() {
        response
    } else {
        response.on_disabled_hover_text(hint)
    };
    response.clicked()
}

const BTN_W: f32 = 184.0;
const BTN_GAP: f32 = 14.0;

fn action_bar(ctx: &egui::Context, app: &mut App, out: &mut UiOut) {
    let frame = Frame::new()
        .fill(Color32::from_rgb(15, 17, 22))
        .inner_margin(Margin::symmetric(16, 12))
        .stroke(Stroke::new(1.0_f32, PANEL_EDGE));

    egui::TopBottomPanel::bottom("actions")
        .exact_height(ACTION_BAR_H)
        .resizable(false)
        .frame(frame)
        .show(ctx, |ui| {
            let (status, tone) = app.status();
            let color = match tone {
                Tone::Neutral => MUTED,
                Tone::YourTurn => Color32::WHITE,
                Tone::Good => GOOD,
                Tone::Bad => BOT,
            };
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(status).font(bold(15.5)).color(color));
            });
            ui.add_space(4.0);

            let hand_over = app.game.state == crate::game::State::HandOver;
            let count = if hand_over { 1.0 } else { 3.0 };
            let total = count * BTN_W + (count - 1.0) * BTN_GAP;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = BTN_GAP;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));

                if hand_over {
                    if app.game.match_winner().is_some() {
                        if big_button(ui, "New match", Color32::from_rgb(160, 112, 24), true, "") {
                            out.new_match = true;
                        }
                    } else if big_button(
                        ui,
                        "Next hand   [Space]",
                        Color32::from_rgb(34, 122, 84),
                        true,
                        "",
                    ) {
                        out.next_hand = true;
                    }
                    return;
                }

                let mine = app.humans_turn();
                let legal = if mine {
                    app.game.legal()
                } else {
                    Default::default()
                };

                if big_button(
                    ui,
                    "Fold   [F]",
                    Color32::from_rgb(150, 48, 54),
                    mine && legal.can_fold,
                    if mine {
                        "Nothing to call, so there is no reason to fold."
                    } else {
                        ""
                    },
                ) {
                    out.action = Some(Action::Fold);
                }

                let call_label = if mine && legal.to_call == 0 {
                    "Check   [C]".to_string()
                } else {
                    "Call 1   [C]".to_string()
                };
                if big_button(ui, &call_label, Color32::from_rgb(44, 98, 168), mine, "") {
                    out.action = Some(Action::Call);
                }

                let raise_label = if mine && legal.to_call == 0 {
                    "Bet 1   [R]".to_string()
                } else {
                    "Raise 2   [R]".to_string()
                };
                let why_not = if !mine {
                    ""
                } else if app.game.bet_level >= MAX_BETS {
                    "Betting is capped for this round."
                } else {
                    "Not possible: one of you has no points left to put in."
                };
                if big_button(
                    ui,
                    &raise_label,
                    Color32::from_rgb(168, 118, 22),
                    mine && legal.can_raise,
                    why_not,
                ) {
                    out.action = Some(Action::Raise);
                }
            });
        });
}
