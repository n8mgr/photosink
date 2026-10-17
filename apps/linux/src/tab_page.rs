use crate::dates_view::DatesPanel;
use crate::status_view::StatusView;
use crate::ui::{self, Toaster, hbox, vbox};
use adw::prelude::*;
use gtk::gio;
use photosink_bridge::catalog::{ArgSpec, PickerKind, RunArgs, Tab, TabInfo};
use photosink_bridge::{Core, CoreError};
use std::rc::Rc;
use std::sync::Arc;

/// One tab: its argument group over its rows, and its header-bar controls.
pub struct TabPage {
    core: Arc<Core>,
    toaster: Toaster,
    pub info: TabInfo,
    pub content: gtk::Box,
    pub header: gtk::Widget,
    run: gtk::Button,
    arg: Option<adw::EntryRow>,
    pub view: Rc<StatusView>,
    pub dates: Option<Rc<DatesPanel>>,
}

impl TabPage {
    fn tab(&self) -> Tab {
        self.info.tab
    }

    fn update_args(&self, f: impl FnOnce(&mut RunArgs)) {
        let mut args = self.core.run_args(self.tab());
        f(&mut args);
        self.core.set_run_args(self.tab(), args);
        self.refresh_run();
    }

    fn start(&self) {
        if let Err(CoreError::Failed(e)) = self.core.start(self.tab()) {
            self.toaster.show(e);
        }
    }

    fn run(self: &Rc<Self>) {
        if self.core.blocker(self.tab()).is_some() {
            return;
        }
        let Some(c) = self.core.confirmation(self.tab()) else {
            return self.start();
        };
        let dialog = adw::AlertDialog::new(Some(&c.title), Some(&c.message));
        dialog.add_responses(&[("cancel", "Cancel"), ("ok", &c.ok)]);
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

    fn pick(self: &Rc<Self>, spec: &ArgSpec, folder: bool) {
        let dialog = gtk::FileDialog::new();
        dialog.set_modal(true);
        let current = self.core.run_args(self.tab()).arg;
        let current = current.trim();
        if !current.is_empty() {
            let file = gio::File::for_path(current);
            if folder {
                dialog.set_initial_folder(Some(&file));
            } else {
                dialog.set_initial_file(Some(&file));
            }
        }
        if !folder && !spec.extensions.is_empty() {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&spec.label));
            for ext in &spec.extensions {
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
        if folder {
            dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, done);
        } else {
            dialog.open(parent.as_ref(), gio::Cancellable::NONE, done);
        }
    }

    fn picker_button(
        self: &Rc<Self>,
        spec: &ArgSpec,
        folder: bool,
        icon: &str,
        tip: &str,
    ) -> gtk::Button {
        let b = gtk::Button::from_icon_name(icon);
        b.set_tooltip_text(Some(tip));
        b.set_valign(gtk::Align::Center);
        b.add_css_class("flat");
        let (weak, spec) = (Rc::downgrade(self), spec.clone());
        b.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.pick(&spec, folder);
            }
        });
        b
    }

    fn connect(self: &Rc<Self>, group: &adw::PreferencesGroup) {
        let tab = self.tab();
        let args = self.core.run_args(tab);
        if let (Some(spec), Some(row)) = (&self.info.arg, &self.arg) {
            row.set_text(&args.arg);
            let weak = Rc::downgrade(self);
            row.connect_changed(move |r| {
                if let Some(page) = weak.upgrade() {
                    let text = r.text().to_string();
                    page.update_args(|a| a.arg = text);
                }
            });
            if spec.run_on_enter {
                let weak = Rc::downgrade(self);
                row.connect_entry_activated(move |_| {
                    if let Some(page) = weak.upgrade() {
                        page.run();
                    }
                });
            }
            match spec.picker {
                PickerKind::None => {}
                PickerKind::Library => {
                    row.add_suffix(&self.picker_button(
                        spec,
                        true,
                        "folder-open-symbolic",
                        "Choose…",
                    ));
                }
                PickerKind::FolderOrArchive => {
                    row.add_suffix(&self.picker_button(
                        spec,
                        true,
                        "folder-open-symbolic",
                        "Choose folder…",
                    ));
                    row.add_suffix(&self.picker_button(
                        spec,
                        false,
                        "package-x-generic-symbolic",
                        "Choose archive…",
                    ));
                }
            }
            group.add(row);
        }
        if let Some(text) = &self.info.option {
            let row = adw::SwitchRow::builder()
                .title(text)
                .use_markup(false)
                .active(args.option)
                .build();
            let weak = Rc::downgrade(self);
            row.connect_active_notify(move |r| {
                if let Some(page) = weak.upgrade() {
                    let on = r.is_active();
                    page.update_args(|a| a.option = on);
                }
            });
            group.add(&row);
        }
        let weak = Rc::downgrade(self);
        self.run.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.run();
            }
        });
    }

    pub fn new(core: Arc<Core>, info: TabInfo, toaster: Toaster) -> Rc<TabPage> {
        let tab = info.tab;
        let args = core.run_args(tab);
        let view = StatusView::new(core.clone(), tab, toaster.clone());

        let run = gtk::Button::with_label(&core.run_label(tab));
        run.add_css_class("suggested-action");
        let normal = hbox(6);
        normal.append(&view.clear);
        normal.append(&view.follow);
        normal.append(&view.filter);
        let dry = info.dry_run.then(|| {
            let dry = gtk::ToggleButton::with_label("Dry run");
            dry.set_active(args.dry);
            normal.append(&dry);
            dry
        });
        normal.append(&run);

        let group = adw::PreferencesGroup::new();
        group.set_description(Some(&info.hint));
        group.set_margin_start(12);
        group.set_margin_end(12);
        group.set_margin_top(12);
        group.set_margin_bottom(12);
        let arg = info.arg.as_ref().map(|spec| {
            let row = adw::EntryRow::builder()
                .title(&spec.label)
                .use_markup(false)
                .build();
            ui::row_placeholder(&row, &spec.placeholder);
            row
        });

        let content = vbox(0);
        content.append(&group);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let (dates, header): (Option<Rc<DatesPanel>>, gtk::Widget) = if tab == Tab::Dates {
            let panel = DatesPanel::new(core.clone(), &view, &normal, toaster.clone());
            content.append(&panel.root);
            let header = panel.header.clone().upcast();
            (Some(panel), header)
        } else {
            content.append(&view.root);
            (None, normal.upcast())
        };

        let page = Rc::new(TabPage {
            core,
            toaster,
            info,
            content,
            header,
            run,
            arg,
            view,
            dates,
        });
        page.connect(&group);
        if let Some(dry) = dry {
            let weak = Rc::downgrade(&page);
            dry.connect_toggled(move |b| {
                if let Some(page) = weak.upgrade() {
                    let on = b.is_active();
                    page.update_args(|a| a.dry = on);
                }
            });
        }
        page
    }

    pub fn refresh_run(&self) {
        let tab = self.tab();
        self.run.set_label(&self.core.run_label(tab));
        let blocker = self.core.blocker(tab);
        self.run.set_sensitive(blocker.is_none());
        self.run.set_tooltip_text(blocker.as_deref());
    }
}
