//! Poker hand evaluation: best five cards out of five, six or seven.

use crate::cards::{rank_name, rank_plural, rank_str, Card};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Category {
    HighCard,
    Pair,
    TwoPair,
    Trips,
    Straight,
    Flush,
    FullHouse,
    Quads,
    StraightFlush,
}

/// Comparable strength of a five-card hand. The derived ordering compares
/// the category first and then the tie-break ranks from most to least
/// significant, which is exactly how poker hands are ranked.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct HandValue {
    pub category: Category,
    pub ranks: [u8; 5],
}

#[cfg(test)]
impl HandValue {
    /// The value squeezed into one integer that sorts the same way. Matches
    /// what [`score7`] returns.
    pub fn packed(&self) -> u32 {
        pack(self.category, self.ranks)
    }
}

fn pack(category: Category, r: [u8; 5]) -> u32 {
    (category as u32) << 20
        | (r[0] as u32) << 16
        | (r[1] as u32) << 12
        | (r[2] as u32) << 8
        | (r[3] as u32) << 4
        | r[4] as u32
}

#[derive(Clone, Copy, Debug)]
pub struct Best {
    pub value: HandValue,
    pub five: [Card; 5],
}

pub fn eval5(cards: &[Card; 5]) -> HandValue {
    let mut ranks = [0u8; 5];
    for (slot, card) in ranks.iter_mut().zip(cards.iter()) {
        *slot = card.rank;
    }
    ranks.sort_unstable_by(|a, b| b.cmp(a));

    let flush = cards.iter().all(|c| c.suit == cards[0].suit);

    // Group equal ranks as (count, rank), biggest group first.
    let mut groups = [(0u8, 0u8); 5];
    let mut n = 0;
    for &r in &ranks {
        if n > 0 && groups[n - 1].1 == r {
            groups[n - 1].0 += 1;
        } else {
            groups[n] = (1, r);
            n += 1;
        }
    }
    groups[..n].sort_unstable_by(|a, b| b.cmp(a));

    let straight_high = if n == 5 {
        if ranks[0] - ranks[4] == 4 {
            Some(ranks[0])
        } else if ranks == [14, 5, 4, 3, 2] {
            Some(5) // the wheel: A-2-3-4-5 plays as five-high
        } else {
            None
        }
    } else {
        None
    };

    let mut tie = [0u8; 5];
    let category = if let Some(high) = straight_high {
        tie[0] = high;
        if flush {
            Category::StraightFlush
        } else {
            Category::Straight
        }
    } else {
        for i in 0..n {
            tie[i] = groups[i].1;
        }
        match (groups[0].0, groups[1].0) {
            (4, _) => Category::Quads,
            (3, 2) => Category::FullHouse,
            _ if flush => Category::Flush,
            (3, _) => Category::Trips,
            (2, 2) => Category::TwoPair,
            (2, _) => Category::Pair,
            _ => Category::HighCard,
        }
    };

    HandValue {
        category,
        ranks: tie,
    }
}

/// Best five-card hand that can be made from `cards` (5 to 7 cards).
pub fn best_of(cards: &[Card]) -> Best {
    let n = cards.len();
    assert!((5..=7).contains(&n), "need 5 to 7 cards, got {n}");

    let mut best: Option<Best> = None;
    for mask in 0u32..(1 << n) {
        if mask.count_ones() != 5 {
            continue;
        }
        let mut five = [cards[0]; 5];
        let mut k = 0;
        for (i, card) in cards.iter().enumerate() {
            if mask & (1 << i) != 0 {
                five[k] = *card;
                k += 1;
            }
        }
        let value = eval5(&five);
        match best {
            Some(b) if value <= b.value => {}
            _ => best = Some(Best { value, five }),
        }
    }
    best.expect("at least one combination")
}

/// Highest card of a straight within a set of ranks (bit `r` set means rank
/// `r` is present), counting the ace low for A-2-3-4-5.
fn straight_high(ranks: u16) -> Option<u8> {
    let with_low_ace = ranks | ((ranks >> 14) & 1) << 1;
    (5..=14u8)
        .rev()
        .find(|&high| (with_low_ace >> (high - 4)) & 0b11111 == 0b11111)
}

/// The `n` highest ranks present in a bit set, highest first.
fn top_ranks(ranks: u16, n: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut k = 0;
    for rank in (2..=14u8).rev() {
        if k == n {
            break;
        }
        if ranks & (1 << rank) != 0 {
            out[k] = rank;
            k += 1;
        }
    }
    out
}

/// Strength of the best five-card hand among 5 to 7 cards, as an integer
/// where bigger is better. It agrees exactly with `best_of(..).value.packed()`
/// but works directly on rank and suit counts instead of trying all 21
/// five-card subsets, which makes it fast enough to enumerate a million
/// possible outcomes for the odds display.
pub fn score7(cards: &[Card]) -> u32 {
    let mut count = [0u8; 15];
    let mut suit_ranks = [0u16; 4];
    let mut suit_count = [0u8; 4];
    let mut present = 0u16;
    for card in cards {
        let bit = 1u16 << card.rank;
        let suit = card.suit as usize;
        count[card.rank as usize] += 1;
        suit_ranks[suit] |= bit;
        suit_count[suit] += 1;
        present |= bit;
    }

    let mut flush = None;
    for suit in 0..4 {
        if suit_count[suit] >= 5 {
            if let Some(high) = straight_high(suit_ranks[suit]) {
                return pack(Category::StraightFlush, [high, 0, 0, 0, 0]);
            }
            flush = Some(top_ranks(suit_ranks[suit], 5));
        }
    }

    // Ranks that appear four, three and two times, highest first.
    let mut quad = 0u8;
    let mut trips = [0u8; 2];
    let mut pairs = [0u8; 3];
    let (mut nt, mut np) = (0, 0);
    for rank in (2..=14u8).rev() {
        match count[rank as usize] {
            4 => quad = rank,
            3 => {
                trips[nt] = rank;
                nt += 1;
            }
            2 => {
                pairs[np] = rank;
                np += 1;
            }
            _ => {}
        }
    }
    let without = |a: u8, b: u8| present & !(1u16 << a) & !(1u16 << b);

    if quad > 0 {
        let kicker = top_ranks(without(quad, quad), 1)[0];
        return pack(Category::Quads, [quad, kicker, 0, 0, 0]);
    }
    if trips[0] > 0 && (trips[1] > 0 || pairs[0] > 0) {
        return pack(
            Category::FullHouse,
            [trips[0], trips[1].max(pairs[0]), 0, 0, 0],
        );
    }
    if let Some(ranks) = flush {
        return pack(Category::Flush, ranks);
    }
    if let Some(high) = straight_high(present) {
        return pack(Category::Straight, [high, 0, 0, 0, 0]);
    }
    if trips[0] > 0 {
        let k = top_ranks(without(trips[0], trips[0]), 2);
        return pack(Category::Trips, [trips[0], k[0], k[1], 0, 0]);
    }
    if pairs[1] > 0 {
        let kicker = top_ranks(without(pairs[0], pairs[1]), 1)[0];
        return pack(Category::TwoPair, [pairs[0], pairs[1], kicker, 0, 0]);
    }
    if pairs[0] > 0 {
        let k = top_ranks(without(pairs[0], pairs[0]), 3);
        return pack(Category::Pair, [pairs[0], k[0], k[1], k[2], 0]);
    }
    pack(Category::HighCard, top_ranks(present, 5))
}

/// Human-readable name, e.g. "Two pair, Aces and Sevens".
pub fn describe(v: &HandValue) -> String {
    let r = v.ranks;
    match v.category {
        Category::HighCard => format!("{} high", rank_name(r[0])),
        Category::Pair => format!("Pair of {}", rank_plural(r[0])),
        Category::TwoPair => format!("Two pair, {} and {}", rank_plural(r[0]), rank_plural(r[1])),
        Category::Trips => format!("Three of a kind, {}", rank_plural(r[0])),
        Category::Straight => format!("Straight, {} high", rank_name(r[0])),
        Category::Flush => format!("Flush, {} high", rank_name(r[0])),
        Category::FullHouse => {
            format!(
                "Full house, {} over {}",
                rank_plural(r[0]),
                rank_plural(r[1])
            )
        }
        Category::Quads => format!("Four of a kind, {}", rank_plural(r[0])),
        Category::StraightFlush => {
            if r[0] == 14 {
                "Royal flush".to_string()
            } else {
                format!("Straight flush, {} high", rank_name(r[0]))
            }
        }
    }
}

/// Describes two showdown hands side by side. When both have the same
/// named hand but one still wins, the deciding kicker is spelled out, so
/// the result never reads "Pair of Fives beats Pair of Fives".
pub fn describe_versus(a: &HandValue, b: &HandValue) -> (String, String) {
    let (da, db) = (describe(a), describe(b));
    if da != db || a == b {
        return (da, db);
    }
    match (0..5).find(|&i| a.ranks[i] != b.ranks[i]) {
        Some(i) => (
            format!("{da}, {} kicker", rank_name(a.ranks[i])),
            format!("{db}, {} kicker", rank_name(b.ranks[i])),
        ),
        None => (da, db),
    }
}

/// Describes whatever a player currently holds: just the two hole cards
/// before the flop, or the best made hand once there is a board.
pub fn describe_holding(hole: &[Card; 2], board: &[Card]) -> String {
    if board.len() < 3 {
        let (a, b) = if hole[0].rank >= hole[1].rank {
            (hole[0], hole[1])
        } else {
            (hole[1], hole[0])
        };
        if a.rank == b.rank {
            format!("Pocket {}", rank_plural(a.rank))
        } else {
            let suited = if a.suit == b.suit {
                "suited"
            } else {
                "offsuit"
            };
            format!("{}-{} {}", rank_str(a.rank), rank_str(b.rank), suited)
        }
    } else {
        let mut all = hole.to_vec();
        all.extend_from_slice(board);
        describe(&best_of(&all).value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Suit::{self, *};

    fn c(rank: u8, suit: Suit) -> Card {
        Card::new(rank, suit)
    }

    fn cat(cards: [Card; 5]) -> Category {
        eval5(&cards).category
    }

    #[test]
    fn categories() {
        assert_eq!(
            cat([
                c(14, Spades),
                c(13, Spades),
                c(12, Spades),
                c(11, Spades),
                c(10, Spades)
            ]),
            Category::StraightFlush
        );
        assert_eq!(
            cat([
                c(9, Hearts),
                c(9, Spades),
                c(9, Clubs),
                c(9, Diamonds),
                c(2, Spades)
            ]),
            Category::Quads
        );
        assert_eq!(
            cat([
                c(9, Hearts),
                c(9, Spades),
                c(9, Clubs),
                c(2, Diamonds),
                c(2, Spades)
            ]),
            Category::FullHouse
        );
        assert_eq!(
            cat([
                c(2, Hearts),
                c(9, Hearts),
                c(11, Hearts),
                c(4, Hearts),
                c(13, Hearts)
            ]),
            Category::Flush
        );
        assert_eq!(
            cat([
                c(5, Hearts),
                c(6, Spades),
                c(7, Clubs),
                c(8, Diamonds),
                c(9, Spades)
            ]),
            Category::Straight
        );
        assert_eq!(
            cat([
                c(5, Hearts),
                c(5, Spades),
                c(5, Clubs),
                c(8, Diamonds),
                c(9, Spades)
            ]),
            Category::Trips
        );
        assert_eq!(
            cat([
                c(5, Hearts),
                c(5, Spades),
                c(8, Clubs),
                c(8, Diamonds),
                c(9, Spades)
            ]),
            Category::TwoPair
        );
        assert_eq!(
            cat([
                c(5, Hearts),
                c(5, Spades),
                c(7, Clubs),
                c(8, Diamonds),
                c(9, Spades)
            ]),
            Category::Pair
        );
        assert_eq!(
            cat([
                c(2, Hearts),
                c(5, Spades),
                c(7, Clubs),
                c(8, Diamonds),
                c(9, Spades)
            ]),
            Category::HighCard
        );
    }

    #[test]
    fn wheel_is_lowest_straight() {
        let wheel = eval5(&[
            c(14, Hearts),
            c(2, Spades),
            c(3, Clubs),
            c(4, Diamonds),
            c(5, Spades),
        ]);
        let six_high = eval5(&[
            c(6, Hearts),
            c(2, Spades),
            c(3, Clubs),
            c(4, Diamonds),
            c(5, Spades),
        ]);
        assert_eq!(wheel.category, Category::Straight);
        assert_eq!(wheel.ranks[0], 5);
        assert!(six_high > wheel);
        // Q-K-A-2-3 does not wrap around.
        assert_eq!(
            cat([
                c(12, Hearts),
                c(13, Spades),
                c(14, Clubs),
                c(2, Diamonds),
                c(3, Spades)
            ]),
            Category::HighCard
        );
    }

    #[test]
    fn kickers_break_ties() {
        let ak = eval5(&[
            c(14, Hearts),
            c(14, Spades),
            c(13, Clubs),
            c(4, Diamonds),
            c(3, Spades),
        ]);
        let aq = eval5(&[
            c(14, Clubs),
            c(14, Diamonds),
            c(12, Clubs),
            c(4, Hearts),
            c(3, Hearts),
        ]);
        assert!(ak > aq);
        let two_pair_hi = eval5(&[
            c(14, Hearts),
            c(14, Spades),
            c(3, Clubs),
            c(3, Diamonds),
            c(2, Spades),
        ]);
        let two_pair_lo = eval5(&[
            c(13, Hearts),
            c(13, Spades),
            c(12, Clubs),
            c(12, Diamonds),
            c(14, Clubs),
        ]);
        assert!(two_pair_hi > two_pair_lo);
        // A full house is ranked by its trips first.
        let threes_full = eval5(&[
            c(3, Hearts),
            c(3, Spades),
            c(3, Clubs),
            c(14, Diamonds),
            c(14, Spades),
        ]);
        let twos_full = eval5(&[
            c(2, Hearts),
            c(2, Spades),
            c(2, Clubs),
            c(14, Hearts),
            c(14, Clubs),
        ]);
        assert!(threes_full > twos_full);
    }

    #[test]
    fn best_of_seven_finds_the_flush() {
        let cards = [
            c(14, Hearts),
            c(14, Spades),
            c(2, Hearts),
            c(9, Hearts),
            c(11, Hearts),
            c(4, Hearts),
            c(14, Clubs),
        ];
        let best = best_of(&cards);
        assert_eq!(best.value.category, Category::Flush);
        assert!(best.five.iter().all(|card| card.suit == Hearts));
    }

    #[test]
    fn best_of_seven_plays_the_board() {
        // Board is a straight; both hole cards are irrelevant.
        let cards = [
            c(2, Hearts),
            c(2, Spades),
            c(5, Hearts),
            c(6, Clubs),
            c(7, Diamonds),
            c(8, Hearts),
            c(9, Clubs),
        ];
        let best = best_of(&cards);
        assert_eq!(best.value.category, Category::Straight);
        assert_eq!(best.value.ranks[0], 9);
    }

    /// The fast evaluator must agree with the try-every-subset one on
    /// every hand, for 5, 6 and 7 cards.
    #[test]
    fn score7_matches_best_of() {
        use crate::cards::full_deck;
        use crate::rng::Rng;
        let mut rng = Rng::new(2024);
        let mut deck = full_deck();
        let mut seen = std::collections::HashSet::new();
        for i in 0..200_000 {
            rng.shuffle(&mut deck);
            let n = 5 + i % 3;
            let slow = best_of(&deck[..n]).value;
            assert_eq!(score7(&deck[..n]), slow.packed(), "cards: {:?}", &deck[..n]);
            seen.insert(slow.category as u8);
        }
        // Plus hand-picked rarities that random deals seldom produce.
        let rare: [[Card; 7]; 4] = [
            // straight flush hidden under an ace-high flush
            [
                c(14, Hearts),
                c(9, Hearts),
                c(8, Hearts),
                c(7, Hearts),
                c(6, Hearts),
                c(5, Hearts),
                c(2, Clubs),
            ],
            // steel wheel
            [
                c(14, Spades),
                c(2, Spades),
                c(3, Spades),
                c(4, Spades),
                c(5, Spades),
                c(13, Hearts),
                c(13, Clubs),
            ],
            // two sets make a full house
            [
                c(9, Hearts),
                c(9, Spades),
                c(9, Clubs),
                c(4, Hearts),
                c(4, Spades),
                c(4, Clubs),
                c(14, Clubs),
            ],
            // quads with a pair on the side: kicker is the ace, not the pair
            [
                c(9, Hearts),
                c(9, Spades),
                c(9, Clubs),
                c(9, Diamonds),
                c(4, Spades),
                c(4, Clubs),
                c(14, Clubs),
            ],
        ];
        for cards in rare {
            assert_eq!(
                score7(&cards),
                best_of(&cards).value.packed(),
                "cards: {cards:?}"
            );
        }
        assert!(
            seen.len() >= 8,
            "random deals should cover nearly every category"
        );
    }

    #[test]
    fn kicker_is_named_when_it_decides() {
        let q = eval5(&[
            c(5, Hearts),
            c(5, Clubs),
            c(12, Clubs),
            c(10, Hearts),
            c(9, Clubs),
        ]);
        let j = eval5(&[
            c(5, Hearts),
            c(5, Clubs),
            c(11, Hearts),
            c(10, Hearts),
            c(9, Clubs),
        ]);
        let (a, b) = describe_versus(&q, &j);
        assert_eq!(a, "Pair of Fives, Queen kicker");
        assert_eq!(b, "Pair of Fives, Jack kicker");
        // Different hands, or a genuine tie, are left alone.
        let aces = eval5(&[
            c(14, Hearts),
            c(14, Clubs),
            c(12, Clubs),
            c(10, Hearts),
            c(9, Clubs),
        ]);
        assert_eq!(describe_versus(&aces, &j).0, "Pair of Aces");
        assert_eq!(describe_versus(&q, &q).0, "Pair of Fives");
    }

    #[test]
    fn descriptions() {
        let v = eval5(&[
            c(14, Hearts),
            c(14, Spades),
            c(6, Clubs),
            c(6, Diamonds),
            c(2, Spades),
        ]);
        assert_eq!(describe(&v), "Two pair, Aces and Sixes");
        assert_eq!(
            describe_holding(&[c(13, Hearts), c(13, Spades)], &[]),
            "Pocket Kings"
        );
        assert_eq!(
            describe_holding(&[c(10, Hearts), c(14, Hearts)], &[]),
            "A-10 suited"
        );
    }
}
