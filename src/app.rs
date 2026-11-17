//! Application state that sits between the rules engine and the screen:
//! pacing (bot "thinking" time, dealing pauses), the hand log, the peek
//! toggles and per-card animation bookkeeping.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use macroquad::prelude::Color;

use crate::bot;
use crate::cards::labels;
use crate::eval::describe_versus;
use crate::game::{Action, Event, Game, Move, Seat, State, BOTTOM_PEEK};
use crate::gfx::{pal, Gfx};
use crate::odds::{self, Odds, Sight};
use crate::rng::Rng;
use crate::table::{Slot, SlotAnim};

pub const STACK_CHOICES: [u32; 4] = [10, 20, 30, 50];

/// The "X-ray" options. They only change what is drawn for the human
/// player; the bot never sees any of this.
#[derive(Clone, Copy, Default)]
pub struct Peeks {
    /// 1) the bottom card of the deck
    pub bottom1: bool,
    /// 2) the five bottom cards of the deck
    pub bottom5: bool,
    /// 3) one of the bot's hole cards
    pub opp_card: bool,
    /// 4) four possible bot hands: the real one and three decoys
    pub candidates: bool,
    /// 5) the burned cards
    pub burned: bool,
    /// 6) the chance of winning, given everything else that is visible
    pub odds: bool,
}

impl Peeks {
    pub fn toggle(&mut self, number: u8) {
        match number {
            1 => self.bottom1 = !self.bottom1,
            2 => self.bottom5 = !self.bottom5,
            3 => self.opp_card = !self.opp_card,
            4 => self.candidates = !self.candidates,
            5 => self.burned = !self.burned,
            6 => self.odds = !self.odds,
            _ => {}
        }
    }

    pub fn set_all(&mut self, on: bool) {
        *self = Peeks {
            bottom1: on,
            bottom5: on,
            opp_card: on,
            candidates: on,
            burned: on,
            odds: on,
        };
    }

    pub fn any(&self) -> bool {
        self.bottom1 || self.bottom5 || self.opp_card || self.candidates || self.burned || self.odds
    }

    /// How many bottom-of-deck cards are currently face up.
    pub fn bottom_count(&self) -> usize {
        if self.bottom5 {
            BOTTOM_PEEK
        } else if self.bottom1 {
            1
        } else {
            0
        }
    }
}

/// Keeps the win-odds display up to date. The sums can take a noticeable
/// fraction of a second, so they run on a worker thread; results are cached
/// by what the player could see, which makes flipping an option off and on
/// again instant.
pub struct OddsTracker {
    cache: HashMap<u64, Odds>,
    pending: HashSet<u64>,
    tx: Sender<(u64, Odds)>,
    rx: Receiver<(u64, Odds)>,
    /// The figures on screen, if any.
    pub shown: Option<Odds>,
    /// `true` while `shown` belongs to an earlier state of knowledge and
    /// the up-to-date figures are still being worked out.
    pub stale: bool,
    /// Don't react before this time: the cards that changed the picture are
    /// still sliding across the table and have not been turned over yet.
    hold_until: f64,
    board_len: usize,
}

impl OddsTracker {
    fn new() -> OddsTracker {
        let (tx, rx) = channel();
        OddsTracker {
            cache: HashMap::new(),
            pending: HashSet::new(),
            tx,
            rx,
            shown: None,
            stale: false,
            hold_until: 0.0,
            board_len: 0,
        }
    }

    fn new_hand(&mut self, now: f64) {
        self.shown = None;
        self.stale = false;
        self.board_len = 0;
        // Roughly when the player's own two cards have landed face up.
        self.hold_until = now + 0.9;
        if self.cache.len() > 2000 {
            self.cache.clear();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum LogKind {
    Header,
    Info,
    You,
    Bot,
    Board,
    Result,
}

pub struct LogLine {
    pub text: String,
    pub kind: LogKind,
}

/// Short-lived label next to a player's name plate ("RAISE", "CHECK"…).
pub struct Bubble {
    pub text: &'static str,
    pub color: Color,
    pub at: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Neutral,
    YourTurn,
    Good,
    Bad,
}

pub struct App {
    pub game: Game,
    pub peeks: Peeks,
    pub gfx: Gfx,
    pub anims: HashMap<Slot, SlotAnim>,
    pub log: Vec<LogLine>,
    pub bubbles: [Option<Bubble>; 2],
    /// Starting stack selected in the sidebar for the *next* match.
    pub pending_stack: u32,
    pub matches_won: [u32; 2],
    /// Shorter pauses for the bot and the dealing.
    pub quick: bool,
    pub odds: OddsTracker,

    bot_rng: Rng,
    seen_seq: u64,
    ready_at: f64,
    hand_started_at: f64,
}

fn seed() -> u64 {
    // Milliseconds since the epoch; works on desktop and in the browser.
    (macroquad::miniquad::date::now() * 1000.0) as u64
}

fn who(seat: Seat) -> &'static str {
    match seat {
        Seat::Human => "You",
        Seat::Bot => "Bot",
    }
}

fn points(n: u32) -> String {
    if n == 1 {
        "1 point".to_string()
    } else {
        format!("{n} points")
    }
}

impl App {
    pub fn new(gfx: Gfx, now: f64) -> App {
        let s = seed();
        let mut app = App {
            game: Game::new(20, s),
            peeks: Peeks::default(),
            gfx,
            anims: HashMap::new(),
            log: Vec::new(),
            bubbles: [None, None],
            pending_stack: 20,
            matches_won: [0; 2],
            quick: false,
            odds: OddsTracker::new(),
            bot_rng: Rng::new(s.rotate_left(17) ^ 0x5EED),
            seen_seq: u64::MAX,
            ready_at: 0.0,
            hand_started_at: now,
        };
        app.odds.new_hand(now);
        app.log.push(LogLine {
            text: format!("New match · {} each", points(20)),
            kind: LogKind::Info,
        });
        app
    }

    pub fn new_match(&mut self, now: f64) {
        self.game = Game::new(self.pending_stack, seed());
        self.anims.clear();
        self.bubbles = [None, None];
        self.seen_seq = u64::MAX;
        self.hand_started_at = now;
        self.odds.new_hand(now);
        self.log.clear();
        self.log.push(LogLine {
            text: format!("New match · {} each", points(self.pending_stack)),
            kind: LogKind::Info,
        });
    }

    pub fn can_start_next_hand(&self) -> bool {
        self.game.state == State::HandOver && self.game.match_winner().is_none()
    }

    pub fn next_hand(&mut self, now: f64) {
        if self.can_start_next_hand() {
            self.game.start_hand();
            self.anims.clear();
            self.bubbles = [None, None];
            self.hand_started_at = now;
            self.odds.new_hand(now);
        }
    }

    pub fn humans_turn(&self) -> bool {
        self.game.state == State::ToAct(Seat::Human)
    }

    pub fn human_action(&mut self, action: Action) {
        if self.humans_turn() {
            self.game.act(action);
        }
    }

    pub fn bot_thinking(&self) -> bool {
        self.game.state == State::ToAct(Seat::Bot)
    }

    /// Called once per frame: turns engine events into log lines and runs
    /// the automatic steps (bot moves, dealing) on a human-friendly clock.
    pub fn update(&mut self, now: f64) {
        self.drain_events(now);

        if self.seen_seq != self.game.seq {
            self.seen_seq = self.game.seq;
            let mut wait = match self.game.state {
                State::ToAct(Seat::Bot) => 0.85 + self.bot_rng.unit() as f64 * 0.7,
                // Linger a little when the board is being run out all-in.
                State::Dealing if self.game.stacks.contains(&0) => 1.0,
                State::Dealing => 0.55,
                _ => 0.0,
            };
            if self.quick {
                wait *= 0.35;
            }
            // Let the opening deal land before the bot does anything.
            if now - self.hand_started_at < 0.1 {
                wait += 0.6;
            }
            self.ready_at = now + wait;
        }

        if now >= self.ready_at {
            match self.game.state {
                State::ToAct(Seat::Bot) => {
                    let action = bot::decide(&self.game, &mut self.bot_rng);
                    self.game.act(action);
                }
                State::Dealing => self.game.advance(),
                _ => {}
            }
        }
        self.drain_events(now);
        self.refresh_odds(now);
    }

    /// What the human can see right now that matters for the odds: their
    /// own cards and the board, plus whatever the other options expose.
    fn sight(&self) -> Sight {
        let g = &self.game;
        let p = &self.peeks;
        let bot = g.hole[Seat::Bot.idx()];
        // At a showdown the bot's hand is on the table for everyone.
        let revealed = g.state == State::HandOver && g.folded.is_none();

        let mut dead = g.bottom_cards(p.bottom_count()).to_vec();
        if p.burned {
            dead.extend_from_slice(&g.burned);
        }
        Sight {
            hole: g.hole[Seat::Human.idx()],
            board: g.board.clone(),
            dead,
            opp_card: (p.opp_card || revealed).then(|| bot[g.peek_index]),
            candidates: if revealed {
                vec![bot]
            } else if p.candidates {
                g.candidates.to_vec()
            } else {
                Vec::new()
            },
        }
    }

    fn refresh_odds(&mut self, now: f64) {
        // Collect finished calculations, whether or not they are still wanted.
        while let Ok((key, result)) = self.odds.rx.try_recv() {
            self.odds.pending.remove(&key);
            self.odds.cache.insert(key, result);
        }
        if !self.peeks.odds {
            return;
        }
        // New board cards exist in the rules engine the instant they are
        // dealt, but take a moment to arrive face up on screen. Wait for
        // that, or the number would give the cards away early.
        let board_len = self.game.board.len();
        if board_len != self.odds.board_len {
            let dealt = board_len.saturating_sub(self.odds.board_len) as f64;
            self.odds.board_len = board_len;
            self.odds.hold_until = now + 0.5 + 0.11 * dealt;
        }
        if now < self.odds.hold_until {
            return;
        }

        let sight = self.sight();
        let key = sight.key();
        if let Some(result) = self.odds.cache.get(&key) {
            self.odds.shown = Some(*result);
            self.odds.stale = false;
        } else {
            self.odds.stale = true;
            if self.odds.pending.insert(key) {
                let tx = self.odds.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send((key, odds::compute(&sight)));
                });
            }
        }
    }

    fn say(&mut self, kind: LogKind, text: String) {
        self.log.push(LogLine { text, kind });
        if self.log.len() > 400 {
            self.log.drain(..100);
        }
    }

    fn drain_events(&mut self, now: f64) {
        let events: Vec<Event> = self.game.events.drain(..).collect();
        for event in events {
            match event {
                Event::HandStart { no, dealer } => {
                    self.say(LogKind::Header, format!("Hand {no}"));
                    let text = match dealer {
                        Seat::Human => "You deal · Bot posts the blind",
                        Seat::Bot => "Bot deals · You post the blind",
                    };
                    self.say(LogKind::Info, text.to_string());
                }
                Event::Blind { seat } => {
                    self.bubbles[seat.idx()] = Some(Bubble {
                        text: "BLIND",
                        color: pal::MUTED,
                        at: now,
                    });
                }
                Event::Moved { seat, mv } => {
                    let (verb_you, verb_bot, tag, color) = match mv {
                        Move::Fold => ("fold", "folds", "FOLD", pal::CHIP_RED),
                        Move::Check => ("check", "checks", "CHECK", pal::MUTED),
                        Move::Call => ("call", "calls", "CALL", pal::YOU),
                        Move::Bet => ("bet", "bets", "BET", pal::GOLD),
                        Move::Raise => ("raise", "raises", "RAISE", pal::GOLD),
                    };
                    let (kind, verb) = match seat {
                        Seat::Human => (LogKind::You, verb_you),
                        Seat::Bot => (LogKind::Bot, verb_bot),
                    };
                    self.say(kind, format!("{} {}", who(seat), verb));
                    self.bubbles[seat.idx()] = Some(Bubble {
                        text: tag,
                        color,
                        at: now,
                    });
                }
                Event::Dealt { street, cards } => {
                    self.say(
                        LogKind::Board,
                        format!("{}  {}", street.name(), labels(&cards)),
                    );
                }
                Event::Showdown { hole, values } => {
                    let (mine, theirs) = describe_versus(&values[0], &values[1]);
                    self.say(
                        LogKind::You,
                        format!("You show {} · {mine}", labels(&hole[Seat::Human.idx()])),
                    );
                    self.say(
                        LogKind::Bot,
                        format!("Bot shows {} · {theirs}", labels(&hole[Seat::Bot.idx()])),
                    );
                }
                Event::Won { winner, pot } => {
                    let text = match winner {
                        Some(Seat::Human) => format!("You win the pot of {pot}"),
                        Some(Seat::Bot) => format!("Bot wins the pot of {pot}"),
                        None => "Split pot".to_string(),
                    };
                    self.say(LogKind::Result, text);
                }
                Event::MatchOver { winner } => {
                    self.matches_won[winner.idx()] += 1;
                    let text = match winner {
                        Seat::Human => "You win the match!",
                        Seat::Bot => "Bot wins the match",
                    };
                    self.say(LogKind::Header, text.to_string());
                }
            }
        }
    }

    /// One-line status for the action bar.
    pub fn status(&self) -> (String, Tone) {
        let g = &self.game;
        match g.state {
            State::ToAct(Seat::Human) => {
                let legal = g.legal();
                let text = if legal.to_call > 0 {
                    format!("Your move · {} to call", points(legal.to_call))
                } else {
                    "Your move · nothing to call".to_string()
                };
                (text, Tone::YourTurn)
            }
            State::ToAct(Seat::Bot) => ("Bot is thinking…".to_string(), Tone::Neutral),
            State::Dealing => {
                let text = if g.stacks.contains(&0) {
                    "All in · running out the board…"
                } else {
                    "Dealing…"
                };
                (text.to_string(), Tone::Neutral)
            }
            State::HandOver => {
                let Some(result) = &g.result else {
                    return (String::new(), Tone::Neutral);
                };
                let pot = result.pot;
                let (mut text, mut tone) = match (result.winner, result.by_fold, &result.values) {
                    (Some(Seat::Human), true, _) => {
                        (format!("Bot folded · you win the pot of {pot}"), Tone::Good)
                    }
                    (Some(Seat::Bot), true, _) => (
                        format!("You folded · Bot takes the pot of {pot}"),
                        Tone::Bad,
                    ),
                    (Some(Seat::Human), false, Some(v)) => {
                        let (mine, theirs) = describe_versus(&v[0], &v[1]);
                        (
                            format!("You win the pot of {pot} · {mine} beats {theirs}"),
                            Tone::Good,
                        )
                    }
                    (Some(Seat::Bot), false, Some(v)) => {
                        let (mine, theirs) = describe_versus(&v[0], &v[1]);
                        (
                            format!("Bot wins the pot of {pot} · {theirs} beats {mine}"),
                            Tone::Bad,
                        )
                    }
                    (None, _, Some(v)) => (
                        format!(
                            "Split pot · you both have {}",
                            describe_versus(&v[0], &v[1]).0
                        ),
                        Tone::Neutral,
                    ),
                    _ => (String::new(), Tone::Neutral),
                };
                match g.match_winner() {
                    Some(Seat::Human) => {
                        text = format!("You won the match!  {text}");
                        tone = Tone::Good;
                    }
                    Some(Seat::Bot) => {
                        text = format!("Bot won the match.  {text}");
                        tone = Tone::Bad;
                    }
                    None => {}
                }
                (text, tone)
            }
        }
    }
}
