mod copy;
mod dates_view;
mod lazy_list;
mod settings_panel;
mod startup;
mod status_view;
mod tab_page;
mod ui;
mod window;

use adw::prelude::*;
use gtk::gio;
use photosink_bridge::{Change, Core, Listener};
use std::rc::Rc;
use std::sync::Arc;

const APP_ID: &str = "com.n8mgr.photosink";

struct Relay(async_channel::Sender<Change>);

impl Listener for Relay {
    fn changed(&self, change: Change) {
        let _ = self.0.send_blocking(change);
    }
}

fn about(win: &window::Window) {
    let about = adw::AboutDialog::builder()
        .application_name("photosink")
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Export Apple Photos, OneDrive and Google Takeout into one folder tree")
        .build();
    about.present(Some(&win.window));
}

fn activate(app: &adw::Application) {
    if let Some(win) = app
        .active_window()
        .or_else(|| app.windows().into_iter().next())
    {
        win.present();
        return;
    }
    let (tx, rx) = async_channel::unbounded();
    let core = Core::new(Arc::new(Relay(tx)));
    let win = window::Window::new(app, core);

    let preferences = gio::ActionEntry::builder("preferences")
        .activate({
            let win = Rc::downgrade(&win);
            move |_: &adw::Application, _, _| {
                if let Some(win) = win.upgrade() {
                    win.show_preferences();
                }
            }
        })
        .build();
    let about_action = gio::ActionEntry::builder("about")
        .activate({
            let win = Rc::downgrade(&win);
            move |_: &adw::Application, _, _| {
                if let Some(win) = win.upgrade() {
                    about(&win);
                }
            }
        })
        .build();
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| app.quit())
        .build();
    app.add_action_entries([preferences, about_action, quit]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);

    win.window.present();
    gtk::glib::spawn_future_local(async move {
        while let Ok(change) = rx.recv().await {
            win.apply(change);
        }
    });
}

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run()
}
