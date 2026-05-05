//! Input thread: polls crossterm events on its own thread, translates to
//! [`InputEvent`], pushes onto a bounded channel. Owned and torn down by
//! [`super::run`] (and re-spawned around editor shell-out).

use crate::render::view::NavEvent;
use crossbeam_channel::Sender;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    Quit,
    Nav(NavEvent),
    ToggleSidebar,
    Edit,
}

pub fn spawn_input_thread(tx: Sender<InputEvent>, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("input".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(err) => {
                        tracing::error!(error = %err, "crossterm poll failed");
                        return;
                    }
                }
                let evt = match event::read() {
                    Ok(e) => e,
                    Err(err) => {
                        tracing::error!(error = %err, "crossterm read failed");
                        return;
                    }
                };
                let Some(out) = translate(evt) else { continue };
                if tx.send(out).is_err() {
                    return;
                }
            }
        })
        .expect("input thread spawn")
}

pub fn translate(evt: Event) -> Option<InputEvent> {
    let Event::Key(key) = evt else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Some(InputEvent::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(InputEvent::Quit),
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
            Some(InputEvent::Nav(NavEvent::ScrollDown(1)))
        }
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => Some(InputEvent::Nav(NavEvent::ScrollUp(1))),
        (KeyCode::PageDown, _) | (KeyCode::Char('d'), _) => {
            Some(InputEvent::Nav(NavEvent::ScrollDown(20)))
        }
        (KeyCode::PageUp, _) | (KeyCode::Char('u'), _) => {
            Some(InputEvent::Nav(NavEvent::ScrollUp(20)))
        }
        (KeyCode::Home, _) | (KeyCode::Char('g'), _) => Some(InputEvent::Nav(NavEvent::Top)),
        (KeyCode::Char('n'), _) => Some(InputEvent::Nav(NavEvent::NextFile)),
        (KeyCode::Char('b'), _) => Some(InputEvent::Nav(NavEvent::PrevFile)),
        (KeyCode::Char('s'), _) => Some(InputEvent::ToggleSidebar),
        (KeyCode::Char('e'), _) | (KeyCode::Enter, _) => Some(InputEvent::Edit),
        _ => None,
    }
}
