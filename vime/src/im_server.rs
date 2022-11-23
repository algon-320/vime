use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{atomic, Arc};

use xcb::x::Window;
use xcb::Xid as _;
use xcb_imdkit::{Ic, ImeServer, ImeServerCallbacks};

use crate::{create_dummy_window, is_vime_message, notify, Channel, Message};

const IM_NAME: &str = "vime";

#[derive(Clone)]
struct Context {
    conn: Arc<xcb::Connection>,
    default_screen: i32,

    chan_vime: Rc<Channel>,
    chan_passthru: Rc<Channel>,

    win_server: Window,
    win_vime: Window,
    win_passthru: Window,

    current_ic: Rc<RefCell<Option<Ic>>>,
    vime_state: VimeState,
}

impl Context {
    fn new(chan_vime: Channel, chan_passthru: Channel) -> Self {
        let chan_vime = Rc::new(chan_vime);
        let chan_passthru = Rc::new(chan_passthru);

        let (conn, default_screen) = xcb::Connection::connect(None).unwrap();
        let conn = Arc::new(conn);
        let screen = conn
            .get_setup()
            .roots()
            .nth(default_screen as usize)
            .unwrap();

        let win_server = create_dummy_window(&conn, screen);

        let Ok(Message::Window(win_vime)) = chan_vime.rx.recv() else { panic!("bug") };
        let Ok(Message::Window(win_passthru)) = chan_passthru.rx.recv() else { panic!("bug") };

        chan_vime.tx.send(Message::Window(win_server)).unwrap();
        chan_vime.tx.send(Message::Conn(conn.clone())).unwrap();

        chan_passthru.tx.send(Message::Window(win_server)).unwrap();

        let current_ic = Rc::new(RefCell::new(None));
        let vime_state = VimeState::new(false);

        Self {
            conn,
            default_screen,
            chan_vime,
            chan_passthru,
            win_server,
            win_vime,
            win_passthru,
            current_ic,
            vime_state,
        }
    }
}

#[derive(Clone)]
struct VimeState {
    active: Rc<atomic::AtomicBool>,
}
impl VimeState {
    fn new(active: bool) -> Self {
        Self {
            active: Rc::new(atomic::AtomicBool::new(active)),
        }
    }
    fn is_active(&self) -> bool {
        self.active.load(atomic::Ordering::SeqCst)
    }
    fn inactivate(&self) {
        self.active.store(false, atomic::Ordering::SeqCst)
    }
    fn toggle(&self) -> bool {
        self.active.fetch_xor(true, atomic::Ordering::SeqCst)
    }
}

pub fn main(chan_vime: Channel, chan_passthru: Channel) {
    let c = Context::new(chan_vime, chan_passthru);

    let im_server_callbacks = ImeServerCallbacks {
        // NOTE: Always enabled
        trigger: { Box::new(move |_, _ic, _enable| {}) },

        focus_in: {
            let c = c.clone();
            Box::new(move |_, ic| {
                *c.current_ic.borrow_mut() = Some(ic.clone());
                log::debug!("focus_in");

                if c.vime_state.is_active() {
                    c.chan_vime.tx.send(Message::StartPreedit(ic)).unwrap();
                } else {
                    c.chan_passthru.tx.send(Message::StartPreedit(ic)).unwrap();
                    notify(&c.conn, c.win_server, c.win_passthru);
                }
            })
        },
        focus_out: {
            let c = c.clone();
            Box::new(move |_, ic| {
                if *c.current_ic.borrow() == Some(ic) {
                    *c.current_ic.borrow_mut() = None;
                    log::debug!("focus_out");

                    if c.vime_state.is_active() {
                        c.chan_vime.tx.send(Message::FocusOut).unwrap();
                    }
                }
            })
        },

        forward: {
            let c = c.clone();

            let trigger_key_state = vime_config::CONFIG.trigger_key_state;
            let trigger_key_state = xcb::x::KeyButMask::from_bits(trigger_key_state).unwrap();
            let trigger_key_keycode = vime_config::CONFIG.trigger_key_keycode;

            Box::new(move |_, ic, mut key_event| {
                if *c.current_ic.borrow() != Some(ic) {
                    log::trace!("forward: mismatch ic");
                    return;
                }

                // FIXME: use key-symbol
                if key_event.state.contains(trigger_key_state)
                    && key_event.detail == trigger_key_keycode
                {
                    if key_event.is_press {
                        if !c.vime_state.toggle() {
                            c.chan_passthru.tx.send(Message::CancelPreedit).unwrap();
                            notify(&c.conn, c.win_server, c.win_passthru);

                            let ic = c.current_ic.borrow().clone().unwrap();
                            c.chan_vime.tx.send(Message::StartPreedit(ic)).unwrap();
                        } else {
                            c.chan_vime.tx.send(Message::CancelPreedit).unwrap();

                            let ic = c.current_ic.borrow().clone().unwrap();
                            c.chan_passthru.tx.send(Message::StartPreedit(ic)).unwrap();
                            notify(&c.conn, c.win_server, c.win_passthru);
                        }
                    }
                } else {
                    let target = if c.vime_state.is_active() {
                        c.win_vime
                    } else {
                        c.win_passthru
                    };

                    key_event.event = target;
                    key_event.child = Window::none();
                    let synth_event = key_event.to_generic();

                    c.conn.send_request(&xcb::x::SendEvent {
                        event: &synth_event,
                        destination: xcb::x::SendEventDest::Window(target),
                        propagate: false,
                        event_mask: xcb::x::EventMask::empty(),
                    });
                    c.conn.flush().unwrap();
                }
            })
        },

        position_changed: {
            let c = c.clone();
            Box::new(move |_, ic, win, pos_x, pos_y| {
                if *c.current_ic.borrow() != Some(ic) {
                    log::trace!("position_changed: mismatch ic");
                    return;
                }

                let (win_x, win_y) = absolute_position(&c.conn, win);
                let (x, y) = (win_x + pos_x, win_y + pos_y);

                c.conn.send_request(&xcb::x::ConfigureWindow {
                    window: c.win_passthru,
                    value_list: &[
                        xcb::x::ConfigWindow::X(x as i32),
                        xcb::x::ConfigWindow::Y(y as i32),
                        xcb::x::ConfigWindow::StackMode(xcb::x::StackMode::Above),
                    ],
                });

                let (x, y) = adjust_vime_window_position(&c.conn, x, y, c.win_vime);
                c.conn.send_request(&xcb::x::ConfigureWindow {
                    window: c.win_vime,
                    value_list: &[
                        xcb::x::ConfigWindow::X(x as i32),
                        xcb::x::ConfigWindow::Y(y as i32),
                        xcb::x::ConfigWindow::StackMode(xcb::x::StackMode::Above),
                    ],
                });

                c.conn.flush().unwrap();
            })
        },
    };

    let server = ImeServer::new(
        c.conn.clone(),
        c.default_screen,
        c.win_server,
        IM_NAME,
        true,
        im_server_callbacks,
    );

    loop {
        let event = c.conn.wait_for_event().unwrap();

        if let Some(win) = is_vime_message(&c.conn, &event) {
            if win == c.win_vime {
                match c.chan_vime.rx.recv().unwrap() {
                    Message::EditResult(ic, Some(text)) => {
                        server.commit_string(ic, &text);
                    }
                    Message::EditResult(_, None) => {}
                    _ => unreachable!(),
                }

                c.vime_state.inactivate();

                if let Some(ic) = c.current_ic.borrow().clone() {
                    c.chan_passthru.tx.send(Message::StartPreedit(ic)).unwrap();
                    notify(&c.conn, c.win_server, c.win_passthru);
                }
            } else if win == c.win_passthru {
                match c.chan_passthru.rx.recv().unwrap() {
                    Message::EditResult(ic, Some(text)) => {
                        server.commit_string(ic, &text);
                    }

                    Message::ForwardEvent(ic, mut key_event) => {
                        key_event.event = server.get_client_window(&ic);
                        key_event.child = Window::none();
                        server.forward_event(ic, key_event);
                    }

                    _ => unreachable!(),
                }
            }
            continue;
        }

        server.process_event(event);
    }
}

/// Calculates the absolute position of the upper-left corner of a window.
fn absolute_position(conn: &xcb::Connection, win: Window) -> (i16, i16) {
    let mut win = win;
    let mut abs_x = 0;
    let mut abs_y = 0;

    while !win.is_none() {
        let cookie = conn.send_request(&xcb::x::GetGeometry {
            drawable: xcb::x::Drawable::Window(win),
        });
        let reply = conn.wait_for_reply(cookie).unwrap();

        abs_x += reply.x();
        abs_y += reply.y();

        let cookie = conn.send_request(&xcb::x::QueryTree { window: win });
        let reply = conn.wait_for_reply(cookie).unwrap();
        win = reply.parent();
    }

    (abs_x, abs_y)
}

/// Calculates a better position of the vime window so that it won't go out of the screen,
/// and returns the adjusted position.
fn adjust_vime_window_position(
    conn: &xcb::Connection,
    x: i16,
    y: i16,
    win_vime: Window,
) -> (i16, i16) {
    let cookie = conn.send_request(&xcb::randr::GetMonitors {
        window: win_vime,
        get_active: true,
    });
    let reply = conn.wait_for_reply(cookie).unwrap();

    let mon_info = reply
        .monitors()
        .find(|info| {
            let x_in = info.x() <= x && x as i32 <= info.x() as i32 + info.width() as i32;
            let y_in = info.y() <= y && y as i32 <= info.y() as i32 + info.height() as i32;
            x_in && y_in
        })
        .or_else(|| reply.monitors().next());

    let Some(mon_info) = mon_info else { return (x, y) };

    #[rustfmt::skip]
    struct Rect { x: i32, y: i32, w: i32, h: i32 }

    #[rustfmt::skip]
    impl Rect {
        fn t(&self) -> i32 { self.y }
        fn b(&self) -> i32 { self.y + self.h }
        fn l(&self) -> i32 { self.x }
        fn r(&self) -> i32 { self.x + self.w }
    }

    let mon = Rect {
        x: mon_info.x() as i32,
        y: mon_info.y() as i32,
        w: mon_info.width() as i32,
        h: mon_info.height() as i32,
    };

    let cookie = conn.send_request(&xcb::x::GetGeometry {
        drawable: xcb::x::Drawable::Window(win_vime),
    });
    let reply = conn.wait_for_reply(cookie).unwrap();

    let mut win = Rect {
        x: x as i32,
        y: y as i32,
        w: reply.width() as i32,
        h: reply.height() as i32,
    };

    if win.t() < mon.t() {
        win.y = mon.t();
    }
    if win.l() < mon.l() {
        win.x = mon.l();
    }
    if mon.b() < win.b() {
        win.y = (y as i32) - win.h - 40;
    }
    if mon.r() < win.r() {
        win.x = mon.r() - win.w;
    }

    (win.x as i16, win.y as i16)
}
