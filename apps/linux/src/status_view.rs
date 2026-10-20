use crate::copy;
use crate::lazy_list::LazyList;
use crate::ui::{self, Toaster, hbox, label, vbox};
use gtk::glib;
use gtk::prelude::*;
use photosink_bridge::Core;
use photosink_bridge::catalog::Tab;
use photosink_bridge::grid::{Chip, CleanOutcome, Detail, Row, RowState, SignIn};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// One tab's rows in a virtualised list, plus the header controls that act
/// on them: the Filter menu, Follow and Clear.
pub struct StatusView {
    tab: Tab,
    core: Arc<Core>,
    pub root: gtk::ScrolledWindow,
    pub filter: gtk::MenuButton,
    pub follow: gtk::ToggleButton,
    pub clear: gtk::Button,
    chips_box: gtk::Box,
    chips: RefCell<Vec<(RowState, gtk::CheckButton)>>,
    syncing: Cell<bool>,
    show_all: gtk::Button,
    hidden: gtk::Label,
    model: LazyList,
    list: gtk::ListView,
}

impl StatusView {
    fn chip_row(self: &Rc<Self>, state: RowState) -> (gtk::Box, gtk::CheckButton) {
        let check = gtk::CheckButton::new();
        check.set_hexpand(true);
        let weak = Rc::downgrade(self);
        check.connect_toggled(move |_| {
            let Some(view) = weak.upgrade() else { return };
            if view.syncing.get() {
                return;
            }
            view.core.toggle_chip(view.tab, state, false);
            view.refresh_chips();
        });
        let only = gtk::Button::with_label("Show Only");
        only.add_css_class("flat");
        let weak = Rc::downgrade(self);
        only.connect_clicked(move |_| {
            let Some(view) = weak.upgrade() else { return };
            view.core.toggle_chip(view.tab, state, true);
            view.refresh_chips();
        });
        let row = hbox(12);
        row.append(&check);
        row.append(&only);
        (row, check)
    }

    fn refresh_chips(self: &Rc<Self>) {
        let chips: Vec<Chip> = self.core.chips(self.tab);
        self.filter.set_sensitive(!chips.is_empty());
        let states: Vec<RowState> = chips.iter().map(|c| c.state).collect();
        let same = self
            .chips
            .borrow()
            .iter()
            .map(|(s, _)| *s)
            .eq(states.iter().copied());
        if !same {
            ui::clear(&self.chips_box);
            let mut checks = Vec::new();
            for s in &states {
                let (row, check) = self.chip_row(*s);
                self.chips_box.append(&row);
                checks.push((*s, check));
            }
            self.chips.replace(checks);
        }
        self.syncing.set(true);
        for (chip, (_, check)) in chips.iter().zip(self.chips.borrow().iter()) {
            check.set_label(Some(&format!("{}  {}", copy::chip(chip.state), chip.count)));
            check.set_active(!chip.hidden);
        }
        self.syncing.set(false);
        let any_hidden = chips.iter().any(|c| c.hidden);
        self.show_all.set_sensitive(any_hidden);
        self.hidden.set_visible(any_hidden);
        self.hidden
            .set_label(&format!("{} hidden", self.core.hidden_rows(self.tab)));
        ui::set_class(&self.filter, "accent", any_hidden);
    }

    pub fn new(core: Arc<Core>, tab: Tab, toaster: Toaster) -> Rc<StatusView> {
        let model = LazyList::new(
            {
                let core = core.clone();
                move || core.grid_len(tab)
            },
            {
                let core = core.clone();
                move |start, count| core.grid_rows(tab, start, count)
            },
        );
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let cell = hbox(0);
            cell.set_margin_start(12);
            cell.set_margin_end(12);
            cell.set_margin_top(3);
            cell.set_margin_bottom(3);
            item.set_child(Some(&cell));
            item.set_activatable(false);
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let Some(cell) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            ui::clear(&cell);
            let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let row = obj.borrow::<Option<Row>>();
            if let Some(row) = row.as_ref() {
                cell.append(&row_widget(row, &toaster));
            }
        });
        let list = gtk::ListView::new(
            Some(gtk::NoSelection::new(Some(model.clone()))),
            Some(factory),
        );
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let chips_box = vbox(2);
        let show_all = gtk::Button::with_label("Show All");
        let hidden = label("", &["dim-label", "caption"]);
        let footer = hbox(12);
        hidden.set_hexpand(true);
        footer.append(&hidden);
        footer.append(&show_all);
        let menu = vbox(6);
        menu.set_margin_start(6);
        menu.set_margin_end(6);
        menu.set_margin_top(6);
        menu.set_margin_bottom(6);
        menu.append(&chips_box);
        menu.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        menu.append(&footer);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu));
        let filter = gtk::MenuButton::builder()
            .label("Filter")
            .popover(&popover)
            .sensitive(false)
            .build();

        let follow = gtk::ToggleButton::builder()
            .icon_name("go-bottom-symbolic")
            .tooltip_text("Follow")
            .active(true)
            .build();
        let clear = gtk::Button::builder()
            .icon_name("edit-clear-all-symbolic")
            .tooltip_text("Clear")
            .sensitive(false)
            .build();

        let view = Rc::new(StatusView {
            tab,
            core,
            root,
            filter,
            follow,
            clear,
            chips_box,
            chips: RefCell::new(Vec::new()),
            syncing: Cell::new(false),
            show_all,
            hidden,
            model,
            list,
        });
        let weak = Rc::downgrade(&view);
        view.show_all.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.core.show_all(view.tab);
                view.refresh_chips();
            }
        });
        let weak = Rc::downgrade(&view);
        view.clear.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.core.clear(view.tab);
            }
        });
        let weak = Rc::downgrade(&view);
        view.follow.connect_toggled(move |b| {
            if let Some(view) = weak.upgrade()
                && b.is_active()
            {
                view.scroll_to_end();
            }
        });
        view
    }

    pub fn refresh(self: &Rc<Self>) {
        let added = self.model.refresh();
        self.refresh_chips();
        if self.follow.is_active() && added {
            self.scroll_to_end();
        }
        self.refresh_clear();
    }

    pub fn refresh_clear(&self) {
        let running = self.core.running();
        self.clear
            .set_sensitive(running != Some(self.tab) && self.core.grid_len(self.tab) > 0);
    }

    pub fn scroll_to_end(&self) {
        let n = self.model.n_items();
        if n > 0 {
            self.list.scroll_to(n - 1, gtk::ListScrollFlags::NONE, None);
        }
    }
}

fn detail_label(text: &str, tooltip: &str, classes: &[&str]) -> gtk::Label {
    let l = label(text, classes);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_tooltip_text(Some(tooltip));
    l
}

fn details_box(details: &[Detail]) -> gtk::Box {
    let b = hbox(12);
    b.set_hexpand(true);
    for d in details {
        match d {
            Detail::Placed { path } => b.append(&detail_label(&format!("→ {path}"), path, &[])),
            Detail::Duplicate { existing } => b.append(&detail_label(
                &format!("= {existing}"),
                existing,
                &["dim-label"],
            )),
            Detail::Metadata { path } => {
                b.append(&detail_label(&format!("xmp {path}"), path, &["dim-label"]))
            }
            Detail::Redated { from, to } => {
                let from = from.map_or_else(|| "none".to_owned(), copy::wall_clock);
                b.append(&detail_label(&from, &from, &["dim-label"]));
                let to = copy::wall_clock(*to);
                b.append(&detail_label(&format!("→ {to}"), &to, &[]));
            }
            Detail::Error { subject, message } => {
                let text = copy::subject_message(subject.as_deref(), message);
                b.append(&detail_label(&text, &text, &["error"]));
            }
            Detail::NothingNew => {
                let text = copy::NOTHING_NEW;
                b.append(&detail_label(text, text, &["dim-label"]));
            }
        }
    }
    b
}

fn title_label(text: &str, failed: bool) -> gtk::Label {
    let l = label(text, &[]);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_tooltip_text(Some(text));
    ui::set_class(&l, "error", failed);
    l
}

fn log_buttons(path: &str, toaster: &Toaster) -> gtk::Box {
    let open = gtk::Button::with_label("Log File");
    open.add_css_class("flat");
    open.set_tooltip_text(Some(path));
    let (toaster_open, path_open) = (toaster.clone(), path.to_owned());
    open.connect_clicked(move |b| ui::launch(b, &toaster_open, &path_open, false));
    let folder = gtk::Button::from_icon_name("folder-open-symbolic");
    folder.add_css_class("flat");
    folder.set_tooltip_text(Some("Open containing folder"));
    let (toaster, path) = (toaster.clone(), path.to_owned());
    folder.connect_clicked(move |b| ui::launch(b, &toaster, &path, true));
    let b = hbox(0);
    b.append(&open);
    b.append(&folder);
    b
}

fn row_widget(row: &Row, toaster: &Toaster) -> gtk::Widget {
    match row {
        Row::Run {
            title,
            facts,
            log_path,
        } => {
            let b = hbox(16);
            b.set_margin_top(6);
            let title = title_label(&copy::run_title(title.as_ref()), false);
            title.add_css_class("heading");
            b.append(&title);
            for f in facts {
                let (value, text, tooltip) = copy::fact(f);
                let l = gtk::Label::new(None);
                if value.is_empty() {
                    l.set_text(&text);
                } else {
                    l.set_markup(&format!(
                        "<b>{}</b> {}",
                        glib::markup_escape_text(&value),
                        glib::markup_escape_text(&text)
                    ));
                }
                l.add_css_class("dim-label");
                l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                if !tooltip.is_empty() {
                    l.set_tooltip_text(Some(&tooltip));
                }
                b.append(&l);
            }
            if !log_path.is_empty() {
                let spacer = hbox(0);
                spacer.set_hexpand(true);
                b.append(&spacer);
                b.append(&log_buttons(log_path, toaster));
            }
            b.upcast()
        }
        Row::Asset {
            position,
            name,
            details,
            failed,
            ..
        } => {
            let b = hbox(12);
            let pos = position.map_or_else(String::new, |p| format!("{}/{}", p.n, p.total));
            let pos = label(&pos, &["dim-label", "monospace", "caption"]);
            pos.set_xalign(1.0);
            pos.set_width_chars(11);
            b.append(&pos);
            b.append(&title_label(name.as_deref().unwrap_or("…"), *failed));
            b.append(&details_box(details));
            b.upcast()
        }
        Row::Clean {
            path,
            reason,
            outcome,
            failed,
        } => {
            let b = hbox(12);
            let tag = label(copy::clean_reason(*reason), &["dim-label", "caption"]);
            tag.set_width_chars(14);
            b.append(&tag);
            b.append(&title_label(path, *failed));
            let text = copy::clean_outcome(*outcome);
            let detail = match outcome {
                CleanOutcome::Removed => detail_label(&format!("→ {text}"), text, &[]),
                CleanOutcome::Kept | CleanOutcome::WouldRemove => {
                    detail_label(text, text, &["dim-label"])
                }
            };
            detail.set_hexpand(true);
            b.append(&detail);
            b.upcast()
        }
        Row::Error { subject, message } => {
            let l = label(
                &copy::subject_message(subject.as_deref(), message),
                &["error"],
            );
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_selectable(true);
            l.set_hexpand(true);
            l.upcast()
        }
        Row::SignIn { prompt, url } => {
            let b = vbox(6);
            b.set_margin_top(12);
            b.set_margin_bottom(12);
            b.set_hexpand(true);
            b.append(&label(copy::sign_in(prompt), &["dim-label"]));
            if let SignIn::DeviceCode { code } = prompt {
                let code = gtk::Label::new(Some(code));
                code.add_css_class("title-1");
                code.add_css_class("monospace");
                code.set_selectable(true);
                b.append(&code);
            }
            let link = gtk::LinkButton::with_label(url, url);
            link.set_halign(gtk::Align::Center);
            b.append(&link);
            b.upcast()
        }
    }
}
