//! Chance of winning the hand at showdown, from the human player's seat.
//!
//! The calculation uses exactly what the player can see and nothing else.
//! That is their own two cards and the board, plus whatever the peek options
//! currently expose:
//!
//! * bottom-of-deck cards and burned cards are out of play, so the opponent
//!   cannot hold them and they cannot come on the board;
//! * an exposed opponent card pins down half of the opponent's hand;
//! * the "four possible hands" narrow the opponent down to those hands, and
//!   together with an exposed card to exactly one.
//!
//! Where the number of possible outcomes is manageable every one of them is
//! counted, so "cannot lose" and "cannot win" are statements of fact. Before
//! the flop there are far too many, and a large random sample is used.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::cards::{full_deck, Card};
use crate::eval::score7;
use crate::rng::Rng;

/// Count every outcome when there are at most this many.
const EXACT_LIMIT: u64 = 3_000_000;
/// Otherwise deal this many random ones.
const SAMPLES: u64 = 300_000;

/// Everything the player can see that bears on the outcome.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sight {
    pub hole: [Card; 2],
    pub board: Vec<Card>,
    /// Exposed cards that are out of play (bottom of the deck, burns).
    pub dead: Vec<Card>,
    /// One exposed card from the opponent's hand.
    pub opp_card: Option<Card>,
    /// Hands the opponent is known to be holding one of. Empty when the
    /// player has no such list.
    pub candidates: Vec<[Card; 2]>,
}

impl Sight {
    /// Identifies this exact state of knowledge, for caching.
    pub fn key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Odds {
    pub win: f64,
    pub tie: f64,
    pub lose: f64,
    /// `true` when every possible outcome was counted, `false` when the
    /// figures are an estimate from random sampling.
    pub exact: bool,
    /// How many different hands the opponent could still be holding.
    pub opp_hands: usize,
}

impl Odds {
    /// No possible outcome loses.
    pub fn cannot_lose(&self) -> bool {
        self.exact && self.lose == 0.0
    }
    /// No possible outcome wins.
    pub fn cannot_win(&self) -> bool {
        self.exact && self.win == 0.0
    }
}

fn bit(card: Card) -> u64 {
    1u64 << (card.suit as u64 * 13 + (card.rank as u64 - 2))
}

fn mask(cards: &[Card]) -> u64 {
    cards.iter().fold(0, |m, &c| m | bit(c))
}

fn choose(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    (0..k as u64).fold(1u64, |acc, i| acc * (n as u64 - i) / (i + 1))
}

#[derive(Default)]
struct Tally {
    win: u64,
    tie: u64,
    lose: u64,
}

impl Tally {
    fn add(&mut self, mine: u32, theirs: u32) {
        match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => self.win += 1,
            std::cmp::Ordering::Equal => self.tie += 1,
            std::cmp::Ordering::Less => self.lose += 1,
        }
    }
}

pub fn compute(sight: &Sight) -> Odds {
    let need = 5usize.saturating_sub(sight.board.len());
    let mut visible = mask(&sight.hole) | mask(&sight.board) | mask(&sight.dead);

    // Which hands can the opponent have, and which cards can still come?
    let consistent: Vec<[Card; 2]> = sight
        .candidates
        .iter()
        .filter(|hand| mask(&hand[..]) & visible == 0)
        .filter(|hand| match sight.opp_card {
            Some(exposed) => hand.contains(&exposed),
            None => true,
        })
        .copied()
        .collect();

    let (hands, pool): (Vec<[Card; 2]>, Vec<Card>) = if !consistent.is_empty() {
        // The decoys are cards that stay in the deck all hand, and the real
        // hand is in the opponent's hand, so none of the listed cards can
        // appear on the board.
        let listed = sight
            .candidates
            .iter()
            .fold(0, |m, hand| m | mask(&hand[..]));
        let pool = full_deck()
            .into_iter()
            .filter(|&c| bit(c) & (visible | listed) == 0)
            .collect();
        (consistent, pool)
    } else {
        if let Some(c) = sight.opp_card {
            visible |= bit(c);
        }
        let pool: Vec<Card> = full_deck()
            .into_iter()
            .filter(|&c| bit(c) & visible == 0)
            .collect();
        let hands = match sight.opp_card {
            Some(known) => pool.iter().map(|&other| [known, other]).collect(),
            None => {
                let mut pairs = Vec::with_capacity(pool.len() * pool.len() / 2);
                for i in 0..pool.len() {
                    for j in i + 1..pool.len() {
                        pairs.push([pool[i], pool[j]]);
                    }
                }
                pairs
            }
        };
        (hands, pool)
    };
    let hand_masks: Vec<u64> = hands.iter().map(|h| mask(&h[..])).collect();

    // Seven-card buffers: two hole cards, then the board, then the run-out.
    let filled = 2 + sight.board.len();
    let mut mine = [sight.hole[0]; 7];
    let mut theirs = [sight.hole[0]; 7];
    mine[1] = sight.hole[1];
    for (i, &card) in sight.board.iter().enumerate() {
        mine[2 + i] = card;
        theirs[2 + i] = card;
    }

    let mut tally = Tally::default();
    let scenarios = choose(pool.len(), need).saturating_mul(hands.len() as u64);
    let exact = scenarios <= EXACT_LIMIT;

    if exact {
        // Walk through every combination of `need` cards from the pool.
        let mut idx = [0usize; 5];
        for (i, slot) in idx.iter_mut().enumerate().take(need) {
            *slot = i;
        }
        let mut more = pool.len() >= need;
        while more {
            let mut run_mask = 0u64;
            for k in 0..need {
                let card = pool[idx[k]];
                mine[filled + k] = card;
                theirs[filled + k] = card;
                run_mask |= bit(card);
            }
            let my_score = score7(&mine);
            for (hand, &hand_mask) in hands.iter().zip(&hand_masks) {
                if hand_mask & run_mask != 0 {
                    continue; // the opponent can't hold a card that is on the board
                }
                theirs[0] = hand[0];
                theirs[1] = hand[1];
                tally.add(my_score, score7(&theirs));
            }

            // Advance to the next combination (lexicographic order).
            more = false;
            for k in (0..need).rev() {
                if idx[k] < pool.len() - (need - k) {
                    idx[k] += 1;
                    for j in k + 1..need {
                        idx[j] = idx[j - 1] + 1;
                    }
                    more = true;
                    break;
                }
            }
        }
    } else {
        // Seeded from the situation itself, so the same situation always
        // shows the same estimate.
        let mut rng = Rng::new(sight.key());
        for _ in 0..SAMPLES {
            let h = rng.below(hands.len());
            theirs[0] = hands[h][0];
            theirs[1] = hands[h][1];
            let mut used = hand_masks[h];
            let mut k = 0;
            while k < need {
                let card = pool[rng.below(pool.len())];
                if used & bit(card) != 0 {
                    continue;
                }
                used |= bit(card);
                mine[filled + k] = card;
                theirs[filled + k] = card;
                k += 1;
            }
            tally.add(score7(&mine), score7(&theirs));
        }
    }

    let total = (tally.win + tally.tie + tally.lose).max(1) as f64;
    Odds {
        win: tally.win as f64 / total,
        tie: tally.tie as f64 / total,
        lose: tally.lose as f64 / total,
        exact,
        opp_hands: hands.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Suit::{self, *};
    use crate::eval::best_of;

    fn c(rank: u8, suit: Suit) -> Card {
        Card::new(rank, suit)
    }

    fn sight(hole: [Card; 2], board: &[Card]) -> Sight {
        Sight {
            hole,
            board: board.to_vec(),
            dead: vec![],
            opp_card: None,
            candidates: vec![],
        }
    }

    fn sums_to_one(o: &Odds) {
        assert!((o.win + o.tie + o.lose - 1.0).abs() < 1e-9);
    }

    #[test]
    fn preflop_is_an_estimate_near_the_known_figures() {
        let aces = compute(&sight([c(14, Spades), c(14, Hearts)], &[]));
        sums_to_one(&aces);
        assert!(!aces.exact);
        assert_eq!(aces.opp_hands, 1225);
        // Pocket aces win about 84.9% and tie about 0.5% against a random hand.
        assert!((aces.win - 0.849).abs() < 0.006, "AA win {}", aces.win);
        let trash = compute(&sight([c(7, Spades), c(2, Hearts)], &[]));
        // Seven-deuce offsuit wins a little under a third of the time.
        assert!((0.31..0.33).contains(&trash.win), "72o win {}", trash.win);
        // Same situation, same estimate.
        assert_eq!(aces, compute(&sight([c(14, Spades), c(14, Hearts)], &[])));
    }

    #[test]
    fn flop_is_counted_exactly_and_matches_brute_force() {
        let hole = [c(14, Hearts), c(13, Hearts)];
        let board = [c(12, Hearts), c(7, Clubs), c(2, Hearts)];
        let opp = [c(7, Spades), c(7, Diamonds)];
        let mut s = sight(hole, &board);
        s.candidates = vec![opp];
        let o = compute(&s);
        sums_to_one(&o);
        assert!(o.exact);
        assert_eq!(o.opp_hands, 1);

        // Independent count with the slow evaluator.
        let rest: Vec<Card> = full_deck()
            .into_iter()
            .filter(|x| !hole.contains(x) && !board.contains(x) && !opp.contains(x))
            .collect();
        let (mut win, mut tie, mut total) = (0u32, 0u32, 0u32);
        for i in 0..rest.len() {
            for j in i + 1..rest.len() {
                let mut a = hole.to_vec();
                a.extend_from_slice(&board);
                a.extend([rest[i], rest[j]]);
                let mut b = opp.to_vec();
                b.extend_from_slice(&board);
                b.extend([rest[i], rest[j]]);
                match best_of(&a).value.cmp(&best_of(&b).value) {
                    std::cmp::Ordering::Greater => win += 1,
                    std::cmp::Ordering::Equal => tie += 1,
                    _ => {}
                }
                total += 1;
            }
        }
        assert_eq!(total, 990);
        assert!((o.win - win as f64 / total as f64).abs() < 1e-12);
        assert!((o.tie - tie as f64 / total as f64).abs() < 1e-12);
    }

    #[test]
    fn unknown_opponent_on_the_flop_is_still_exact() {
        let o = compute(&sight(
            [c(14, Spades), c(13, Spades)],
            &[c(12, Spades), c(11, Spades), c(10, Spades)],
        ));
        // A flopped royal flush: 1081 opponent hands, none of which can win.
        assert!(o.exact);
        assert_eq!(o.opp_hands, 1081);
        assert!(o.cannot_lose());
        assert_eq!(o.win, 1.0);
    }

    #[test]
    fn the_board_can_make_winning_impossible() {
        // Royal flush on the board: everyone plays it, so it is a sure split.
        let board = [
            c(14, Spades),
            c(13, Spades),
            c(12, Spades),
            c(11, Spades),
            c(10, Spades),
        ];
        let o = compute(&sight([c(2, Hearts), c(3, Clubs)], &board));
        assert!(o.exact && o.cannot_win() && o.cannot_lose());
        assert_eq!(o.tie, 1.0);

        // Drawing dead: the opponent's exposed hand already beats anything
        // the last card can give us.
        let mut s = sight(
            [c(2, Hearts), c(3, Clubs)],
            &[c(13, Spades), c(13, Hearts), c(9, Diamonds), c(9, Clubs)],
        );
        s.candidates = vec![[c(13, Diamonds), c(13, Clubs)]];
        let o = compute(&s);
        assert!(o.cannot_win());
        assert_eq!(o.lose, 1.0);
    }

    #[test]
    fn dead_cards_change_the_odds() {
        // Four to a flush with one card to come, against a pair of kings:
        // only a heart saves us, and every heart does.
        let mut s = sight(
            [c(7, Hearts), c(4, Hearts)],
            &[c(9, Hearts), c(8, Hearts), c(13, Clubs), c(2, Spades)],
        );
        s.candidates = vec![[c(13, Diamonds), c(12, Spades)]];
        let open = compute(&s);
        // 9 hearts left among 44 unseen cards.
        assert!((open.win - 9.0 / 44.0).abs() < 1e-12);

        // Seeing five dead cards, three of them hearts, leaves 6 outs in 39.
        s.dead = vec![
            c(2, Hearts),
            c(3, Hearts),
            c(5, Hearts),
            c(6, Clubs),
            c(7, Clubs),
        ];
        let fewer = compute(&s);
        assert!((fewer.win - 6.0 / 39.0).abs() < 1e-12);

        // Dead cards that are blanks help instead: 9 outs in 39.
        s.dead = vec![
            c(3, Clubs),
            c(4, Clubs),
            c(5, Clubs),
            c(6, Clubs),
            c(7, Clubs),
        ];
        let more = compute(&s);
        assert!((more.win - 9.0 / 39.0).abs() < 1e-12);
    }

    #[test]
    fn exposed_card_plus_candidates_pins_the_hand() {
        let board = [c(9, Hearts), c(8, Hearts), c(13, Clubs), c(2, Spades)];
        let real = [c(13, Diamonds), c(6, Spades)];
        let mut s = sight([c(14, Hearts), c(4, Hearts)], &board);
        s.candidates = vec![
            [c(12, Clubs), c(5, Diamonds)],
            real,
            [c(11, Clubs), c(10, Diamonds)],
            [c(7, Clubs), c(3, Diamonds)],
        ];
        let four = compute(&s);
        assert_eq!(four.opp_hands, 4);

        s.opp_card = Some(c(6, Spades));
        let pinned = compute(&s);
        assert_eq!(pinned.opp_hands, 1);
        assert_ne!(four.win, pinned.win);

        // Only the exposed card, no list: the other card could be anything unseen.
        s.candidates.clear();
        let half = compute(&s);
        assert_eq!(half.opp_hands, 52 - 2 - 4 - 1);
    }

    #[test]
    fn river_with_everything_known_is_all_or_nothing() {
        let board = [
            c(9, Hearts),
            c(8, Hearts),
            c(13, Clubs),
            c(2, Spades),
            c(5, Hearts),
        ];
        let mut s = sight([c(14, Hearts), c(4, Hearts)], &board);
        s.candidates = vec![[c(13, Diamonds), c(13, Spades)]];
        s.opp_card = Some(c(13, Spades));
        let o = compute(&s);
        assert!(o.exact && o.cannot_lose());
        assert_eq!(o.win, 1.0);
    }
}
