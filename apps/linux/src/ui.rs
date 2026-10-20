use adw::prelude::*;
use gtk::gio;

/// Shows errors from Core as toasts.
#[derive(Clone)]
pub enum Toaster {
    Window(adw::ToastOverlay),
    Preferences(adw::PreferencesDialog),
}

impl Toaster {
    pub fn show(&self, text: impl AsRef<str>) {
        let toast = adw::Toast::new(text.as_ref());
        toast.set_use_markup(false);
        toast.set_timeout(6);
        match self {
            Toaster::Window(o) => o.add_toast(toast),
            Toaster::Preferences(d) => d.add_toast(toast),
        }
    }
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

pub fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

pub fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_downcast::<gtk::Window>()
}

/// Sets the placeholder of an `adw::EntryRow`'s text field.
pub fn row_placeholder(row: &adw::EntryRow, text: &str) {
    if let Some(t) = row.delegate().and_downcast::<gtk::Text>() {
        t.set_placeholder_text(Some(text));
    }
}

/// Runs `f` when `widget` loses keyboard focus.
pub fn on_focus_out(widget: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(move |_| f());
    widget.add_controller(focus);
}

/// Opens `path` with its default app, or its containing folder, toasting failures.
pub fn launch(widget: &impl IsA<gtk::Widget>, toaster: &Toaster, path: &str, folder: bool) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    let parent = window_of(widget);
    let (toaster, path) = (toaster.clone(), path.to_owned());
    let done = move |res: Result<(), gtk::glib::Error>| {
        if let Err(e) = res {
            toaster.show(format!("{path}: {}", e.message()));
        }
    };
    if folder {
        launcher.open_containing_folder(parent.as_ref(), gio::Cancellable::NONE, done);
    } else {
        launcher.launch(parent.as_ref(), gio::Cancellable::NONE, done);
    }
}
