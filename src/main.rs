//! X-Ray Hold'em: heads-up fixed-limit Texas Hold'em against a bot, with
//! six optional "peek" modes that only the human player gets to use.
//!
//! * `cards`, `eval`, `game`, `bot`, `odds`, `rng`: the rules, with no rendering.
//! * `gfx`, `table`: the table drawn with macroquad.
//! * `panels`: sidebar and action bar built with egui.
//! * `app`: glue (pacing, log, toggles).

mod app;
mod bot;
mod cards;
mod eval;
mod game;
mod gfx;
mod odds;
mod panels;
mod rng;
mod table;

use macroquad::prelude::*;

use app::App;
use game::Action;

fn window_conf() -> Conf {
    Conf {
        window_title: "X-Ray Hold'em".to_owned(),
        // Small enough to fit a 1280x720 laptop screen with room for the
        // title bar and taskbar. Everything scales with the window, so
        // maximise it (or drag it bigger) on a larger display.
        window_width: 1000,
        window_height: 636,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let gfx = gfx::Gfx::load();
    egui_macroquad::cfg(|ctx| {
        panels::install_style(ctx);
        // Start at the right zoom so the first frame is already in proportion.
        let scale = panels::ui_scale((screen_width(), screen_height()));
        ctx.set_pixels_per_point(scale * screen_dpi_scale());
    });
    let mut app = App::new(gfx, get_time());

    loop {
        let now = get_time();

        // Lay out the egui panels first: that tells us how much room is
        // left for the table. They are painted after the table, below.
        let mut ui = panels::UiOut::default();
        let window = (screen_width(), screen_height());
        let dpi = screen_dpi_scale();
        egui_macroquad::ui(|ctx| ui = panels::build(ctx, &mut app, window, dpi));

        // Keyboard shortcuts mirror the buttons.
        if is_key_pressed(KeyCode::F) {
            ui.action = Some(Action::Fold);
        }
        if is_key_pressed(KeyCode::C) {
            ui.action = Some(Action::Call);
        }
        if is_key_pressed(KeyCode::R) {
            ui.action = Some(Action::Raise);
        }
        if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
            ui.next_hand = true;
        }
        for (key, number) in [
            (KeyCode::Key1, 1),
            (KeyCode::Key2, 2),
            (KeyCode::Key3, 3),
            (KeyCode::Key4, 4),
            (KeyCode::Key5, 5),
            (KeyCode::Key6, 6),
        ] {
            if is_key_pressed(key) {
                app.peeks.toggle(number);
            }
        }

        if ui.new_match {
            app.new_match(now);
        } else if ui.next_hand {
            app.next_hand(now);
        } else if let Some(action) = ui.action {
            // A fold is only accepted when there is something to fold to;
            // the engine would otherwise treat it as a check.
            let legal = app.game.legal();
            let allowed = match action {
                Action::Fold => legal.can_fold,
                Action::Raise => legal.can_raise,
                Action::Call => true,
            };
            if allowed {
                app.human_action(action);
            }
        }
        app.update(now);

        clear_background(gfx::pal::BACKDROP);
        let (x, y, w, h) = ui.table;
        table::draw(&mut app, Rect::new(x, y, w, h), now);
        egui_macroquad::draw();

        next_frame().await
    }
}
