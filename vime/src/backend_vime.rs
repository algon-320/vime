use std::cell::RefCell;
use std::rc::Rc;

use toyterm::{glium, window, TOYTERM_CONFIG};
use xcb::XidNew as _;
use xcb_imdkit::Ic;

use crate::{notify, Channel, Message};

pub fn main(chan: Channel) {
    // Make sure that configuration errors are detected earlier
    lazy_static::initialize(&TOYTERM_CONFIG);

    let event_loop = glium::glutin::event_loop::EventLoop::new();

    let display = {
        let title = "vime";

        use glium::glutin::platform::unix::WindowBuilderExtUnix as _;
        use glium::glutin::{window::WindowBuilder, ContextBuilder};

        let win_builder = WindowBuilder::new()
            .with_title(title)
            .with_resizable(true)
            .with_override_redirect(true)
            .with_always_on_top(true);

        let ctx_builder = ContextBuilder::new().with_vsync(true).with_srgb(true);
        glium::Display::new(win_builder, ctx_builder, &event_loop).expect("display new")
    };

    let mut term = window::TerminalWindow::new(display, None);

    {
        // Invisible by default
        term.hide();

        let rows = vime_config::CONFIG.default_rows;
        let cols = vime_config::CONFIG.default_columns;
        term.resize_with_terminal_size(window::TerminalSize { rows, cols });
    }

    let vime_win = unsafe { xcb::x::Window::new(term.window_id()) };
    chan.tx.send(Message::Window(vime_win)).unwrap();

    let Ok(Message::Window(server_win)) = chan.rx.recv() else { panic!("bug") };
    let Ok(Message::Conn(conn)) = chan.rx.recv() else { panic!("bug") };

    // Set border width
    conn.send_request(&xcb::x::ConfigureWindow {
        window: vime_win,
        value_list: &[xcb::x::ConfigWindow::BorderWidth(1)],
    });
    conn.flush().unwrap();

    let current_ic: Rc<RefCell<Option<Ic>>> = Rc::new(RefCell::new(None));

    event_loop.run(move |event, _, control_flow| {
        let Some(event) = event.to_static() else { return };

        loop {
            match chan.rx.try_recv() {
                Ok(Message::StartPreedit(ic)) => {
                    if current_ic.borrow().as_ref() == Some(&ic) {
                        log::debug!("vime: restart");
                    } else {
                        log::debug!("vime: start preedit, set new ic");
                        *current_ic.borrow_mut() = Some(ic);

                        term.close_pty();
                        let _ = std::fs::remove_file("/tmp/vime_buffer.txt");
                        term.reset_pty();
                    }

                    // HACK
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    term.show();
                }

                Ok(Message::FocusOut) => {
                    log::debug!("vime: focus out");
                    term.hide();
                }

                Ok(Message::CancelPreedit) => {
                    log::debug!("vime: cancel preedit");
                    term.hide();
                    term.close_pty();
                }

                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    break;
                }
                _ => panic!("disconnected"),
            }
        }

        term.on_event(&event, control_flow);

        use glium::glutin::event_loop::ControlFlow;
        if *control_flow == ControlFlow::Exit {
            *control_flow = ControlFlow::default();

            let edit_result = std::fs::read_to_string("/tmp/vime_buffer.txt").ok();

            if let Some(status) = term.reset_pty() {
                let ic = current_ic.borrow().clone().unwrap();
                let msg = Message::EditResult(ic, if status == 0 { edit_result } else { None });
                chan.tx.send(msg).unwrap();
                notify(&conn, vime_win, server_win);

                term.hide();

                log::debug!("vime: reset ic");
                *current_ic.borrow_mut() = None;
            }
        }
    });
}
