use cursive::{Cursive};
use cursive::views::*;
use cursive::traits::*;

use std::thread;
use std::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dirs::config_dir;

use log::{info, warn};

#[path="../portforwarder.rs"]
mod portforwarder;

fn connect(siv: &mut Cursive) -> Result<(), io::Error> {
    let src = siv.call_on_id("src", |view: &mut EditView| view.get_content()).unwrap();
    let dst = siv.call_on_id("dst", |view: &mut EditView| view.get_content()).unwrap();

    let dst_addr = portforwarder::get_ipv4_socket_addr(&dst)?;
    let src_addr = portforwarder::get_ipv4_socket_addr(&src)?;

    siv.set_fps(5);
    let abort = Arc::new(AtomicBool::new(false));

    let abort1 = Arc::clone(&abort);
    thread::spawn(move || {
        match portforwarder::forward(src_addr, dst_addr, Some(&abort1)) {
            Err(e) => {
                warn!("forwarder error {}", e);
            }
            Ok(()) => {
                info!("forwarder exitited without error");
            }
        }
    });

    siv.add_layer(Dialog::around(LinearLayout::vertical()
                                 .child(DebugView::new().full_screen())
                                 .scrollable()
                                 .scroll_x(true))
                  .button("Disconnect", move |s| { abort.store(true, Ordering::Relaxed); s.pop_layer(); } ));
    Ok(())
}

fn save(siv: &mut Cursive) -> Result<(), io::Error> {
    let src = siv.call_on_id("src", |view: &mut EditView| view.get_content()).unwrap();
    let dst = siv.call_on_id("dst", |view: &mut EditView| view.get_content()).unwrap();
    let configfile = config_dir().map(|p| p.join(r"portforwarder.config")).
		ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Configfile not found"))?;

    fs::write(configfile, format!("{},{}", src, dst))?;

    Ok(())
}

fn load() -> (String, String) {
    let cfg = config_dir().map(|p| p.join(r"portforwarder.config"))
        .and_then(|configfile| fs::read_to_string(configfile).ok())
		.unwrap_or_else(|| "".to_string());

	let mut split: Vec<&str> = cfg.splitn(2, ',').collect();

	if split.len() != 2 {
		split = ["127.0.0.1:815", "127.0.0.1:1815"].to_vec();
	}

	(split[0].trim().to_string(), split[1].trim().to_string())
}

fn main() {
    /* pancurses on windows opens it's own window.
     * So hide the standard windows console.
     */
    #[cfg(windows)]
    unsafe {
        use winapi::um::wincon::FreeConsole;
        FreeConsole();
    }

    cursive::logger::init();
    // Creates the cursive root - required for every application.
    let mut siv = Cursive::default();

    let (src, dst) = load();

    siv.add_layer(Dialog::new()
                  .title("Portforwarder")
                  .content(ListView::new()
                           .child("Connect to:", EditView::new().content(dst).with_id("dst").fixed_width(32))
                           .child("Listen on: ", EditView::new().content(src).with_id("src").fixed_width(32))
                  )
                  .button("Save", |s| { if let Err(e) = save(s) {
                      let content = format!("Error saving config: {}!", e);
                      s.add_layer(
                          Dialog::around(TextView::new(content))
                              .button("Close", |s| {s.pop_layer().unwrap();}),
                      );
                  }
                  })
                  .button("Connect", |s| { if let Err(e) = connect(s) {
                      let content = format!("Error connecting: {}!", e);
                      s.add_layer(
                          Dialog::around(TextView::new(content))
                              .button("Close", |s| {s.pop_layer().unwrap();}),
                      );
                  }})
                  .button("Quit", |s| s.quit()));

    // Starts the event loop.
    siv.run();
}
