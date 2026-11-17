//! Draws the poker table with macroquad shapes.
//!
//! Everything is laid out in a virtual 1000 x 720 space that is scaled
//! uniformly to fit whatever room the egui panels leave free.

use std::collections::HashMap;
use std::f64::consts::PI;

use macroquad::prelude::*;

use crate::app::App;
use crate::cards::Card;
use crate::eval::{describe_holding, describe_versus};
use crate::game::{Seat, State};
use crate::gfx::{self, alpha, pal, Align, CardLook, Gfx};
use crate::odds::Odds;

const VW: f32 = 1000.0;
const VH: f32 = 720.0;

/// Card sizes in virtual units.
const BIG: (f32, f32) = (84.0, 118.0);
const MID: (f32, f32) = (60.0, 84.0);
const MINI: (f32, f32) = (46.0, 64.0);

const DECK_POS: (f32, f32) = (165.0, 370.0);
const BOARD_Y: f32 = 370.0;
const BOARD_STEP: f32 = 94.0;

const TRAVEL: f64 = 0.32;
const FLIP: f64 = 0.26;
const STAGGER: f64 = 0.11;

/// Identifies a card position that animates (slides out of the deck, flips).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    Hole(Seat, u8),
    Board(u8),
    Burn(u8),
}

pub struct SlotAnim {
    spawn: f64,
    face: bool,
    flip_at: f64,
}

struct Pose {
    x: f32,
    y: f32,
    sx: f32,
    face: bool,
    settled: bool,
}

struct View {
    ox: f32,
    oy: f32,
    s: f32,
}

impl View {
    fn x(&self, vx: f32) -> f32 {
        self.ox + vx * self.s
    }
    fn y(&self, vy: f32) -> f32 {
        self.oy + vy * self.s
    }
    fn u(&self, v: f32) -> f32 {
        v * self.s
    }
}

/// Makes sure a slot has an animation record. Slots created in the same
/// frame are staggered in the order they are registered, which is how the
/// one-card-at-a-time dealing rhythm comes about.
fn ensure(anims: &mut HashMap<Slot, SlotAnim>, slot: Slot, now: f64, queue: &mut u32) {
    anims.entry(slot).or_insert_with(|| {
        let spawn = now + *queue as f64 * STAGGER;
        *queue += 1;
        SlotAnim {
            spawn,
            face: false,
            flip_at: f64::NEG_INFINITY,
        }
    });
}

/// Where a card is right now and which side is showing. Cards always leave
/// the deck face down and turn over once they have landed.
fn pose(
    anims: &mut HashMap<Slot, SlotAnim>,
    slot: Slot,
    want_face: bool,
    to: (f32, f32),
    now: f64,
    queue: &mut u32,
) -> Option<Pose> {
    ensure(anims, slot, now, queue);
    let a = anims.get_mut(&slot)?;
    if a.face != want_face {
        a.face = want_face;
        a.flip_at = now.max(a.spawn + TRAVEL);
    }
    if now < a.spawn {
        return None;
    }
    let t = ((now - a.spawn) / TRAVEL).min(1.0) as f32;
    let ease = 1.0 - (1.0 - t).powi(3);
    let f = (now - a.flip_at) / FLIP;
    let (sx, face) = if f < 0.0 {
        (1.0, !a.face)
    } else if f < 1.0 {
        (
            (f * PI).cos().abs() as f32,
            if f < 0.5 { !a.face } else { a.face },
        )
    } else {
        (1.0, a.face)
    };
    Some(Pose {
        x: DECK_POS.0 + (to.0 - DECK_POS.0) * ease,
        y: DECK_POS.1 + (to.1 - DECK_POS.1) * ease,
        sx,
        face,
        settled: t >= 1.0 && f >= 1.0,
    })
}

fn draw_card(
    g: &Gfx,
    v: &View,
    card: Card,
    face: bool,
    at: (f32, f32),
    size: (f32, f32),
    look: CardLook,
) {
    let (cx, cy, w, h) = (v.x(at.0), v.y(at.1), v.u(size.0), v.u(size.1));
    if face {
        gfx::card_face(g, card, cx, cy, w, h, look);
    } else {
        gfx::card_back(cx, cy, w, h, look);
    }
}

fn label(g: &Gfx, v: &View, text: &str, x: f32, y: f32, size: f32, color: Color) {
    g.text(text, v.x(x), v.y(y), v.u(size), color, Align::Center, false);
}

pub fn draw(app: &mut App, area: Rect, now: f64) {
    let s = (area.w / VW).min(area.h / VH).max(0.05);
    let v = View {
        ox: area.x + (area.w - VW * s) / 2.0,
        oy: area.y + (area.h - VH * s) / 2.0,
        s,
    };

    // Cards the showdown winner is playing: these glow, the rest fade.
    let showdown = app
        .game
        .result
        .as_ref()
        .filter(|r| !r.by_fold && app.game.state == State::HandOver);
    let winning: Option<Vec<Card>> = showdown.and_then(|r| {
        let fives = r.best_five.as_ref()?;
        Some(match r.winner {
            Some(seat) => fives[seat.idx()].to_vec(),
            None => fives.iter().flatten().copied().collect(),
        })
    });
    let reveal_bot = showdown.is_some();

    // Register this frame's new cards in true dealing order.
    let mut queue = 0u32;
    {
        let g = &app.game;
        let blind = g.blind_seat();
        for round in 0..2u8 {
            for seat in [blind, g.dealer] {
                ensure(&mut app.anims, Slot::Hole(seat, round), now, &mut queue);
            }
        }
        // Burn, flop (3 cards), burn, turn, burn, river.
        let board_after_burn = [3usize, 4, 5];
        let mut b = 0;
        for (i, upto) in board_after_burn
            .into_iter()
            .enumerate()
            .take(g.burned.len())
        {
            ensure(&mut app.anims, Slot::Burn(i as u8), now, &mut queue);
            while b < upto.min(g.board.len()) {
                ensure(&mut app.anims, Slot::Board(b as u8), now, &mut queue);
                b += 1;
            }
        }
    }

    draw_felt(&v);
    draw_deck(app, &v);
    draw_burns(app, &v, now, &mut queue);
    draw_board(app, &v, now, &mut queue, winning.as_deref());
    if app.peeks.candidates {
        draw_candidates(app, &v, reveal_bot);
    }
    for seat in [Seat::Bot, Seat::Human] {
        draw_seat(
            app,
            &v,
            seat,
            now,
            &mut queue,
            winning.as_deref(),
            reveal_bot,
        );
    }
    draw_pot(app, &v);
    if app.peeks.odds {
        draw_odds(app, &v, now);
    }
}

fn draw_felt(v: &View) {
    let (x, y, w, h, r) = (40.0, 130.0, 920.0, 520.0, 150.0);
    let rr = |grow: f32, dy: f32, color: Color| {
        gfx::rrect(
            v.x(x - grow),
            v.y(y - grow + dy),
            v.u(w + 2.0 * grow),
            v.u(h + 2.0 * grow),
            v.u(r + grow),
            color,
        );
    };
    // Soft shadow under the table, then the wooden rail.
    rr(34.0, 14.0, Color::new(0.0, 0.0, 0.0, 0.22));
    rr(28.0, 10.0, Color::new(0.0, 0.0, 0.0, 0.30));
    rr(24.0, 0.0, pal::RIM_DARK);
    rr(21.0, 0.0, pal::RIM);
    rr(12.0, 0.0, gfx::shade(pal::RIM, 1.18));
    rr(9.0, 0.0, pal::RIM);
    rr(3.0, 0.0, pal::RIM_DARK);
    // Felt, a touch lighter towards the middle.
    rr(0.0, 0.0, gfx::shade(pal::FELT, 0.86));
    for i in 1..=7 {
        let f = 0.86 + 0.14 * i as f32 / 7.0;
        rr(-(i as f32) * 20.0, 0.0, gfx::shade(pal::FELT, f));
    }
    gfx::rrect_stroke(
        v.x(x + 22.0),
        v.y(y + 22.0),
        v.u(w - 44.0),
        v.u(h - 44.0),
        v.u(r - 22.0),
        v.u(1.6).max(1.0),
        Color::new(1.0, 1.0, 1.0, 0.09),
    );
}

fn draw_deck(app: &App, v: &View) {
    let g = &app.gfx;
    let game = &app.game;

    // A few offset backs read as a stack.
    for i in (0..4).rev() {
        let off = i as f32 * 1.6;
        let at = (DECK_POS.0 - off, DECK_POS.1 + off);
        gfx::card_back(
            v.x(at.0),
            v.y(at.1),
            v.u(BIG.0),
            v.u(BIG.1),
            CardLook::default(),
        );
    }
    label(
        g,
        v,
        &format!("DECK · {}", game.cards_left()),
        DECK_POS.0,
        DECK_POS.1 + 78.0,
        12.5,
        Color::new(1.0, 1.0, 1.0, 0.55),
    );

    // Peeks 1 and 2: the bottom of the deck.
    let n = app.peeks.bottom_count();
    if n > 0 {
        let text = if n == 1 {
            "BOTTOM CARD"
        } else {
            "BOTTOM FIVE CARDS"
        };
        // Five cards need a little more room, so the row sits slightly right
        // of the deck to stay clear of the table edge.
        let (mid, step) = if n == 1 {
            (DECK_POS.0, 0.0)
        } else {
            (DECK_POS.0 + 8.0, 47.5)
        };
        label(g, v, text, mid, DECK_POS.1 + 104.0, 12.5, pal::XRAY);
        let cards = game.bottom_cards(n);
        let x_of = |i: usize| mid + (i as f32 - (n as f32 - 1.0) / 2.0) * step;
        for (i, card) in cards.iter().enumerate() {
            let look = CardLook {
                compact: true,
                ..Default::default()
            };
            draw_card(g, v, *card, true, (x_of(i), DECK_POS.1 + 150.0), MINI, look);
        }
        if n > 1 {
            // Left to right is the order in the deck; mark the very last card.
            label(
                g,
                v,
                "last",
                x_of(n - 1),
                DECK_POS.1 + 192.0,
                10.5,
                alpha(pal::XRAY, 0.8),
            );
        }
    }
}

fn draw_burns(app: &mut App, v: &View, now: f64, queue: &mut u32) {
    let show = app.peeks.burned;
    let color = if show {
        pal::XRAY
    } else {
        Color::new(1.0, 1.0, 1.0, 0.40)
    };
    label(&app.gfx, v, "BURNED", 835.0, BOARD_Y + 60.0, 12.5, color);
    for i in 0..3 {
        let at = (791.0 + i as f32 * 44.0, BOARD_Y);
        if let Some(card) = app.game.burned.get(i).copied() {
            if let Some(p) = pose(&mut app.anims, Slot::Burn(i as u8), show, at, now, queue) {
                let look = CardLook {
                    sx: p.sx,
                    ..Default::default()
                };
                draw_card(&app.gfx, v, card, p.face, (p.x, p.y), MID, look);
            }
        }
    }
}

fn draw_board(app: &mut App, v: &View, now: f64, queue: &mut u32, winning: Option<&[Card]>) {
    for i in 0..5 {
        let at = (500.0 + (i as f32 - 2.0) * BOARD_STEP, BOARD_Y);
        gfx::rrect_stroke(
            v.x(at.0 - BIG.0 / 2.0),
            v.y(at.1 - BIG.1 / 2.0),
            v.u(BIG.0),
            v.u(BIG.1),
            v.u(BIG.0 * 0.09),
            v.u(1.5).max(1.0),
            Color::new(1.0, 1.0, 1.0, 0.10),
        );
        if let Some(card) = app.game.board.get(i).copied() {
            if let Some(p) = pose(&mut app.anims, Slot::Board(i as u8), true, at, now, queue) {
                let mut look = CardLook {
                    sx: p.sx,
                    ..Default::default()
                };
                if let (Some(win), true) = (winning, p.settled) {
                    if win.contains(&card) {
                        look.glow = Some(pal::GOLD);
                    } else {
                        look.dim = 0.5;
                    }
                }
                draw_card(&app.gfx, v, card, p.face, (p.x, p.y), BIG, look);
            }
        }
    }
}

/// Peek 4: the bot's real hand hidden among three decoys.
fn draw_candidates(app: &App, v: &View, reveal: bool) {
    let g = &app.gfx;
    label(
        g,
        v,
        "BOT HOLDS ONE OF THESE",
        221.0,
        152.0,
        12.5,
        pal::XRAY,
    );
    for (k, hand) in app.game.candidates.iter().enumerate() {
        let cx = 173.0 + (k % 2) as f32 * 96.0;
        let cy = 200.0 + (k / 2) as f32 * 72.0;
        let is_real = k == app.game.real_candidate;
        for (i, card) in hand.iter().enumerate() {
            let mut look = CardLook {
                compact: true,
                ..Default::default()
            };
            if reveal {
                if is_real {
                    look.glow = Some(pal::GOLD);
                } else {
                    look.dim = 0.55;
                }
            }
            draw_card(
                g,
                v,
                *card,
                true,
                (cx + (i as f32 - 0.5) * 40.0, cy),
                MINI,
                look,
            );
        }
    }
}

fn draw_seat(
    app: &mut App,
    v: &View,
    seat: Seat,
    now: f64,
    queue: &mut u32,
    winning: Option<&[Card]>,
    reveal_bot: bool,
) {
    let (cards_y, plate_y, label_y, bet_y) = match seat {
        Seat::Bot => (215.0, 100.0, 291.0, 236.0),
        Seat::Human => (550.0, 682.0, 627.0, 528.0),
    };
    let i = seat.idx();
    let folded = app.game.folded == Some(seat);
    let hole = app.game.hole[i];

    // Hole cards.
    for (k, card) in hole.iter().enumerate() {
        let at = (500.0 + (k as f32 - 0.5) * 92.0, cards_y);
        let peeked = seat == Seat::Bot && app.peeks.opp_card && k == app.game.peek_index;
        let want_face = seat == Seat::Human || reveal_bot || peeked;
        if let Some(p) = pose(
            &mut app.anims,
            Slot::Hole(seat, k as u8),
            want_face,
            at,
            now,
            queue,
        ) {
            let mut look = CardLook {
                sx: p.sx,
                ..Default::default()
            };
            if folded {
                look.dim = 0.55;
            } else if let (Some(win), true) = (winning, p.settled) {
                if win.contains(card) {
                    look.glow = Some(pal::GOLD);
                } else {
                    look.dim = 0.5;
                }
            } else if peeked && p.face && p.settled {
                look.glow = Some(pal::XRAY);
            }
            draw_card(&app.gfx, v, *card, p.face, (p.x, p.y), BIG, look);
        }
    }

    let g = &app.gfx;
    let game = &app.game;

    // What the hand currently amounts to.
    let shown = game
        .result
        .as_ref()
        .and_then(|r| r.values.as_ref())
        .map(|vals| {
            let (mine, theirs) = describe_versus(&vals[0], &vals[1]);
            if seat == Seat::Human {
                mine
            } else {
                theirs
            }
        });
    let holding = match seat {
        Seat::Human if folded => None,
        Seat::Human => shown.or_else(|| Some(describe_holding(&hole, &game.board))),
        Seat::Bot if reveal_bot => shown,
        Seat::Bot => None,
    };
    if let Some(text) = holding {
        let size = 14.5;
        let w = g.width(&text, v.u(size), false) + v.u(24.0);
        gfx::pill(
            v.x(500.0) - w / 2.0,
            v.y(label_y - 12.0),
            w,
            v.u(24.0),
            Color::new(0.0, 0.0, 0.0, 0.34),
            None,
        );
        label(g, v, &text, 500.0, label_y, size, pal::TEXT);
    }

    // Dealer button.
    if game.dealer == seat {
        let (bx, by) = (372.0, cards_y + 36.0);
        gfx::disc(
            v.x(bx),
            v.y(by + 2.0),
            v.u(16.0),
            Color::new(0.0, 0.0, 0.0, 0.35),
        );
        gfx::disc(v.x(bx), v.y(by), v.u(16.0), gfx::rgb(240, 238, 230));
        gfx::disc(v.x(bx), v.y(by), v.u(13.0), gfx::rgb(252, 250, 244));
        g.text(
            "D",
            v.x(bx),
            v.y(by),
            v.u(17.0),
            pal::INK_BLACK,
            Align::Center,
            true,
        );
    }

    // Points put in on this street.
    let bet = game.street_bets[i];
    if bet > 0 {
        let color = if seat == Seat::Human {
            pal::CHIP_BLUE
        } else {
            pal::CHIP_RED
        };
        gfx::chip_stack(v.x(652.0), v.y(bet_y), v.u(15.0), bet, color);
        g.text(
            &bet.to_string(),
            v.x(676.0),
            v.y(bet_y - 3.0),
            v.u(20.0),
            pal::TEXT,
            Align::Left,
            true,
        );
    }

    draw_plate(app, v, seat, plate_y, now);
}

fn draw_plate(app: &App, v: &View, seat: Seat, cy: f32, now: f64) {
    let g = &app.gfx;
    let game = &app.game;
    let (w, h) = (236.0, 56.0);
    let (x, y) = (500.0 - w / 2.0, cy - h / 2.0);
    let active = game.state == State::ToAct(seat);
    let accent = if seat == Seat::Human {
        pal::YOU
    } else {
        pal::BOT
    };

    gfx::pill(
        v.x(x),
        v.y(y + 3.0),
        v.u(w),
        v.u(h),
        Color::new(0.0, 0.0, 0.0, 0.35),
        None,
    );
    let edge = if active {
        let pulse = 0.65 + 0.35 * ((now * 4.0).sin() as f32);
        (v.u(2.6), alpha(pal::GOLD, pulse))
    } else {
        (v.u(1.4).max(1.0), pal::PLATE_EDGE)
    };
    gfx::pill(v.x(x), v.y(y), v.u(w), v.u(h), pal::PLATE, Some(edge));

    // Avatar.
    let (ax, ay) = (x + h / 2.0, cy);
    gfx::disc(v.x(ax), v.y(ay), v.u(20.0), gfx::shade(accent, 0.55));
    gfx::disc(v.x(ax), v.y(ay), v.u(17.5), accent);
    let initial = if seat == Seat::Human { "Y" } else { "B" };
    g.text(
        initial,
        v.x(ax),
        v.y(ay),
        v.u(19.0),
        pal::PLATE,
        Align::Center,
        true,
    );

    // Name, role and stack.
    let name = if seat == Seat::Human { "YOU" } else { "BOT" };
    let tx = x + h + 4.0;
    let nw = g.text(
        name,
        v.x(tx),
        v.y(cy - 12.0),
        v.u(12.5),
        accent,
        Align::Left,
        false,
    );
    let role = if game.dealer == seat {
        "DEALER"
    } else {
        "BLIND"
    };
    g.text(
        role,
        v.x(tx) + nw + v.u(8.0),
        v.y(cy - 12.0),
        v.u(11.0),
        pal::MUTED,
        Align::Left,
        false,
    );

    let stack = game.stacks[seat.idx()];
    let sw = g.text(
        &stack.to_string(),
        v.x(tx),
        v.y(cy + 9.0),
        v.u(23.0),
        pal::TEXT,
        Align::Left,
        true,
    );
    let unit = if stack == 1 { "point" } else { "points" };
    g.text(
        unit,
        v.x(tx) + sw + v.u(6.0),
        v.y(cy + 11.0),
        v.u(12.5),
        pal::MUTED,
        Align::Left,
        false,
    );

    // Thinking dots.
    if seat == Seat::Bot && app.bot_thinking() {
        for d in 0..3 {
            let phase = (now * 5.0 - d as f64 * 0.7).sin() as f32;
            let a = 0.35 + 0.65 * phase.max(0.0);
            gfx::disc(
                v.x(x + w - 44.0 + d as f32 * 11.0),
                v.y(cy),
                v.u(3.4),
                alpha(pal::TEXT, a),
            );
        }
    }

    // Last action, fading out beside the plate.
    if let Some(bubble) = &app.bubbles[seat.idx()] {
        let age = now - bubble.at;
        let fade = (1.0 - ((age - 1.8) / 0.5).clamp(0.0, 1.0)) as f32;
        if fade > 0.0 {
            let pop = (1.0 - (age / 0.18).min(1.0)) as f32;
            let size = 15.0 + pop * 3.0;
            let tw = g.width(bubble.text, v.u(size), true);
            let bw = tw + v.u(30.0);
            let bx = v.x(x + w + 14.0);
            let bh = v.u(34.0);
            gfx::pill(
                bx,
                v.y(cy) - bh / 2.0,
                bw,
                bh,
                alpha(pal::PLATE, 0.92 * fade),
                Some((v.u(1.6).max(1.0), alpha(bubble.color, fade))),
            );
            g.text(
                bubble.text,
                bx + bw / 2.0,
                v.y(cy),
                v.u(size),
                alpha(bubble.color, fade),
                Align::Center,
                true,
            );
        }
    }
}

fn draw_pot(app: &App, v: &View) {
    let g = &app.gfx;
    let game = &app.game;
    let cy = 462.0;

    if let (State::HandOver, Some(result)) = (game.state, &game.result) {
        let (text, color) = match result.winner {
            Some(Seat::Human) => (format!("YOU WIN {}", result.pot), pal::GOLD),
            Some(Seat::Bot) => (format!("BOT WINS {}", result.pot), pal::BOT),
            None => ("SPLIT POT".to_string(), pal::TEXT),
        };
        let size = 19.0;
        let w = g.width(&text, v.u(size), true) + v.u(44.0);
        let h = v.u(40.0);
        gfx::pill(
            v.x(500.0) - w / 2.0,
            v.y(cy) - h / 2.0,
            w,
            h,
            alpha(pal::PLATE, 0.94),
            Some((v.u(2.0), color)),
        );
        g.text(
            &text,
            v.x(500.0),
            v.y(cy),
            v.u(size),
            color,
            Align::Center,
            true,
        );
        return;
    }

    let text = format!("POT  {}", game.pot());
    let size = 16.0;
    let tw = g.width(&text, v.u(size), true);
    let w = tw + v.u(62.0);
    let h = v.u(34.0);
    let x = v.x(500.0) - w / 2.0;
    gfx::pill(
        x,
        v.y(cy) - h / 2.0,
        w,
        h,
        Color::new(0.0, 0.0, 0.0, 0.36),
        None,
    );
    gfx::chip(x + v.u(22.0), v.y(cy), v.u(10.5), pal::GOLD);
    g.text(
        &text,
        x + v.u(40.0),
        v.y(cy),
        v.u(size),
        pal::TEXT,
        Align::Left,
        true,
    );
}

/// Colour for a chance of winning. The two certainties get colours of their
/// own; everything in between runs from red through amber to green.
fn odds_color(o: &Odds) -> Color {
    if o.cannot_lose() {
        return pal::GOLD;
    }
    if o.cannot_win() {
        return gfx::rgb(150, 124, 196);
    }
    let stops: [(f32, Color); 6] = [
        (0.00, gfx::rgb(196, 44, 52)),
        (0.20, gfx::rgb(232, 76, 60)),
        (0.40, gfx::rgb(240, 146, 54)),
        (0.50, gfx::rgb(236, 204, 72)),
        (0.65, gfx::rgb(150, 208, 92)),
        (1.00, gfx::rgb(50, 196, 112)),
    ];
    let w = o.win as f32;
    for pair in stops.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if w <= b {
            let t = ((w - a) / (b - a)).clamp(0.0, 1.0);
            return Color::new(
                ca.r + (cb.r - ca.r) * t,
                ca.g + (cb.g - ca.g) * t,
                ca.b + (cb.b - ca.b) * t,
                1.0,
            );
        }
    }
    stops[5].1
}

fn odds_word(o: &Odds) -> &'static str {
    if o.cannot_lose() && o.cannot_win() {
        return "SURE SPLIT";
    }
    if o.cannot_lose() {
        return "CAN'T LOSE";
    }
    if o.cannot_win() {
        return "CAN'T WIN";
    }
    match o.win {
        w if w < 0.10 => "NEARLY DEAD",
        w if w < 0.30 => "LONG SHOT",
        w if w < 0.45 => "UNDERDOG",
        w if w < 0.55 => "COIN FLIP",
        w if w < 0.70 => "AHEAD",
        w if w < 0.90 => "BIG FAVOURITE",
        _ => "NEAR LOCK",
    }
}

/// A probability as a whole percentage. 0% and 100% are only ever printed
/// when they are literally true; a 99.7% that is not a certainty reads ">99%".
fn percent(p: f64) -> String {
    let rounded = (p * 100.0).round() as i32;
    if rounded >= 100 && p < 1.0 {
        ">99%".to_string()
    } else if rounded <= 0 && p > 0.0 {
        "<1%".to_string()
    } else {
        format!("{rounded}%")
    }
}

/// Peek 6: the chance of winning, given everything the player can see.
fn draw_odds(app: &App, v: &View, now: f64) {
    let g = &app.gfx;
    let (cx, cy, w, h) = (800.0, 546.0, 172.0, 104.0);
    let (x, y) = (cx - w / 2.0, cy - h / 2.0);
    let shown = app.odds.shown;
    let color = shown.as_ref().map(odds_color).unwrap_or(pal::MUTED);
    // Figures that are being recalculated stay up, faded, until the new ones land.
    let fade = if app.odds.stale { 0.4 } else { 1.0 };

    gfx::rrect(
        v.x(x),
        v.y(y + 3.0),
        v.u(w),
        v.u(h),
        v.u(14.0),
        Color::new(0.0, 0.0, 0.0, 0.28),
    );
    gfx::rrect(
        v.x(x),
        v.y(y),
        v.u(w),
        v.u(h),
        v.u(14.0),
        alpha(pal::PLATE, 0.90),
    );
    gfx::rrect_stroke(
        v.x(x),
        v.y(y),
        v.u(w),
        v.u(h),
        v.u(14.0),
        v.u(1.8).max(1.0),
        alpha(color, 0.85 * fade),
    );
    label(g, v, "CHANCE TO WIN", cx, y + 15.0, 11.5, pal::XRAY);

    let Some(o) = shown else {
        // Nothing to show yet: animated dots while the first figures arrive.
        for d in 0..3 {
            let phase = (now * 5.0 - d as f64 * 0.7).sin() as f32;
            let a = 0.3 + 0.7 * phase.max(0.0);
            gfx::disc(
                v.x(cx + (d as f32 - 1.0) * 13.0),
                v.y(cy + 4.0),
                v.u(4.0),
                alpha(pal::MUTED, a),
            );
        }
        return;
    };

    // Sampled figures carry a "~"; counted ones do not.
    let big = if o.exact {
        percent(o.win)
    } else {
        format!("~{}", percent(o.win))
    };
    g.text(
        &big,
        v.x(cx),
        v.y(y + 43.0),
        v.u(33.0),
        alpha(color, fade),
        Align::Center,
        true,
    );

    // Win / tie / lose bar.
    let (bx, by, bw, bh) = (x + 16.0, y + 66.0, w - 32.0, 7.0);
    gfx::rrect(
        v.x(bx),
        v.y(by),
        v.u(bw),
        v.u(bh),
        v.u(bh / 2.0),
        gfx::rgb(52, 56, 68),
    );
    let win_w = bw * o.win as f32;
    let tie_w = bw * o.tie as f32;
    if tie_w > 0.3 {
        gfx::rrect(
            v.x(bx),
            v.y(by),
            v.u(win_w + tie_w),
            v.u(bh),
            v.u(bh / 2.0),
            alpha(gfx::rgb(170, 176, 190), fade),
        );
    }
    if win_w > 0.3 {
        gfx::rrect(
            v.x(bx),
            v.y(by),
            v.u(win_w),
            v.u(bh),
            v.u(bh / 2.0),
            alpha(color, fade),
        );
    }

    let caption = if o.tie >= 0.005 && !(o.cannot_lose() && o.cannot_win()) {
        format!("{} · tie {}", odds_word(&o), percent(o.tie))
    } else {
        odds_word(&o).to_string()
    };
    label(
        g,
        v,
        &caption,
        cx,
        y + 88.0,
        11.0,
        alpha(pal::TEXT, 0.85 * fade),
    );
}
