//! Heads-up, fixed-limit Texas Hold'em.
//!
//! House rules (kept deliberately simple):
//! * No small blind. One player posts a 1-point blind; the other is the dealer.
//! * Every bet and every raise is exactly 1 point. So calling always costs 1
//!   and raising always costs 2 (1 to call + 1 more). Opening the betting when
//!   there is nothing to call costs 1.
//! * At most `MAX_BETS` bets per street (the blind counts as the first one
//!   before the flop).
//! * The dealer acts first before the flop, the blind acts first afterwards.
//!
//! Because every bet is a single point, a player who still has chips can
//! always cover a call in full, so side pots never arise. A raise is only
//! allowed when the opponent has a point left to call it with.
//!
//! The module is a pure state machine with no rendering or timing: the UI
//! calls [`Game::act`] for whoever is to act and [`Game::advance`] whenever
//! the state is [`State::Dealing`].

use crate::cards::{full_deck, Card};
use crate::eval::{best_of, HandValue};
use crate::rng::Rng;

pub const MAX_BETS: u32 = 4;
pub const BLIND: u32 = 1;

/// Deck positions consumed by one full hand: 4 hole cards, 3 burns, 5 board.
pub const CARDS_IN_PLAY: usize = 12;

/// How many cards the larger "bottom of the deck" peek exposes.
pub const BOTTOM_PEEK: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Seat {
    Human = 0,
    Bot = 1,
}

impl Seat {
    pub fn other(self) -> Seat {
        match self {
            Seat::Human => Seat::Bot,
            Seat::Bot => Seat::Human,
        }
    }
    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Street {
    PreFlop,
    Flop,
    Turn,
    River,
}

impl Street {
    pub fn name(self) -> &'static str {
        match self {
            Street::PreFlop => "Pre-flop",
            Street::Flop => "Flop",
            Street::Turn => "Turn",
            Street::River => "River",
        }
    }
}

/// The three buttons. `Call` doubles as "check" when there is nothing to
/// call, and `Raise` doubles as "bet".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Fold,
    Call,
    Raise,
}

/// What an action actually amounted to, for the log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Fold,
    Check,
    Call,
    Bet,
    Raise,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Waiting for this seat to act.
    ToAct(Seat),
    /// Betting round finished: call [`Game::advance`] to deal the next
    /// street (or to reach the showdown after the river).
    Dealing,
    /// Hand finished; see [`Game::result`]. Call [`Game::start_hand`].
    HandOver,
}

#[derive(Clone, Debug)]
pub struct HandResult {
    /// `None` means the pot was split.
    pub winner: Option<Seat>,
    pub by_fold: bool,
    pub pot: u32,
    /// Showdown only: each seat's best hand.
    pub values: Option<[HandValue; 2]>,
    pub best_five: Option<[[Card; 5]; 2]>,
}

#[derive(Clone, Debug)]
pub enum Event {
    HandStart {
        no: u32,
        dealer: Seat,
    },
    Blind {
        seat: Seat,
    },
    Moved {
        seat: Seat,
        mv: Move,
    },
    Dealt {
        street: Street,
        cards: Vec<Card>,
    },
    Showdown {
        hole: [[Card; 2]; 2],
        values: [HandValue; 2],
    },
    Won {
        winner: Option<Seat>,
        pot: u32,
    },
    MatchOver {
        winner: Seat,
    },
}

/// What the seat to act may do right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Legal {
    pub to_call: u32,
    pub can_fold: bool,
    pub can_raise: bool,
    /// Total cost of pressing "raise" (the call plus the 1-point raise).
    pub raise_cost: u32,
}

pub struct Game {
    rng: Rng,
    /// The complete shuffled deck for the current hand. It is never mutated
    /// while the hand is running: `next_card` just walks down from the top
    /// (index 0). The last element is the bottom card.
    pub deck: Vec<Card>,
    pub next_card: usize,

    pub hole: [[Card; 2]; 2],
    pub board: Vec<Card>,
    pub burned: Vec<Card>,

    pub stacks: [u32; 2],
    /// Points each seat has put in over the whole hand.
    pub committed: [u32; 2],
    /// Points each seat has put in on the current street.
    pub street_bets: [u32; 2],
    /// Bets + raises made on the current street.
    pub bet_level: u32,
    acted: [bool; 2],

    pub dealer: Seat,
    pub street: Street,
    pub state: State,
    pub result: Option<HandResult>,
    pub folded: Option<Seat>,

    pub hand_no: u32,
    pub hands_won: [u32; 2],

    /// Which of the bot's two hole cards the "show one card" peek exposes.
    pub peek_index: usize,
    /// Four candidate hands for the bot: the real one plus three decoys,
    /// in random order. Each is sorted high card first.
    pub candidates: [[Card; 2]; 4],
    pub real_candidate: usize,

    /// Things that happened since the UI last drained this list.
    pub events: Vec<Event>,
    /// Bumped on every state change so the UI can notice transitions.
    pub seq: u64,
}

impl Game {
    pub fn new(starting_stack: u32, seed: u64) -> Game {
        let mut rng = Rng::new(seed);
        let dealer = if rng.chance(0.5) {
            Seat::Human
        } else {
            Seat::Bot
        };
        let placeholder = full_deck()[0];
        let mut game = Game {
            rng,
            deck: Vec::new(),
            next_card: 0,
            hole: [[placeholder; 2]; 2],
            board: Vec::with_capacity(5),
            burned: Vec::with_capacity(3),
            stacks: [starting_stack.max(1); 2],
            committed: [0; 2],
            street_bets: [0; 2],
            bet_level: 0,
            acted: [false; 2],
            // start_hand() flips this, so the randomly chosen seat deals first.
            dealer: dealer.other(),
            street: Street::PreFlop,
            state: State::HandOver,
            result: None,
            folded: None,
            hand_no: 0,
            hands_won: [0; 2],
            peek_index: 0,
            candidates: [[placeholder; 2]; 4],
            real_candidate: 0,
            events: Vec::new(),
            seq: 0,
        };
        game.start_hand();
        game
    }

    /// The seat posting the blind this hand.
    pub fn blind_seat(&self) -> Seat {
        self.dealer.other()
    }

    /// Everything put in this hand. After the hand is over this still reports
    /// the pot that was just awarded (the stacks already include it).
    pub fn pot(&self) -> u32 {
        self.committed[0] + self.committed[1]
    }

    /// `Some` once a player has run out of points.
    pub fn match_winner(&self) -> Option<Seat> {
        if self.state != State::HandOver {
            return None;
        }
        if self.stacks[Seat::Bot.idx()] == 0 {
            Some(Seat::Human)
        } else if self.stacks[Seat::Human.idx()] == 0 {
            Some(Seat::Bot)
        } else {
            None
        }
    }

    /// The last `n` cards of the deck, bottom card last.
    pub fn bottom_cards(&self, n: usize) -> &[Card] {
        &self.deck[self.deck.len() - n..]
    }

    pub fn cards_left(&self) -> usize {
        self.deck.len() - self.next_card
    }

    /// Shuffle up and deal the next hand. Does nothing while a hand is in
    /// progress or once the match is over.
    pub fn start_hand(&mut self) {
        if self.state != State::HandOver || self.match_winner().is_some() {
            return;
        }
        self.hand_no += 1;
        self.dealer = self.dealer.other();
        let blind = self.blind_seat();

        self.deck = full_deck();
        self.rng.shuffle(&mut self.deck);
        self.next_card = 0;
        self.board.clear();
        self.burned.clear();
        self.committed = [0; 2];
        self.street_bets = [0; 2];
        self.acted = [false; 2];
        self.result = None;
        self.folded = None;
        self.street = Street::PreFlop;

        self.events.push(Event::HandStart {
            no: self.hand_no,
            dealer: self.dealer,
        });

        // One card at a time, starting with the player who is not dealing.
        for round in 0..2 {
            for seat in [blind, self.dealer] {
                self.hole[seat.idx()][round] = self.draw();
            }
        }

        self.pay(blind, BLIND);
        self.bet_level = 1;
        self.events.push(Event::Blind { seat: blind });

        self.prepare_peeks();

        self.state = State::ToAct(self.dealer);
        self.seq += 1;
    }

    /// Sets up the information behind the "one card" and "four possible
    /// hands" peeks. It is fixed for the whole hand so that toggling the
    /// options on and off never re-rolls anything.
    fn prepare_peeks(&mut self) {
        self.peek_index = self.rng.below(2);

        // Decoys are built from cards that can never show up anywhere during
        // this hand: not dealt, not burned, not on the board, and not one of
        // the bottom cards that another peek can expose. That keeps all four
        // candidates plausible right through to the river.
        let mut pool: Vec<Card> = self.deck[CARDS_IN_PLAY..self.deck.len() - BOTTOM_PEEK].to_vec();
        self.rng.shuffle(&mut pool);

        self.real_candidate = self.rng.below(4);
        let mut decoys = pool.chunks_exact(2);
        for slot in 0..4 {
            let mut hand = if slot == self.real_candidate {
                self.hole[Seat::Bot.idx()]
            } else {
                let pair = decoys.next().expect("pool has plenty of cards");
                [pair[0], pair[1]]
            };
            // Uniform ordering, so the real hand can't be told apart by layout.
            hand.sort_by(|a, b| b.rank.cmp(&a.rank).then(b.suit.cmp(&a.suit)));
            self.candidates[slot] = hand;
        }
    }

    fn draw(&mut self) -> Card {
        let card = self.deck[self.next_card];
        self.next_card += 1;
        card
    }

    fn pay(&mut self, seat: Seat, amount: u32) {
        let i = seat.idx();
        debug_assert!(self.stacks[i] >= amount);
        let amount = amount.min(self.stacks[i]);
        self.stacks[i] -= amount;
        self.committed[i] += amount;
        self.street_bets[i] += amount;
    }

    fn betting_open(&self) -> bool {
        self.stacks[0] > 0 && self.stacks[1] > 0
    }

    /// Options for the seat that is to act (all `false`/0 otherwise).
    pub fn legal(&self) -> Legal {
        let State::ToAct(seat) = self.state else {
            return Legal::default();
        };
        let opp = seat.other();
        let to_call = self.street_bets[opp.idx()] - self.street_bets[seat.idx()];
        let raise_cost = to_call + 1;
        Legal {
            to_call,
            // Folding when a check is free would only ever be a misclick.
            can_fold: to_call > 0,
            can_raise: self.bet_level < MAX_BETS
                && self.stacks[seat.idx()] >= raise_cost
                && self.stacks[opp.idx()] >= 1,
            raise_cost,
        }
    }

    /// Apply an action for the seat that is to act. An illegal raise is
    /// downgraded to a call and an illegal fold to a check, so the game can
    /// never get stuck.
    pub fn act(&mut self, action: Action) {
        let State::ToAct(seat) = self.state else {
            return;
        };
        let opp = seat.other();
        let legal = self.legal();

        let action = match action {
            Action::Raise if !legal.can_raise => Action::Call,
            Action::Fold if !legal.can_fold => Action::Call,
            a => a,
        };

        match action {
            Action::Fold => {
                self.events.push(Event::Moved {
                    seat,
                    mv: Move::Fold,
                });
                self.folded = Some(seat);
                self.finish(Some(opp), true, None, None);
                return;
            }
            Action::Call => {
                self.pay(seat, legal.to_call);
                let mv = if legal.to_call > 0 {
                    Move::Call
                } else {
                    Move::Check
                };
                self.events.push(Event::Moved { seat, mv });
            }
            Action::Raise => {
                self.pay(seat, legal.raise_cost);
                self.bet_level += 1;
                let mv = if legal.to_call > 0 {
                    Move::Raise
                } else {
                    Move::Bet
                };
                self.events.push(Event::Moved { seat, mv });
            }
        }
        self.acted[seat.idx()] = true;

        let matched = self.street_bets[0] == self.street_bets[1];
        if matched && (self.acted[opp.idx()] || !self.betting_open()) {
            // Round complete. Chips stay in `committed`; clear the street.
            self.street_bets = [0; 2];
            self.state = State::Dealing;
        } else {
            self.state = State::ToAct(opp);
        }
        self.seq += 1;
    }

    /// Deal the next street, or settle the showdown after the river.
    pub fn advance(&mut self) {
        if self.state != State::Dealing {
            return;
        }
        let (next, count) = match self.street {
            Street::PreFlop => (Street::Flop, 3),
            Street::Flop => (Street::Turn, 1),
            Street::Turn => (Street::River, 1),
            Street::River => {
                self.showdown();
                return;
            }
        };

        let burn = self.draw();
        self.burned.push(burn);
        let mut dealt = Vec::with_capacity(count);
        for _ in 0..count {
            let card = self.draw();
            self.board.push(card);
            dealt.push(card);
        }
        self.street = next;
        self.events.push(Event::Dealt {
            street: next,
            cards: dealt,
        });

        self.bet_level = 0;
        self.acted = [false; 2];
        self.state = if self.betting_open() {
            State::ToAct(self.blind_seat())
        } else {
            // Somebody is all-in: just run the board out.
            State::Dealing
        };
        self.seq += 1;
    }

    fn showdown(&mut self) {
        let mut bests = Vec::with_capacity(2);
        for seat in 0..2 {
            let mut cards = self.hole[seat].to_vec();
            cards.extend_from_slice(&self.board);
            bests.push(best_of(&cards));
        }
        let values = [bests[0].value, bests[1].value];
        let fives = [bests[0].five, bests[1].five];
        self.events.push(Event::Showdown {
            hole: self.hole,
            values,
        });

        let winner = match values[0].cmp(&values[1]) {
            std::cmp::Ordering::Greater => Some(Seat::Human),
            std::cmp::Ordering::Less => Some(Seat::Bot),
            std::cmp::Ordering::Equal => None,
        };
        self.finish(winner, false, Some(values), Some(fives));
    }

    fn finish(
        &mut self,
        winner: Option<Seat>,
        by_fold: bool,
        values: Option<[HandValue; 2]>,
        best_five: Option<[[Card; 5]; 2]>,
    ) {
        let pot = self.pot();
        match winner {
            Some(seat) => {
                self.stacks[seat.idx()] += pot;
                self.hands_won[seat.idx()] += 1;
            }
            None => {
                // Each player simply takes their own chips back.
                self.stacks[0] += self.committed[0];
                self.stacks[1] += self.committed[1];
            }
        }
        self.street_bets = [0; 2];
        self.result = Some(HandResult {
            winner,
            by_fold,
            pot,
            values,
            best_five,
        });
        self.state = State::HandOver;
        self.events.push(Event::Won { winner, pot });
        if let Some(champ) = self.match_winner() {
            self.events.push(Event::MatchOver { winner: champ });
        }
        self.seq += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Plays whole matches with random legal-ish actions and checks the
    /// invariants that must always hold.
    #[test]
    fn random_matches_keep_invariants() {
        let mut dice = Rng::new(12345);
        for seed in 0..300u64 {
            let start = [10, 20, 30, 1, 2, 3][seed as usize % 6];
            let mut game = Game::new(start, seed);
            let total = 2 * start;
            let mut steps = 0;
            loop {
                steps += 1;
                assert!(steps < 200_000, "match never ended (seed {seed})");
                // While a hand is running the pot sits outside the stacks;
                // once it is over the pot has already been paid out.
                let on_table = if game.state == State::HandOver {
                    0
                } else {
                    game.pot()
                };
                assert_eq!(
                    game.stacks[0] + game.stacks[1] + on_table,
                    total,
                    "chips were created or destroyed (seed {seed})"
                );
                match game.state {
                    State::ToAct(seat) => {
                        let legal = game.legal();
                        assert!(legal.to_call <= 1);
                        assert!(game.stacks[seat.idx()] >= legal.to_call);
                        if legal.can_raise {
                            assert!(game.stacks[seat.idx()] >= legal.raise_cost);
                        }
                        let action = match dice.below(10) {
                            0 => Action::Fold,
                            1..=5 => Action::Call,
                            _ => Action::Raise,
                        };
                        game.act(action);
                        assert!(game.bet_level <= MAX_BETS);
                    }
                    State::Dealing => game.advance(),
                    State::HandOver => {
                        let result = game.result.as_ref().expect("result after a hand");
                        assert_eq!(game.pot(), result.pot);
                        if !result.by_fold {
                            assert_eq!(game.board.len(), 5);
                            assert_eq!(game.burned.len(), 3);
                            assert_eq!(game.next_card, CARDS_IN_PLAY);
                        }
                        if game.match_winner().is_some() {
                            assert!(game.stacks.contains(&0));
                            assert!(game.stacks.contains(&total));
                            break;
                        }
                        game.start_hand();
                    }
                }
            }
        }
    }

    #[test]
    fn decoys_never_collide_with_anything_visible() {
        for seed in 0..500u64 {
            let game = Game::new(20, seed);
            let bot = game.hole[Seat::Bot.idx()];

            // The real hand is among the candidates, at the recorded slot.
            let real: HashSet<Card> = game.candidates[game.real_candidate]
                .iter()
                .copied()
                .collect();
            assert_eq!(real, bot.iter().copied().collect::<HashSet<_>>());

            // All eight candidate cards are distinct.
            let all: HashSet<Card> = game.candidates.iter().flatten().copied().collect();
            assert_eq!(all.len(), 8);

            // Decoys avoid every card that is or will be in play, plus the
            // bottom cards a peek can show.
            let mut forbidden: HashSet<Card> = game.deck[..CARDS_IN_PLAY].iter().copied().collect();
            forbidden.extend(game.bottom_cards(BOTTOM_PEEK).iter().copied());
            for (slot, hand) in game.candidates.iter().enumerate() {
                if slot == game.real_candidate {
                    continue;
                }
                for card in hand {
                    assert!(
                        !forbidden.contains(card),
                        "decoy uses a live card (seed {seed})"
                    );
                }
            }
        }
    }

    #[test]
    fn blind_and_first_action() {
        let game = Game::new(20, 7);
        let blind = game.blind_seat();
        assert_eq!(game.stacks[blind.idx()], 19);
        assert_eq!(game.stacks[game.dealer.idx()], 20);
        assert_eq!(game.pot(), 1);
        assert_eq!(game.state, State::ToAct(game.dealer));
        let legal = game.legal();
        assert_eq!(legal.to_call, 1);
        assert_eq!(legal.raise_cost, 2);
        assert!(legal.can_fold && legal.can_raise);
    }

    #[test]
    fn blind_gets_an_option_after_a_limp() {
        let mut game = Game::new(20, 7);
        let blind = game.blind_seat();
        game.act(Action::Call);
        assert_eq!(game.state, State::ToAct(blind));
        let legal = game.legal();
        assert_eq!(legal.to_call, 0);
        assert!(!legal.can_fold);
        assert_eq!(legal.raise_cost, 1);
        game.act(Action::Call); // check
        assert_eq!(game.state, State::Dealing);
        game.advance();
        assert_eq!(game.board.len(), 3);
        assert_eq!(game.burned.len(), 1);
        // After the flop the blind acts first.
        assert_eq!(game.state, State::ToAct(blind));
    }

    #[test]
    fn raises_are_capped() {
        let mut game = Game::new(20, 7);
        // Blind is bet #1; three raises reach the cap.
        game.act(Action::Raise);
        game.act(Action::Raise);
        game.act(Action::Raise);
        assert_eq!(game.bet_level, MAX_BETS);
        assert!(!game.legal().can_raise);
        game.act(Action::Call);
        assert_eq!(game.state, State::Dealing);
        assert_eq!(game.pot(), 8);
    }

    #[test]
    fn fold_awards_the_pot() {
        let mut game = Game::new(20, 7);
        let dealer = game.dealer;
        game.act(Action::Raise); // dealer puts in 2
        game.act(Action::Fold); // blind gives up its 1
        assert_eq!(game.state, State::HandOver);
        assert_eq!(game.stacks[dealer.idx()], 21);
        assert_eq!(game.stacks[dealer.other().idx()], 19);
        assert!(game.result.as_ref().unwrap().by_fold);
    }

    #[test]
    fn all_in_runs_the_board_out() {
        let mut game = Game::new(1, 3);
        // The blind is all-in just by posting; the dealer can only call or fold.
        assert!(!game.legal().can_raise);
        game.act(Action::Call);
        let mut guard = 0;
        while game.state == State::Dealing {
            game.advance();
            guard += 1;
            assert!(guard < 10);
        }
        assert_eq!(game.state, State::HandOver);
        assert_eq!(game.board.len(), 5);
        assert_eq!(game.stacks[0] + game.stacks[1], 2);
    }
}
