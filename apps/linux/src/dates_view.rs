use crate::copy;
use crate::lazy_list::LazyList;
use crate::status_view::StatusView;
use crate::ui::{self, Toaster, hbox, label, vbox};
use gtk::glib;
use gtk::prelude::*;
use photosink_bridge::Core;
use photosink_bridge::catalog::Tab;
use photosink_bridge::dates::{DateRow, DatesView};
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
    apply: gtk::Button,
    content: gtk::Stack,
    empty: adw::StatusPage,
    model: LazyList,
}

fn choice_button(
    core: &Arc<Core>,
    popover: &gtk::Popover,
    text: &str,
    f: impl Fn(&Core) + 'static,
) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("flat");
    let (core, popover) = (core.clone(), popover.clone());
    b.connect_clicked(move |_| {
        f(&core);
        popover.popdown();
    });
    b
}

impl DatesPanel {
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
        factory.connect_bind({
            let core = core.clone();
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
                    cell.append(&date_row(row, &core));
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
        for key in copy::DATE_KEYS {
            choose_box.append(&choice_button(
                &core,
                &popover,
                copy::date_key(key),
                move |core| core.dates_pick_all(key),
            ));
        }
        choose_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        choose_box.append(&choice_button(&core, &popover, "None", |core| {
            core.dates_clear()
        }));
        popover.set_child(Some(&choose_box));
        let choose = gtk::MenuButton::builder()
            .label("Choose All")
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
            apply,
            content,
            empty,
            model,
        });
        let core = panel.core.clone();
        panel.apply.connect_clicked(move |_| {
            if let Err(e) = core.apply_dates() {
                toaster.show(copy::error(Tab::Dates, &e));
            }
        });
        panel
    }

    pub fn refresh(&self) {
        let view: DatesView = self.core.dates_view();
        let page = if view.shown { "picker" } else { "status" };
        self.root.set_visible_child_name(page);
        self.header
            .set_visible_child_name(if view.shown { "picker" } else { "normal" });
        self.model.refresh();
        self.summary.set_text(&copy::dates_summary(
            view.scanning,
            view.scan.map(|p| (p.checked, p.total)),
            view.rows,
            view.query,
            view.chosen,
        ));
        self.progress.set_visible(view.scanning);
        self.progress.set_fraction(view.progress);
        self.choose
            .set_sensitive(view.can_pick && self.core.dates_len() > 0);
        self.apply
            .set_label(&copy::dates_apply(view.applying, view.chosen));
        self.apply.set_sensitive(view.can_apply);
        self.empty
            .set_title(copy::dates_empty(view.scanning, view.query));
        self.content
            .set_visible_child_name(if self.core.dates_len() == 0 {
                "empty"
            } else {
                "rows"
            });
        self.content.set_sensitive(view.can_pick);
    }
}

fn spin(max: f64, value: i32) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(0.0, max, 1.0);
    s.set_value(value as f64);
    s.set_wrap(true);
    s.set_numeric(true);
    s.set_orientation(gtk::Orientation::Vertical);
    s.connect_output(|s| {
        s.set_text(&format!("{:02}", s.value_as_int()));
        glib::Propagation::Stop
    });
    s
}

fn manual_picker(row: &DateRow, core: &Arc<Core>) -> gtk::MenuButton {
    let initial = row
        .manual
        .or(row.current)
        .and_then(|at| glib::DateTime::from_unix_utc(at).ok());
    let calendar = gtk::Calendar::new();
    if let Some(d) = &initial {
        calendar.select_day(d);
    }
    let hour = spin(23.0, initial.as_ref().map_or(0, |d| d.hour()));
    let minute = spin(59.0, initial.as_ref().map_or(0, |d| d.minute()));
    let time = hbox(6);
    time.set_halign(gtk::Align::Center);
    time.append(&hour);
    time.append(&gtk::Label::new(Some(":")));
    time.append(&minute);

    let clear = gtk::Button::with_label("Clear");
    clear.set_sensitive(row.manual.is_some());
    let set = gtk::Button::with_label("Set");
    set.add_css_class("suggested-action");
    let actions = hbox(6);
    actions.set_homogeneous(true);
    actions.append(&clear);
    actions.append(&set);

    let body = vbox(12);
    body.append(&calendar);
    body.append(&time);
    body.append(&actions);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&body));

    let face = hbox(6);
    match row.manual {
        Some(at) => face.append(&label(&copy::wall_clock(at), &["monospace"])),
        None => face.append(&gtk::Image::from_icon_name("x-office-calendar-symbolic")),
    }
    if row.manual_chosen {
        face.append(&gtk::Image::from_icon_name("object-select-symbolic"));
    }
    let button = gtk::MenuButton::builder()
        .child(&face)
        .popover(&popover)
        .valign(gtk::Align::Center)
        .tooltip_text("Set date")
        .build();

    let (c, uuid, p) = (core.clone(), row.uuid.clone(), popover.clone());
    clear.connect_clicked(move |_| {
        p.popdown();
        c.dates_manual(uuid.clone(), None);
    });
    let (c, uuid) = (core.clone(), row.uuid.clone());
    set.connect_clicked(move |_| {
        let day = calendar.date();
        let at = glib::DateTime::from_utc(
            day.year(),
            day.month(),
            day.day_of_month(),
            hour.value_as_int(),
            minute.value_as_int(),
            0.0,
        );
        popover.popdown();
        if let Ok(at) = at {
            c.dates_manual(uuid.clone(), Some(at.to_unix()));
        }
    });
    button
}

fn date_row(row: &DateRow, core: &Arc<Core>) -> gtk::Widget {
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

    let current = label(&copy::current_date(row.current), &["monospace"]);
    current.set_valign(gtk::Align::Center);
    b.append(&current);

    let options = hbox(4);
    for o in &row.options {
        let content = vbox(0);
        content.append(&label(copy::date_key(o.key), &["caption", "dim-label"]));
        let value = o.value.map_or_else(|| "—".to_owned(), copy::wall_clock);
        content.append(&label(&value, &["monospace"]));
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&content));
        button.set_active(o.chosen);
        button.set_sensitive(o.value.is_some());
        if o.same_day {
            button.add_css_class("dim-label");
        }
        let (core, uuid, key) = (core.clone(), row.uuid.clone(), o.key);
        button.connect_clicked(move |_| core.dates_pick(uuid.clone(), key));
        options.append(&button);
    }
    b.append(&options);
    b.append(&manual_picker(row, core));
    b.upcast()
}
