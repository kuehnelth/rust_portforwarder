use cursive::{Cursive};
use cursive::views::*;
use cursive::traits::*;

use std::thread;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[path="../portforwarder.rs"]
mod portforwarder;

fn connect(siv: &mut Cursive) {
    let src = siv.call_on_id("src", |view: &mut EditView| view.get_content()).unwrap();
    let dst = siv.call_on_id("dst", |view: &mut EditView| view.get_content()).unwrap();

    let dst_addr = portforwarder::get_ipv4_socket_addr(&dst).unwrap();
    let src_addr = portforwarder::get_ipv4_socket_addr(&src).unwrap();

    siv.set_fps(5);
    let abort = Arc::new(AtomicBool::new(false));

    let abort1 = Arc::clone(&abort);
    thread::spawn(move || {
        portforwarder::forward(src_addr, dst_addr, Some(&abort1)).unwrap();
    });

    siv.add_layer(Dialog::around(LinearLayout::vertical()
                                 .child(DebugView::new().full_screen())
                                 .scrollable()
                                 .scroll_x(true))
                  .button("Disconnect", move |s| { abort.store(true, Ordering::Relaxed); s.pop_layer(); } ));
}

fn main() {
    /* pancurses on windows opens it's own window.
     * So hide the standard windows console.
     */
    #[cfg(all(windows, release))]
    unsafe {
        use winapi::um::wincon::FreeConsole;
        FreeConsole();
    }

    cursive::logger::init();
    // Creates the cursive root - required for every application.
    let mut siv = Cursive::default();

    siv.add_layer(Dialog::new()
                  .title("Portforwarder")
                  .content(ListView::new()
                           .child("Connect to:", EditView::new().content("127.0.0.1:2815").with_id("dst").fixed_width(32))
                           .child("Listen on: ", EditView::new().content("127.0.0.1:1815").with_id("src").fixed_width(32))
                  )
                  .button("Connect", |s| connect(s))
                  .button("Quit", |s| s.quit()));

    // Starts the event loop.
    siv.run();
}
