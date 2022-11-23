use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use xcb::x::Window;
use xcb_imdkit::{Ic, ImeClient, InputStyle, KeyEvent};

use crate::{create_dummy_window, is_vime_message, notify, Channel, Message};

#[derive(Clone)]
pub struct Context {
    conn: Arc<xcb::Connection>,
    default_screen: i32,

    chan: Rc<Channel>,
    win_server: Window,
    win_dummy: Window,
    current_ic: Rc<RefCell<Option<Ic>>>,
}

impl Context {
    pub fn new(chan: Channel) -> Self {
        let chan = Rc::new(chan);

        let (conn, default_screen) = xcb::Connection::connect(None).unwrap();
        let conn = Arc::new(conn);
        let screen = conn
            .get_setup()
            .roots()
            .nth(default_screen as usize)
            .unwrap();

        let win_dummy = create_dummy_window(&conn, screen);

        chan.tx.send(Message::Window(win_dummy)).unwrap();

        let Ok(Message::Window(win_server)) = chan.rx.recv() else { panic!("bug") };

        Self {
            conn,
            default_screen,
            chan,
            win_server,
            win_dummy,
            current_ic: Rc::new(RefCell::new(None)),
        }
    }
}

pub fn main(chan: Channel) {
    let ctx = Context::new(chan);

    if std::env::var("XMODIFIERS").is_ok() {
        with_ime(ctx);
    } else {
        without_ime(ctx);
    }
}

fn with_ime(c: Context) {
    // ImeClient::set_logger(|msg| log::trace!("Log: {}", msg));

    let mut ime = ImeClient::new(
        c.conn.clone(),
        c.default_screen,
        InputStyle::PREEDIT_CALLBACKS,
        None, // derive $XMODIFIERS
    );

    ime.update_pos(c.win_dummy, 0, 0);

    ime.set_commit_string_cb({
        let c = c.clone();
        move |_win, input| {
            let Some(ic) = c.current_ic.borrow().clone() else { return };

            let msg = Message::EditResult(ic, Some(input.to_owned()));
            c.chan.tx.send(msg).unwrap();
            notify(&c.conn, c.win_dummy, c.win_server);
        }
    });

    ime.set_forward_event_cb({
        let c = c.clone();
        move |_win, key_event| {
            let Some(ic) = c.current_ic.borrow().clone() else { return };
            let msg = Message::ForwardEvent(ic, key_event);
            c.chan.tx.send(msg).unwrap();
            notify(&c.conn, c.win_dummy, c.win_server);
        }
    });

    // ime.set_preedit_draw_cb(move |_win, _info| {});

    loop {
        let event = c.conn.wait_for_event().unwrap();

        if is_vime_message(&c.conn, &event).is_some() {
            match c.chan.rx.recv().unwrap() {
                Message::StartPreedit(ic) => {
                    log::debug!("passthru: start preedit");
                    *c.current_ic.borrow_mut() = Some(ic);
                }
                Message::CancelPreedit => {
                    log::debug!("passthru: cancel preedit");
                    *c.current_ic.borrow_mut() = None;
                }
                _ => {}
            }
            continue;
        }

        ime.process_event(&event);
    }
}

fn without_ime(c: Context) {
    loop {
        let event = c.conn.wait_for_event().unwrap();

        if is_vime_message(&c.conn, &event).is_some() {
            match c.chan.rx.recv().unwrap() {
                Message::StartPreedit(ic) => {
                    *c.current_ic.borrow_mut() = Some(ic);
                }
                Message::CancelPreedit => {
                    *c.current_ic.borrow_mut() = None;
                }
                _ => {}
            }
            continue;
        }

        let Some(ic) = c.current_ic.borrow().clone() else { continue };

        let xcb::Event::X(xev) = event else { continue };
        let Some(key_event) = KeyEvent::from_xevent(xev) else { continue };

        let msg = Message::ForwardEvent(ic, key_event);
        c.chan.tx.send(msg).unwrap();
        notify(&c.conn, c.win_dummy, c.win_server);
    }
}
