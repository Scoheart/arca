//! Interactive TUI — the no-arg `arca` experience. Built on The Elm Architecture
//! (model/message/update/view) over `arca-core`. See `docs/tui-architecture.md`.

mod effect;
mod message;
mod model;
mod update;
mod view;

use std::sync::mpsc;
use std::thread;

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use effect::EffectRunner;
use message::Msg;
use model::Model;
use update::update;
use view::view;

/// Enter the alt screen, run the loop, and always restore the terminal (a panic
/// hook installed by `ratatui::init` restores it on unwind too).
pub fn run() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let res = run_loop(&mut terminal);
    ratatui::restore();
    res
}

fn run_loop(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    spawn_input_thread(tx.clone());
    let runner = EffectRunner::new(tx);

    let mut model = Model::new();
    for eff in update(&mut model, Msg::Init) {
        runner.spawn(eff);
    }

    while !model.should_quit {
        terminal.draw(|f| view(&model, f))?;
        // One channel multiplexes input-thread events and effect results, so the
        // loop blocks on exactly one recv. Timeout only drives idle/animation ticks.
        let msg = match rx.recv_timeout(model.tick_interval()) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => Msg::Tick,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        for eff in update(&mut model, msg) {
            runner.spawn(eff);
        }
    }
    Ok(())
}

/// A dedicated thread blocks on terminal input and forwards it as `Msg`s, so the
/// main loop never polls.
fn spawn_input_thread(tx: mpsc::Sender<Msg>) {
    thread::spawn(move || loop {
        match event::read() {
            Ok(Event::Key(k)) => {
                if tx.send(Msg::Key(k)).is_err() {
                    break;
                }
            }
            Ok(Event::Resize(_, _)) => {
                if tx.send(Msg::Redraw).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });
}
