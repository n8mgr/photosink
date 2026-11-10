use crate::copy;
use crate::dates_view::DatesPanel;
use crate::status_view::StatusView;
use crate::ui::{self, Toaster, hbox, vbox};
use adw::prelude::*;
use gtk::gio;
use photosink_bridge::Core;
use photosink_bridge::catalog::{RunArgs, Tab};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Pick {
    Folder,
    Archive(&'static [&'static str]),
}

/// One tab: its argument group over its rows, and its header-bar controls.
pub struct TabPage {
    core: Arc<Core>,
    toaster: Toaster,
    pub tab: Tab,
    pub content: gtk::Box,
    pub header: gtk::Widget,
    run: gtk::Button,
    arg: Option<adw::EntryRow>,
    pub view: Rc<StatusView>,
    pub dates: Option<Rc<DatesPanel>>,
}

fn arg_row(tab: Tab, placeholder: &str) -> adw::EntryRow {
    let row = adw::EntryRow::builder()
        .title(copy::arg_label(tab))
        .use_markup(false)
        .build();
    ui::row_placeholder(&row, placeholder);
    row
}

impl TabPage {
    fn update_args(&self, f: impl FnOnce(&mut RunArgs)) {
        let mut args = self.core.run_args(self.tab);
        f(&mut args);
        if let Err(e) = self.core.set_run_args(self.tab, args) {
            self.toaster.show(copy::error(self.tab, &e));
        }
        self.refresh_run();
    }

    fn start(&self) {
        if let Err(e) = self.core.start(self.tab) {
            self.toaster.show(copy::error(self.tab, &e));
        }
    }

    fn run(self: &Rc<Self>) {
        if self.core.blocker(self.tab).is_some() {
            return;
        }
        let Some(c) = self.core.confirmation(self.tab) else {
            return self.start();
        };
        let dialog = adw::AlertDialog::new(
            Some(copy::CONFIRM_CLEAN_TITLE),
            Some(copy::confirmation_message(&c)),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("ok", copy::CONFIRM_CLEAN_OK)]);
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if response == "ok"
                && let Some(page) = weak.upgrade()
            {
                page.start();
            }
        });
        dialog.present(Some(&self.content));
    }

    fn pick(self: &Rc<Self>, pick: Pick) {
        let dialog = gtk::FileDialog::new();
        dialog.set_modal(true);
        let current = self.core.run_args(self.tab).arg;
        let current = current.trim();
        if !current.is_empty() {
            let file = gio::File::for_path(current);
            match pick {
                Pick::Folder => dialog.set_initial_folder(Some(&file)),
                Pick::Archive(_) => dialog.set_initial_file(Some(&file)),
            }
        }
        if let Pick::Archive(extensions) = pick {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(copy::arg_label(self.tab)));
            for ext in extensions {
                filter.add_suffix(ext);
            }
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            dialog.set_default_filter(Some(&filter));
        }
        let weak = Rc::downgrade(self);
        let done = move |res: Result<gio::File, gtk::glib::Error>| {
            let (Some(page), Ok(file)) = (weak.upgrade(), res) else {
                return;
            };
            let (Some(path), Some(row)) = (file.path(), page.arg.as_ref()) else {
                return;
            };
            row.set_text(&path.display().to_string());
        };
        let parent = ui::window_of(&self.content);
        match pick {
            Pick::Folder => dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, done),
            Pick::Archive(_) => dialog.open(parent.as_ref(), gio::Cancellable::NONE, done),
        }
    }

    fn add_picker(self: &Rc<Self>, row: &adw::EntryRow, pick: Pick, icon: &str, tip: &str) {
        let b = gtk::Button::from_icon_name(icon);
        b.set_tooltip_text(Some(tip));
        b.set_valign(gtk::Align::Center);
        b.add_css_class("flat");
        let weak = Rc::downgrade(self);
        b.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.pick(pick);
            }
        });
        row.add_suffix(&b);
    }

    fn connect_arg(self: &Rc<Self>, group: &adw::PreferencesGroup, run_on_enter: bool) {
        let Some(row) = &self.arg else { return };
        row.set_text(&self.core.run_args(self.tab).arg);
        let weak = Rc::downgrade(self);
        row.connect_changed(move |r| {
            if let Some(page) = weak.upgrade() {
                let text = r.text().to_string();
                page.update_args(|a| a.arg = text);
            }
        });
        if run_on_enter {
            let weak = Rc::downgrade(self);
            row.connect_entry_activated(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.run();
                }
            });
        }
        group.add(row);
    }

    fn add_switch(
        self: &Rc<Self>,
        group: &adw::PreferencesGroup,
        title: &str,
        active: bool,
        set: fn(&mut RunArgs, bool),
    ) {
        let row = adw::SwitchRow::builder()
            .title(title)
            .use_markup(false)
            .active(active)
            .build();
        let weak = Rc::downgrade(self);
        row.connect_active_notify(move |r| {
            if let Some(page) = weak.upgrade() {
                let on = r.is_active();
                page.update_args(|a| set(a, on));
            }
        });
        group.add(&row);
    }

    fn add_dry_run(self: &Rc<Self>, group: &adw::PreferencesGroup) {
        let dry = self.core.run_args(self.tab).dry;
        self.add_switch(group, "Dry run", dry, |a, on| a.dry = on);
    }

    fn build(self: &Rc<Self>, group: &adw::PreferencesGroup) {
        match self.tab {
            Tab::ApplePhotos => {
                if let Some(row) = &self.arg {
                    self.add_picker(row, Pick::Folder, "folder-open-symbolic", "Choose…");
                }
                self.connect_arg(group, false);
                self.add_dry_run(group);
            }
            Tab::OneDrive | Tab::Dropbox => {
                self.connect_arg(group, false);
                self.add_dry_run(group);
            }
            Tab::GoogleTakeout => {
                if let Some(row) = &self.arg {
                    self.add_picker(row, Pick::Folder, "folder-open-symbolic", "Choose folder…");
                    self.add_picker(
                        row,
                        Pick::Archive(&["zip", "tgz", "gz"]),
                        "package-x-generic-symbolic",
                        "Choose archive…",
                    );
                }
                self.connect_arg(group, false);
                self.add_dry_run(group);
            }
            Tab::Consolidate => self.add_dry_run(group),
            Tab::Clean => {
                let option = self.core.run_args(self.tab).option;
                self.add_switch(
                    group,
                    "Also delete files the state doesn't reference",
                    option,
                    |a, on| a.option = on,
                );
                self.add_dry_run(group);
            }
            Tab::Dates => self.connect_arg(group, true),
        }
        if matches!(
            self.tab,
            Tab::ApplePhotos | Tab::OneDrive | Tab::Dropbox | Tab::GoogleTakeout
        ) {
            self.add_switch(
                group,
                "Export automatically",
                self.core.run_args(self.tab).automatic,
                |a, on| a.automatic = on,
            );
        }
        let weak = Rc::downgrade(self);
        self.run.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.run();
            }
        });
    }

    pub fn new(core: Arc<Core>, tab: Tab, toaster: Toaster) -> Rc<TabPage> {
        let view = StatusView::new(core.clone(), tab, toaster.clone());

        let run = gtk::Button::with_label(copy::run_label(tab, &core.run_args(tab)));
        run.add_css_class("suggested-action");
        let normal = hbox(6);
        normal.append(&view.clear);
        normal.append(&view.follow);
        normal.append(&view.filter);
        normal.append(&run);

        let group = adw::PreferencesGroup::new();
        group.set_description(Some(copy::hint(tab)));
        group.set_margin_start(12);
        group.set_margin_end(12);
        group.set_margin_top(12);
        group.set_margin_bottom(12);
        let arg = match tab {
            Tab::ApplePhotos => Some(arg_row(tab, "Photos library")),
            Tab::OneDrive => Some(arg_row(
                tab,
                "Folder under the drive root; empty for the whole drive",
            )),
            Tab::Dropbox => Some(arg_row(tab, "Folder in your Dropbox; empty for everything")),
            Tab::GoogleTakeout => Some(arg_row(
                tab,
                "Folder of archives, a single archive, or an extracted tree",
            )),
            Tab::Dates => Some(arg_row(
                tab,
                "Leave empty to list only assets whose date disagrees with the file",
            )),
            Tab::Consolidate | Tab::Clean => None,
        };

        let content = vbox(0);
        content.append(&group);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let (dates, header): (Option<Rc<DatesPanel>>, gtk::Widget) = match tab {
            Tab::Dates => {
                let panel = DatesPanel::new(core.clone(), &view, &normal, toaster.clone());
                content.append(&panel.root);
                let header = panel.header.clone().upcast();
                (Some(panel), header)
            }
            _ => {
                content.append(&view.root);
                (None, normal.upcast())
            }
        };

        let page = Rc::new(TabPage {
            core,
            toaster,
            tab,
            content,
            header,
            run,
            arg,
            view,
            dates,
        });
        page.build(&group);
        page
    }

    pub fn refresh_run(&self) {
        self.run
            .set_label(copy::run_label(self.tab, &self.core.run_args(self.tab)));
        let blocker = self.core.blocker(self.tab);
        self.run.set_sensitive(blocker.is_none());
        self.run
            .set_tooltip_text(blocker.map(|b| copy::blocker(self.tab, &b)).as_deref());
    }
}
