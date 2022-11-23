mod backend_passthrough;
mod backend_vime;
mod im_server;

mod channel {
    use std::sync::mpsc;

    pub struct Channel<T> {
        pub tx: mpsc::Sender<T>,
        pub rx: mpsc::Receiver<T>,
    }

    pub fn pair<T>() -> (Channel<T>, Channel<T>) {
        let (tx1, rx1) = mpsc::channel::<T>();
        let (tx2, rx2) = mpsc::channel::<T>();
        (Channel { tx: tx1, rx: rx2 }, Channel { tx: tx2, rx: rx1 })
    }
}

use std::sync::Arc;
use xcb::x::Window;

/// A message passed among threads.
pub enum Message {
    Conn(Arc<xcb::Connection>),
    Window(Window),
    StartPreedit(xcb_imdkit::Ic),
    CancelPreedit,
    FocusOut,
    EditResult(xcb_imdkit::Ic, Option<String>),
    ForwardEvent(xcb_imdkit::Ic, xcb_imdkit::KeyEvent),
}

pub type Channel = channel::Channel<Message>;

fn main() {
    env_logger::init();

    let (chan_passthru_a, chan_passthru_b) = channel::pair::<Message>();
    let (chan_vime_a, chan_vime_b) = channel::pair::<Message>();
    std::thread::spawn(move || im_server::main(chan_vime_a, chan_passthru_a));
    std::thread::spawn(move || backend_passthrough::main(chan_passthru_b));
    backend_vime::main(chan_vime_b);
}

/// Creates an invisible window.
pub fn create_dummy_window(conn: &xcb::Connection, screen: &xcb::x::Screen) -> Window {
    use xcb::x::{CreateWindow, Cw, WindowClass, COPY_FROM_PARENT};

    let wid = conn.generate_id();
    conn.send_request(&CreateWindow {
        depth: COPY_FROM_PARENT as u8,
        wid,
        parent: screen.root(),
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        border_width: 0,
        class: WindowClass::InputOnly,
        visual: screen.root_visual(),
        value_list: &[Cw::OverrideRedirect(true)],
    });
    conn.flush().unwrap();
    wid
}

/// Returns an atom used for client message tag.
pub fn get_vime_message_type(conn: &xcb::Connection) -> xcb::x::Atom {
    use xcb::x::Atom;
    use xcb::Xid as _;

    use std::sync::RwLock;
    lazy_static::lazy_static! {
        static ref VIME_TYPE: RwLock<Atom> = RwLock::new(Atom::none());
    }

    let lock = VIME_TYPE.read().unwrap();
    if lock.is_none() {
        drop(lock);

        let cookie = conn.send_request(&xcb::x::InternAtom {
            only_if_exists: false,
            name: b"VIME_MESSAGE",
        });
        let reply = conn.wait_for_reply(cookie).unwrap();

        let mut lock = VIME_TYPE.write().unwrap();
        *lock = reply.atom();
        *lock
    } else {
        *lock
    }
}

/// Sends an empty message with the vime message type to the dst window.
pub fn notify(conn: &xcb::Connection, src: Window, dst: Window) {
    use xcb::x::{ClientMessageData, ClientMessageEvent, EventMask, SendEvent, SendEventDest};

    let msg = ClientMessageEvent::new(
        src,
        get_vime_message_type(conn),
        ClientMessageData::Data8([0; 20]),
    );
    conn.send_request(&SendEvent {
        propagate: false,
        destination: SendEventDest::Window(dst),
        event_mask: EventMask::empty(),
        event: &msg,
    });
    conn.flush().unwrap();
}

/// Determines whether an event has the vime message type
/// if true returns the src window, otherwise returns None.
pub fn is_vime_message(conn: &xcb::Connection, event: &xcb::Event) -> Option<Window> {
    if let xcb::Event::X(xcb::x::Event::ClientMessage(msg)) = event {
        if msg.r#type() == get_vime_message_type(conn) {
            return Some(msg.window());
        }
    }
    None
}
