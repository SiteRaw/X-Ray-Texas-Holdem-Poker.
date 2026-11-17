# X-Ray Hold'em

Heads-up Texas Hold'em against a bot, written in Rust with
Macroquad (table, cards, animation) and
Egui (sidebar and buttons).
The twist: six "X-ray" options that let you see things you shouldn't.
Only you get them; the bot always plays blind.

From [SiteRaw](https://www.siteraw.com)

## Run it

```
cargo run --release
```

Needs a recent stable Rust (1.82 or newer). No other setup: the cards are
drawn with shapes and the font is embedded, so there are no asset files to
ship next to the binary. On Linux the usual X11/OpenGL libraries must be
present, which they are on any desktop install.

The window opens at 1000x636 so it fits a 1280x720 laptop screen. The whole
interface (table, sidebar, buttons) scales with the window, so maximise it or
drag it to any size and everything stays visible and in proportion.

`cargo test` runs the rules tests (hand ranking, betting, chip conservation
over thousands of random hands, decoy generation, the odds calculation, bot
sanity).

## Rules

Simplified fixed-limit, measured in points:

- No small blind. One player posts a **1-point blind**; the other is the dealer (D). The roles swap every hand.
- Three buttons: **Fold**, **Call** (1 point), **Raise** (2 points = 1 to call + 1 more).
  When there is nothing to call, the same buttons read **Check** (free) and **Bet** (1 point).
- At most 4 bets per betting round; the blind counts as the first one before the flop.
- The dealer acts first before the flop, the blind acts first on the flop, turn and river.
- If someone runs out of points mid-hand, the rest of the board is dealt without further betting.
- The match ends when a player has no points left. Starting stack: 10, 20, 30 or 50.

## X-ray options

Toggle them in the sidebar or with keys 1 to 6, at any time, even mid-hand.

| Key | Option | What you see |
|-----|--------|--------------|
| 1 | Bottom card of the deck | The last card of the shuffled deck. It is never dealt, so it is a card nobody can have or hit. |
| 2 | Five bottom cards of the deck | The last five cards of the deck. None of them is dealt. |
| 3 | One of the bot's cards | One of the bot's two hole cards, face up. Which one is picked at random each hand. |
| 4 | Four possible bot hands | The bot's real hand plus three decoys, in random order. |
| 5 | Burned cards | The cards burned before the flop, turn and river, face up as they are burned. |
| 6 | Chance of winning | Your chance of winning at showdown, given what you can see. |

About the decoys in option 4: they are built from cards that are still in
the deck and are not dealt, burned or shown by options 1 and 2 during the
hand. A decoy is therefore never ruled out by a later board card, and all
four hands stay plausible to the river. Options 3 and 4 together do give
the real hand away, since only one candidate contains the exposed card.

Everything revealed by an option is marked in cyan.

### How the chance of winning is worked out

Option 6 uses your own cards, the board, and whatever the other options are
showing at that moment. It never uses anything you cannot see. Turning
another option on or off changes the number straight away:

- Bottom-of-deck cards (1, 2) and burned cards (5) are out of play: the bot
  cannot hold them and they cannot come on the board.
- An exposed bot card (3) fixes half of the bot's hand.
- The four possible hands (4) limit the bot to those hands. Since the decoys
  stay in the deck, none of the eight listed cards can come on the board
  either.
- Options 3 and 4 together leave exactly one possible hand.

From the flop onwards every possible outcome is counted, so the figure is
exact. Before the flop there are too many outcomes (unless the bot's hand is
pinned down), so 300,000 random deals are sampled instead and the figure is
shown with a `~` in front.

The colour runs from red (bad) through amber (even) to green (good). Two
cases get their own colour because they are certainties, not just very good
or very bad odds: violet when no outcome wins ("can't win") and gold when no
outcome loses ("can't lose"). 0% and 100% are only printed when they are
literally true; otherwise you see `<1%` or `>99%`. Ties are shown separately.

The sums run on a background thread and are cached per situation, so
switching an option off and on again is instant.

## Controls

| Key | Action |
|-----|--------|
| F | Fold |
| C | Check / Call |
| R | Bet / Raise |
| Space or Enter | Next hand |
| 1 to 6 | Toggle an X-ray option |

"Quick play" in the sidebar shortens the bot's thinking time and the dealing pauses.

## The bot

It estimates its chance of winning by dealing out a few hundred random
run-outs against a random hand, lowers that estimate when you have been
betting, compares it with the price of calling, and adds a little
randomness (occasional bluffs and slow-plays). It sees only its own cards,
the board and the betting.

## Code layout

| File | Contents |
|------|----------|
| `src/cards.rs` | Cards and the deck |
| `src/eval.rs` | Hand ranking: best five of seven, hand names |
| `src/game.rs` | The rules as a state machine: blinds, betting, dealing, showdown, peek data |
| `src/bot.rs` | The opponent |
| `src/odds.rs` | Chance of winning from what the player can see |
| `src/rng.rs` | Small random number generator |
| `src/app.rs` | Pacing, hand log, option toggles |
| `src/table.rs` | Table rendering and card animation (macroquad) |
| `src/gfx.rs` | Drawing primitives: cards, suits, chips, text |
| `src/panels.rs` | Sidebar and action bar (egui) |
| `src/main.rs` | Window setup and main loop |

The first six files have no rendering code, so the rules can be tested and
changed on their own. Good places to start tinkering: `MAX_BETS` and `BLIND`
in `game.rs`, `STACK_CHOICES` in `app.rs`, and the thresholds in `bot.rs`.

## Credits

SiteRaw.com.