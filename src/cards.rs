//! Cards and the 52-card deck.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

    pub fn symbol(self) -> char {
        match self {
            Suit::Clubs => '♣',
            Suit::Diamonds => '♦',
            Suit::Hearts => '♥',
            Suit::Spades => '♠',
        }
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Diamonds | Suit::Hearts)
    }
}

/// A playing card. `rank` runs from 2 to 14, where 11–14 are J, Q, K, A.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Card {
    pub rank: u8,
    pub suit: Suit,
}

impl Card {
    pub fn new(rank: u8, suit: Suit) -> Self {
        debug_assert!((2..=14).contains(&rank));
        Card { rank, suit }
    }

    /// Short rank as printed on the card: "A", "K", "10", "7"…
    pub fn rank_str(self) -> &'static str {
        rank_str(self.rank)
    }

    /// e.g. "A♠"
    pub fn label(self) -> String {
        format!("{}{}", self.rank_str(), self.suit.symbol())
    }
}

pub fn rank_str(rank: u8) -> &'static str {
    match rank {
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        10 => "10",
        11 => "J",
        12 => "Q",
        13 => "K",
        14 => "A",
        _ => "?",
    }
}

pub fn rank_name(rank: u8) -> &'static str {
    match rank {
        2 => "Two",
        3 => "Three",
        4 => "Four",
        5 => "Five",
        6 => "Six",
        7 => "Seven",
        8 => "Eight",
        9 => "Nine",
        10 => "Ten",
        11 => "Jack",
        12 => "Queen",
        13 => "King",
        14 => "Ace",
        _ => "?",
    }
}

pub fn rank_plural(rank: u8) -> String {
    match rank {
        6 => "Sixes".to_string(),
        r => format!("{}s", rank_name(r)),
    }
}

/// A fresh, ordered 52-card deck.
pub fn full_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for suit in Suit::ALL {
        for rank in 2..=14 {
            deck.push(Card::new(rank, suit));
        }
    }
    deck
}

/// Space-separated labels, e.g. "A♠ 7♦ 2♣".
pub fn labels(cards: &[Card]) -> String {
    cards
        .iter()
        .map(|c| c.label())
        .collect::<Vec<_>>()
        .join(" ")
}
