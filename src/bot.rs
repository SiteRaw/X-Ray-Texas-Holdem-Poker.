//! The opponent. It only ever looks at what a real player could see: its own
//! two cards, the board, the pot and the betting. None of the peek options
//! are available to it.
//!
//! The approach is intentionally simple: estimate the chance of winning
//! against a random hand by dealing out random run-outs, shade that estimate
//! down when the opponent has shown aggression, and then pick an action with
//! a bit of randomness so it is not perfectly readable.

use crate::cards::{full_deck, Card};
use crate::eval::best_of;
use crate::game::{Action, Game, Seat, Street};
use crate::rng::Rng;

const SAMPLES: usize = 300;

pub fn decide(game: &Game, rng: &mut Rng) -> Action {
    let legal = game.legal();
    let hole = game.hole[Seat::Bot.idx()];
    let equity = equity(&hole, &game.board, rng, SAMPLES);

    let preflop = game.street == Street::PreFlop;
    // Bets/raises made voluntarily on this street (the blind is not one).
    let voluntary = game.bet_level.saturating_sub(preflop as u32) as f32;

    let mut strength = equity + (rng.unit() - 0.5) * 0.08;

    if legal.to_call > 0 {
        // Someone betting into us is usually not holding a random hand, and
        // that matters more the later the street.
        let respect = match game.street {
            Street::PreFlop => 0.03,
            Street::Flop => 0.04,
            Street::Turn => 0.06,
            Street::River => 0.08,
        };
        strength -= respect * voluntary;

        let pot = game.pot() as f32;
        let price = legal.to_call as f32 / (pot + legal.to_call as f32);
        let fold_below = if preflop && voluntary == 0.0 {
            0.36 // just completing the blind: play most hands
        } else {
            (price + 0.05).max(0.24)
        };

        if strength < fold_below {
            // Very occasionally turn a hopeless hand into a bluff-raise.
            if legal.can_raise && rng.chance(0.04) {
                return Action::Raise;
            }
            return Action::Fold;
        }
        if legal.can_raise {
            let p = if strength > 0.74 {
                0.85
            } else if strength > 0.62 {
                0.30
            } else {
                0.03
            };
            if rng.chance(p) {
                return Action::Raise;
            }
        }
        Action::Call
    } else {
        if legal.can_raise {
            let p = if strength > 0.62 {
                0.85 // value bet, with the odd slow-play
            } else if strength > 0.50 {
                0.35
            } else if game.street == Street::River {
                0.10
            } else {
                0.15 // bluff
            };
            if rng.chance(p) {
                return Action::Raise;
            }
        }
        Action::Call // check
    }
}

/// Monte Carlo estimate of the probability that `hole` beats one random
/// hand at showdown (ties count as half a win).
pub fn equity(hole: &[Card; 2], board: &[Card], rng: &mut Rng, samples: usize) -> f32 {
    let mut unseen: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !hole.contains(c) && !board.contains(c))
        .collect();
    let board_missing = 5 - board.len();
    let need = 2 + board_missing;

    let mut mine = [hole[0]; 7];
    let mut theirs = [hole[0]; 7];
    mine[1] = hole[1];
    for (i, card) in board.iter().enumerate() {
        mine[2 + i] = *card;
        theirs[2 + i] = *card;
    }

    let mut score = 0.0f32;
    for _ in 0..samples {
        // Partial Fisher–Yates: only the first `need` positions matter.
        for i in 0..need {
            let j = i + rng.below(unseen.len() - i);
            unseen.swap(i, j);
        }
        theirs[0] = unseen[0];
        theirs[1] = unseen[1];
        for k in 0..board_missing {
            let card = unseen[2 + k];
            mine[2 + board.len() + k] = card;
            theirs[2 + board.len() + k] = card;
        }
        let a = best_of(&mine).value;
        let b = best_of(&theirs).value;
        score += match a.cmp(&b) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        };
    }
    score / samples as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Suit::*;
    use crate::game::State;

    #[test]
    fn aces_are_better_than_seven_deuce() {
        let mut rng = Rng::new(1);
        let aces = equity(
            &[Card::new(14, Spades), Card::new(14, Hearts)],
            &[],
            &mut rng,
            2000,
        );
        let trash = equity(
            &[Card::new(7, Spades), Card::new(2, Hearts)],
            &[],
            &mut rng,
            2000,
        );
        assert!(aces > 0.80 && aces < 0.90, "AA equity was {aces}");
        assert!(trash > 0.28 && trash < 0.42, "72o equity was {trash}");
    }

    #[test]
    fn made_nuts_have_full_equity() {
        let mut rng = Rng::new(2);
        let board = [
            Card::new(10, Spades),
            Card::new(11, Spades),
            Card::new(12, Spades),
            Card::new(2, Hearts),
            Card::new(3, Diamonds),
        ];
        let e = equity(
            &[Card::new(14, Spades), Card::new(13, Spades)],
            &board,
            &mut rng,
            500,
        );
        assert_eq!(e, 1.0);
    }

    /// Bot against bot: every decision must be legal and matches must end.
    #[test]
    fn bot_plays_whole_matches() {
        let mut rng = Rng::new(99);
        for seed in 0..20u64 {
            let mut game = Game::new(10, seed);
            let mut steps = 0;
            while game.match_winner().is_none() {
                steps += 1;
                assert!(steps < 100_000);
                match game.state {
                    State::ToAct(_) => {
                        // `decide` always reads the Bot seat's cards, which is
                        // fine here: we only care that play proceeds legally.
                        let legal = game.legal();
                        let action = decide(&game, &mut rng);
                        if action == Action::Raise {
                            assert!(legal.can_raise);
                        }
                        if action == Action::Fold {
                            assert!(legal.can_fold);
                        }
                        game.act(action);
                    }
                    State::Dealing => game.advance(),
                    State::HandOver => game.start_hand(),
                }
            }
        }
    }
}
