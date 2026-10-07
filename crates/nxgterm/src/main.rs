//! nxgterm: window, shell in a pty, GPU-rendered text with a CPU fallback.

mod app;
mod choice;
mod keys;

use std::error::Error;

use winit::event_loop::EventLoop;

use crate::app::{App, UserEvent};

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut app = App::new(event_loop.create_proxy());
    event_loop.run_app(&mut app)?;
    match app.take_error() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
