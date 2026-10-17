use crate::lazy_list::LazyList;
use crate::status_view::StatusView;
use crate::ui::{self, Toaster, hbox, label, vbox};
use gtk::glib;
use gtk::prelude::*;
use photosink_bridge::Core;
use photosink_bridge::dates::{DateKey, DateRow, DatesView};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// The Dates tab's list: its status rows, or the picker while
/// `DatesView::shown`. `header` swaps the tab's header controls for the
/// picker's to match.
pub struct DatesPanel {
    core: Arc<Core>,
    pub root: gtk::Stack,
    pub header: gtk::Stack,
    summary: gtk::Label,
    progress: gtk::ProgressBar,
    choose: gtk::MenuButton,
    choose_box: gtk::Box,
    choose_keys: RefCell<Option<Vec<DateKey>>>,
    apply: gtk::Button,
    content: gtk::Stack,
    empty: adw::StatusPage,
    model: LazyList,
    placeholder: Rc<RefCell<String>>,
}

impl DatesPanel {
    fn choice_button(&self, text: &str, f: impl Fn(&Core) + 'static) -> gtk::Button {
        let b = gtk::Button::with_label(text);
        b.add_css_class("flat");
        let (core, popover) = (self.core.clone(), self.choose.popover());
        b.connect_clicked(move |_| {
            f(&core);
            if let Some(p) = &popover {
                p.popdown();
            }
        });
        b
    }

    fn refresh_choose(&self, can_pick: bool) {
        let first = self.core.dates_rows(0, 1);
        let options: Vec<(DateKey, String)> = first
            .first()
            .map(|r| r.options.iter().map(|o| (o.key, o.label.clone())).collect())
            .unwrap_or_default();
        let keys: Vec<DateKey> = options.iter().map(|(k, _)| *k).collect();
        if self.choose_keys.borrow().as_ref() != Some(&keys) {
            ui::clear(&self.choose_box);
            for (key, text) in options {
                self.choose_box
                    .append(&self.choice_button(&text, move |core| core.dates_pick_all(key)));
            }
            self.choose_box
                .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            self.choose_box
                .append(&self.choice_button("None", |core| core.dates_clear()));
            self.choose_keys.replace(Some(keys));
        }
        self.choose
            .set_sensitive(can_pick && self.core.dates_len() > 0);
    }

    pub fn new(
        core: Arc<Core>,
        status: &Rc<StatusView>,
        normal: &gtk::Box,
        toaster: Toaster,
    ) -> Rc<DatesPanel> {
        let model = LazyList::new(
            {
                let core = core.clone();
                move || core.dates_len()
            },
            {
                let core = core.clone();
                move |start, count| core.dates_rows(start, count)
            },
        );
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let cell = hbox(0);
            cell.set_margin_start(12);
            cell.set_margin_end(12);
            cell.set_margin_top(4);
            cell.set_margin_bottom(4);
            item.set_child(Some(&cell));
            item.set_activatable(false);
        });
        let placeholder = Rc::new(RefCell::new(String::new()));
        factory.connect_bind({
            let core = core.clone();
            let placeholder = placeholder.clone();
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(cell) = item.child().and_downcast::<gtk::Box>() else {
                    return;
                };
                ui::clear(&cell);
                let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = obj.borrow::<Option<DateRow>>();
                if let Some(row) = row.as_ref() {
                    cell.append(&date_row(row, &core, &placeholder.borrow()));
                }
            }
        });
        let list = gtk::ListView::new(
            Some(gtk::NoSelection::new(Some(model.clone()))),
            Some(factory),
        );
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let empty = adw::StatusPage::new();
        empty.add_css_class("compact");
        empty.set_vexpand(true);
        let content = gtk::Stack::new();
        content.add_named(&scroller, Some("rows"));
        content.add_named(&empty, Some("empty"));

        let summary = label("", &["dim-label"]);
        let progress = gtk::ProgressBar::new();
        progress.set_valign(gtk::Align::Center);
        progress.set_hexpand(true);
        let bar = hbox(12);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.append(&summary);
        bar.append(&progress);

        let picker = vbox(0);
        picker.append(&bar);
        picker.append(&content);

        let root = gtk::Stack::new();
        root.set_vexpand(true);
        root.add_named(&status.root, Some("status"));
        root.add_named(&picker, Some("picker"));
        root.set_visible_child_name("status");

        let choose_box = vbox(0);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&choose_box));
        let choose = gtk::MenuButton::builder()
            .label("Choose all")
            .popover(&popover)
            .build();
        let apply = gtk::Button::new();
        apply.add_css_class("suggested-action");
        let picker_header = hbox(6);
        picker_header.append(&choose);
        picker_header.append(&apply);
        let header = gtk::Stack::new();
        header.set_hhomogeneous(false);
        header.add_named(normal, Some("normal"));
        header.add_named(&picker_header, Some("picker"));
        header.set_visible_child_name("normal");

        let panel = Rc::new(DatesPanel {
            core,
            root,
            header,
            summary,
            progress,
            choose,
            choose_box,
            choose_keys: RefCell::new(None),
            apply,
            content,
            empty,
            model,
            placeholder,
        });
        let core = panel.core.clone();
        panel.apply.connect_clicked(move |_| {
            if let Err(e) = core.apply_dates() {
                toaster.show(e.to_string());
            }
        });
        panel
    }

    pub fn refresh(&self) {
        let view: DatesView = self.core.dates_view();
        self.placeholder.replace(view.manual_placeholder.clone());
        let page = if view.shown { "picker" } else { "status" };
        self.root.set_visible_child_name(page);
        self.header
            .set_visible_child_name(if view.shown { "picker" } else { "normal" });
        self.model.refresh();
        self.summary.set_text(&view.summary);
        self.progress.set_visible(view.scanning);
        self.progress.set_fraction(view.progress);
        self.refresh_choose(view.can_pick);
        self.apply.set_label(&view.apply_label);
        self.apply.set_sensitive(view.can_apply);
        self.empty.set_title(&view.empty_text);
        self.content
            .set_visible_child_name(if self.core.dates_len() == 0 {
                "empty"
            } else {
                "rows"
            });
        self.content.set_sensitive(view.can_pick);
    }
}

fn date_row(row: &DateRow, core: &Arc<Core>, placeholder: &str) -> gtk::Widget {
    let b = hbox(12);
    let name = vbox(0);
    name.set_hexpand(true);
    name.set_valign(gtk::Align::Center);
    let folder = label(&row.folder, &["dim-label", "caption"]);
    folder.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    let stem_line = hbox(4);
    if row.video {
        stem_line.append(&gtk::Image::from_icon_name("video-x-generic-symbolic"));
    }
    let stem = label(&row.stem, &[]);
    stem.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    stem_line.append(&stem);
    name.append(&folder);
    name.append(&stem_line);
    name.set_tooltip_text(Some(&row.uuid));
    b.append(&name);

    let current = label(&row.current, &["monospace"]);
    current.set_valign(gtk::Align::Center);
    b.append(&current);

    let options = hbox(4);
    for o in &row.options {
        let content = vbox(0);
        content.append(&label(&o.label, &["caption", "dim-label"]));
        let value = if o.value.is_empty() { "—" } else { &o.value };
        content.append(&label(value, &["monospace"]));
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&content));
        button.set_active(o.chosen);
        button.set_sensitive(!o.value.is_empty());
        if o.same_day {
            button.add_css_class("dim-label");
        }
        let (core, uuid, key) = (core.clone(), row.uuid.clone(), o.key);
        button.connect_clicked(move |_| core.dates_pick(uuid.clone(), key));
        options.append(&button);
    }
    b.append(&options);

    let manual = vbox(2);
    manual.set_valign(gtk::Align::Center);
    let entry = gtk::Entry::new();
    entry.set_text(&row.manual);
    entry.set_width_chars(24);
    entry.add_css_class("monospace");
    ui::set_class(&entry, "error", !row.manual_error.is_empty());
    if row.manual_chosen {
        entry.set_secondary_icon_name(Some("object-select-symbolic"));
    }
    entry.set_placeholder_text(Some(placeholder));
    let commit = {
        let (core, uuid, entry, last) = (
            core.clone(),
            row.uuid.clone(),
            entry.clone(),
            row.manual.clone(),
        );
        move || {
            let text = entry.text().to_string();
            if text != last {
                core.dates_manual(uuid.clone(), text);
            }
        }
    };
    let on_activate = commit.clone();
    entry.connect_activate(move |_| on_activate());
    ui::on_focus_out(&entry, commit);
    manual.append(&entry);
    if !row.manual_error.is_empty() {
        let err = label(&row.manual_error, &["error", "caption"]);
        err.set_wrap(true);
        err.set_max_width_chars(30);
        manual.append(&err);
    }
    b.append(&manual);
    b.upcast()
}
