use crate::copy;
use crate::ui::{self, Toaster, hbox, label, vbox};
use adw::prelude::*;
use gtk::{gio, glib};
use photosink_bridge::Core;
use photosink_bridge::settings::{Media, Metadata, Settings, TemplateKind, ZoneMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const PREVIEW_DELAY: Duration = Duration::from_millis(150);

struct Template {
    kind: TemplateKind,
    row: adw::EntryRow,
    line: gtk::Label,
    note: gtk::Label,
    problem: gtk::Label,
    fields: Vec<(String, String, gtk::Button)>,
    popover: gtk::Popover,
}

struct DateEntry {
    row: adw::EntryRow,
    calendar: gtk::Calendar,
    popover: gtk::Popover,
}

/// The Preferences dialog: the export folder and the Layout, Dates, Filter
/// and Files pages.
pub struct SettingsPanel {
    core: Arc<Core>,
    toaster: Toaster,
    window_toaster: Toaster,
    pub root: adw::PreferencesDialog,
    state: RefCell<Option<Settings>>,
    saved: RefCell<Option<Settings>>,
    loading: Cell<bool>,
    preview_gen: Cell<u32>,
    on_saved: RefCell<Option<Box<dyn Fn()>>>,
    export_row: adw::ActionRow,
    export_button: gtk::Button,
    layout: adw::PreferencesPage,
    dates: adw::PreferencesPage,
    filter: adw::PreferencesPage,
    files: adw::PreferencesPage,
    folder: Template,
    filename: Template,
    date_source: adw::ComboRow,
    zone: adw::ComboRow,
    offset: adw::EntryRow,
    media: Vec<(Media, gtk::ToggleButton)>,
    favorites: adw::SwitchRow,
    from: DateEntry,
    to: DateEntry,
    albums: adw::EntryRow,
    keywords: adw::EntryRow,
    filter_group: adw::PreferencesGroup,
    edits: Vec<(bool, gtk::ToggleButton)>,
    metadata: Vec<(Metadata, gtk::ToggleButton)>,
    automatic: adw::SwitchRow,
    interval: adw::SpinRow,
    keep_running: adw::SwitchRow,
    open_at_login: adw::SwitchRow,
}

impl Template {
    fn new(core: &Core, kind: TemplateKind, title: &str, placeholder: &str) -> Template {
        let row = adw::EntryRow::builder()
            .title(title)
            .use_markup(false)
            .show_apply_button(true)
            .build();
        ui::row_placeholder(&row, placeholder);
        row.add_css_class("monospace");

        let groups = vbox(12);
        groups.set_margin_start(6);
        groups.set_margin_end(6);
        groups.set_margin_top(6);
        groups.set_margin_bottom(6);
        let mut fields = Vec::new();
        let all = core.fields();
        for g in copy::FIELD_GROUPS {
            let members: Vec<_> = all.iter().filter(|f| f.group == g).collect();
            if members.is_empty() {
                continue;
            }
            let group = vbox(4);
            group.append(&label(copy::field_group(g), &["heading"]));
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(false)
                .row_spacing(4)
                .column_spacing(4)
                .max_children_per_line(6)
                .build();
            for f in members {
                let b = gtk::Button::with_label(&f.name);
                b.set_tooltip_text(Some(copy::field_help(&f.name)));
                b.add_css_class("monospace");
                flow.insert(&b, -1);
                fields.push((f.name.clone(), f.token.clone(), b));
            }
            group.append(&flow);
            groups.append(&group);
        }
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(420)
            .min_content_width(460)
            .child(&groups)
            .build();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&scroller));
        let picker = gtk::MenuButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Insert field")
            .valign(gtk::Align::Center)
            .popover(&popover)
            .build();
        picker.add_css_class("flat");
        row.add_suffix(&picker);

        Template {
            kind,
            row,
            line: label("", &["monospace", "caption"]),
            note: label("", &["dim-label", "caption"]),
            problem: label("", &["error", "caption"]),
            fields,
            popover,
        }
    }

    fn preview_row(&self) -> gtk::ListBoxRow {
        let b = vbox(2);
        b.set_margin_start(12);
        b.set_margin_end(12);
        b.set_margin_top(6);
        b.set_margin_bottom(6);
        self.line.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        self.problem.set_wrap(true);
        b.append(&self.line);
        b.append(&self.note);
        b.append(&self.problem);
        gtk::ListBoxRow::builder()
            .activatable(false)
            .selectable(false)
            .child(&b)
            .build()
    }
}

impl DateEntry {
    fn new(title: &str) -> DateEntry {
        let row = adw::EntryRow::builder()
            .title(title)
            .use_markup(false)
            .show_apply_button(true)
            .build();
        ui::row_placeholder(&row, "YYYY-MM-DD");
        let calendar = gtk::Calendar::new();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&calendar));
        let button = gtk::MenuButton::builder()
            .icon_name("x-office-calendar-symbolic")
            .valign(gtk::Align::Center)
            .popover(&popover)
            .build();
        button.add_css_class("flat");
        row.add_suffix(&button);
        DateEntry {
            row,
            calendar,
            popover,
        }
    }
}

fn segmented<T: Copy>(items: &[(T, &str)]) -> (gtk::Box, Vec<(T, gtk::ToggleButton)>) {
    let b = hbox(0);
    b.add_css_class("linked");
    b.set_valign(gtk::Align::Center);
    let mut out: Vec<(T, gtk::ToggleButton)> = Vec::new();
    for (value, text) in items {
        let t = gtk::ToggleButton::with_label(text);
        if let Some((_, first)) = out.first() {
            t.set_group(Some(first));
        }
        b.append(&t);
        out.push((*value, t));
    }
    (b, out)
}

fn combo(title: &str, subtitle: &str, labels: &[&str]) -> adw::ComboRow {
    adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .model(&gtk::StringList::new(labels))
        .build()
}

fn list_row(title: &str, tooltip: &str) -> adw::EntryRow {
    let row = adw::EntryRow::builder()
        .title(title)
        .use_markup(false)
        .show_apply_button(true)
        .tooltip_text(tooltip)
        .build();
    ui::row_placeholder(&row, tooltip);
    row
}

fn page(title: &str, icon: &str) -> adw::PreferencesPage {
    adw::PreferencesPage::builder()
        .title(title)
        .icon_name(icon)
        .build()
}

fn group_of(rows: &[&gtk::Widget]) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    for r in rows {
        g.add(*r);
    }
    g
}

fn parse_day(text: &str) -> Option<glib::DateTime> {
    let mut parts = text.trim().splitn(3, '-').map(|p| p.parse::<i32>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    glib::DateTime::from_local(y, m, d, 0, 0, 0.0).ok()
}

fn on_commit(row: &adw::EntryRow, f: impl Fn() + 'static) {
    let f = Rc::new(f);
    let g = f.clone();
    row.connect_apply(move |_| g());
    let g = f.clone();
    row.connect_entry_activated(move |_| g());
    ui::on_focus_out(row, move || f());
}

impl SettingsPanel {
    fn current(&self) -> Option<Settings> {
        self.state.borrow().clone()
    }

    /// Applies `f` to the settings and saves them. On failure the widgets keep
    /// the edit, `mark` gets the error style and the stored settings revert.
    fn edit_marked(&self, mark: Option<&gtk::Widget>, f: impl FnOnce(&mut Settings)) {
        if self.loading.get() {
            return;
        }
        let Some(mut s) = self.current() else { return };
        f(&mut s);
        if self.saved.borrow().as_ref() == Some(&s) {
            if let Some(w) = mark {
                w.remove_css_class("error");
            }
            return;
        }
        match self.core.save_settings(s.clone()) {
            Ok(()) => {
                if let Some(w) = mark {
                    w.remove_css_class("error");
                }
                self.state.replace(Some(s.clone()));
                self.saved.replace(Some(s));
                self.refresh_summaries();
                if let Some(cb) = self.on_saved.borrow().as_ref() {
                    cb();
                }
            }
            Err(e) => {
                if let Some(w) = mark {
                    w.add_css_class("error");
                }
                self.toaster.show(copy::settings_error(&e));
            }
        }
    }

    fn edit(&self, f: impl FnOnce(&mut Settings)) {
        self.edit_marked(None, f);
    }

    fn template(&self, kind: TemplateKind) -> &Template {
        match kind {
            TemplateKind::Folder => &self.folder,
            TemplateKind::Filename => &self.filename,
        }
    }

    fn candidate(&self) -> Option<Settings> {
        let mut s = self.current()?;
        s.path = self.folder.row.text().to_string();
        s.filename = self.filename.row.text().to_string();
        Some(s)
    }

    fn update_previews(&self) {
        let Some(s) = self.candidate() else { return };
        let p = self.core.preview(s);
        for (t, check) in [(&self.folder, &p.folder), (&self.filename, &p.filename)] {
            let bold = |part: &str, on: bool| {
                let e = glib::markup_escape_text(part);
                if on {
                    format!("<b>{e}</b>")
                } else {
                    e.to_string()
                }
            };
            t.line.set_markup(&format!(
                "{}/{}/{}.{}",
                glib::markup_escape_text(copy::preview_root(p.root.as_deref())),
                bold(copy::preview_dir(&p.dir), t.kind == TemplateKind::Folder),
                bold(&p.file, t.kind == TemplateKind::Filename),
                glib::markup_escape_text(&p.ext)
            ));
            t.note
                .set_text(&copy::preview_sample(p.exported_stem.as_deref()));
            let problem = copy::preview_problem(&check.unknown).unwrap_or_default();
            t.problem.set_text(&problem);
            t.problem.set_visible(!problem.is_empty());
            ui::set_class(&t.row, "error", !problem.is_empty());
            for (name, _, button) in &t.fields {
                ui::set_class(button, "accent", check.used.contains(name));
            }
        }
    }

    fn schedule_previews(self: &Rc<Self>) {
        let generation = self.preview_gen.get().wrapping_add(1);
        self.preview_gen.set(generation);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(PREVIEW_DELAY, move || {
            if let Some(panel) = weak.upgrade()
                && panel.preview_gen.get() == generation
            {
                panel.update_previews();
            }
        });
    }

    fn commit_template(&self, kind: TemplateKind) {
        let Some(s) = self.candidate() else { return };
        let text = self.template(kind).row.text().to_string();
        let p = self.core.preview(s);
        let check = match kind {
            TemplateKind::Folder => p.folder,
            TemplateKind::Filename => p.filename,
        };
        if !check.unknown.is_empty() {
            return;
        }
        let row: gtk::Widget = self.template(kind).row.clone().upcast();
        self.edit_marked(Some(&row), |s| match kind {
            TemplateKind::Folder => s.path = text,
            TemplateKind::Filename => s.filename = text,
        });
    }

    fn insert_field(&self, kind: TemplateKind, token: &str) {
        let row = &self.template(kind).row;
        let text = row.text().to_string();
        let (a, b) = row.selection_bounds().unwrap_or_else(|| {
            let p = row.position();
            (p, p)
        });
        let chars: Vec<char> = text.chars().collect();
        let start = (a.min(b).max(0) as usize).min(chars.len());
        let end = (a.max(b).max(0) as usize).min(chars.len());
        let separator = match kind {
            TemplateKind::Folder => '/',
            TemplateKind::Filename => '_',
        };
        let needs_separator = start > 0 && !"/_-{|.".contains(chars[start - 1]);
        let mut out: String = chars[..start].iter().collect();
        if needs_separator {
            out.push(separator);
        }
        out.push_str(token);
        let caret = out.chars().count();
        out.extend(&chars[end..]);
        row.set_text(&out);
        row.set_position(caret as i32);
        self.update_previews();
        self.commit_template(kind);
    }

    fn refresh_summaries(&self) {
        let Some(s) = self.current() else { return };
        self.export_row.set_subtitle(&s.export_dir);
        self.layout.set_description(&copy::layout_summary(&s));
        self.dates
            .set_description(&copy::dates_settings_summary(&s));
        self.filter.set_description(&copy::filter_summary(&s));
        self.files.set_description(&copy::files_summary(&s));
        self.filter_group
            .set_description(Some(copy::filter_hint(&s)));
        self.update_previews();
    }

    fn choose_export_dir(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::new();
        dialog.set_modal(true);
        if let Some(s) = self.current()
            && !s.export_dir.trim().is_empty()
        {
            dialog.set_initial_folder(Some(&gio::File::for_path(s.export_dir.trim())));
        }
        let weak = Rc::downgrade(self);
        dialog.select_folder(
            ui::window_of(&self.root).as_ref(),
            gio::Cancellable::NONE,
            move |res| {
                let (Some(panel), Ok(file)) = (weak.upgrade(), res) else {
                    return;
                };
                if let Some(path) = file.path() {
                    let dir = path.display().to_string();
                    panel.edit(|s| s.export_dir = dir);
                }
            },
        );
    }

    fn load(&self) {
        let s = match self.core.settings() {
            Ok(s) => s,
            Err(e) => {
                self.window_toaster.show(copy::settings_error(&e));
                self.root.set_sensitive(false);
                return;
            }
        };
        self.loading.set(true);
        self.folder.row.set_text(&s.path);
        self.filename.row.set_text(&s.filename);
        if let Some(i) = copy::DATE_SOURCES.iter().position(|d| *d == s.date_source) {
            self.date_source.set_selected(i as u32);
        }
        if let Some(i) = copy::ZONES.iter().position(|z| *z == s.zone) {
            self.zone.set_selected(i as u32);
        }
        self.offset.set_text(&s.offset);
        self.offset.set_visible(s.zone == ZoneMode::Fixed);
        for (v, b) in &self.media {
            b.set_active(*v == s.media);
        }
        self.favorites.set_active(s.favorites);
        self.from.row.set_text(&s.from);
        self.to.row.set_text(&s.to);
        self.albums.set_text(&s.albums);
        self.keywords.set_text(&s.keywords);
        for (v, b) in &self.edits {
            b.set_active(*v == s.edits);
        }
        for (v, b) in &self.metadata {
            b.set_active(*v == s.metadata);
        }
        self.automatic.set_active(s.automatic);
        self.interval.set_value(f64::from(s.check_interval_minutes));
        self.keep_running.set_active(s.keep_running_after_close);
        match crate::startup::enabled() {
            Ok(enabled) => self.open_at_login.set_active(enabled),
            Err(e) => self
                .toaster
                .show(format!("Could not read login settings: {e}")),
        }
        self.state.replace(Some(s.clone()));
        self.saved.replace(Some(s));
        self.loading.set(false);
        self.refresh_summaries();
    }

    fn connect_text(self: &Rc<Self>, row: &adw::EntryRow, f: fn(&mut Settings, String)) {
        let weak = Rc::downgrade(self);
        let r = row.clone();
        on_commit(row, move || {
            let Some(panel) = weak.upgrade() else { return };
            let text = r.text().to_string();
            let mark: gtk::Widget = r.clone().upcast();
            panel.edit_marked(Some(&mark), |s| f(s, text));
        });
    }

    fn connect_date(self: &Rc<Self>, entry: &DateEntry, f: fn(&mut Settings, String)) {
        self.connect_text(&entry.row, f);
        let syncing = Rc::new(Cell::new(false));
        let (row, calendar) = (entry.row.clone(), entry.calendar.clone());
        entry.popover.connect_show({
            let syncing = syncing.clone();
            move |_| {
                if let Some(d) = parse_day(&row.text()) {
                    syncing.set(true);
                    calendar.select_day(&d);
                    syncing.set(false);
                }
            }
        });
        let weak = Rc::downgrade(self);
        let (row, popover) = (entry.row.clone(), entry.popover.clone());
        entry.calendar.connect_day_selected(move |cal| {
            if syncing.get() {
                return;
            }
            let Some(panel) = weak.upgrade() else { return };
            let Ok(text) = cal.date().format("%Y-%m-%d") else {
                return;
            };
            row.set_text(&text);
            popover.popdown();
            let mark: gtk::Widget = row.clone().upcast();
            panel.edit_marked(Some(&mark), |s| f(s, text.to_string()));
        });
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.automatic.connect_active_notify(move |row| {
            if let Some(panel) = weak.upgrade() {
                panel.edit(|s| s.automatic = row.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.interval.connect_value_notify(move |row| {
            if let Some(panel) = weak.upgrade() {
                panel.edit(|s| s.check_interval_minutes = row.value() as u32);
            }
        });
        let weak = Rc::downgrade(self);
        self.keep_running.connect_active_notify(move |row| {
            if let Some(panel) = weak.upgrade() {
                panel.edit(|s| s.keep_running_after_close = row.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.open_at_login.connect_active_notify(move |row| {
            let Some(panel) = weak.upgrade() else { return };
            if panel.loading.get() {
                return;
            }
            if let Err(e) = crate::startup::set_enabled(row.is_active()) {
                panel
                    .toaster
                    .show(format!("Could not change login settings: {e}"));
                panel.loading.set(true);
                row.set_active(!row.is_active());
                panel.loading.set(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.export_button.connect_clicked(move |_| {
            if let Some(panel) = weak.upgrade() {
                panel.choose_export_dir();
            }
        });

        for t in [&self.folder, &self.filename] {
            let kind = t.kind;
            let weak = Rc::downgrade(self);
            t.row.connect_changed(move |_| {
                if let Some(panel) = weak.upgrade() {
                    panel.schedule_previews();
                }
            });
            let weak = Rc::downgrade(self);
            on_commit(&t.row, move || {
                if let Some(panel) = weak.upgrade() {
                    panel.commit_template(kind);
                }
            });
            for (_, token, button) in &t.fields {
                let weak = Rc::downgrade(self);
                let token = token.clone();
                button.connect_clicked(move |_| {
                    if let Some(panel) = weak.upgrade() {
                        panel.insert_field(kind, &token);
                    }
                });
            }
            let weak = Rc::downgrade(self);
            t.popover.connect_show(move |_| {
                if let Some(panel) = weak.upgrade() {
                    panel.update_previews();
                }
            });
        }

        let weak = Rc::downgrade(self);
        self.date_source.connect_selected_notify(move |row| {
            let Some(panel) = weak.upgrade() else { return };
            let Some(v) = copy::DATE_SOURCES.get(row.selected() as usize).copied() else {
                return;
            };
            panel.edit(|s| s.date_source = v);
        });
        let weak = Rc::downgrade(self);
        self.zone.connect_selected_notify(move |row| {
            let Some(panel) = weak.upgrade() else { return };
            let Some(v) = copy::ZONES.get(row.selected() as usize).copied() else {
                return;
            };
            panel.offset.set_visible(v == ZoneMode::Fixed);
            panel.edit(|s| s.zone = v);
        });
        self.connect_text(&self.offset, |s, t| s.offset = t);

        for (v, b) in &self.media {
            let (v, weak) = (*v, Rc::downgrade(self));
            b.connect_toggled(move |b| {
                if let Some(panel) = weak.upgrade()
                    && b.is_active()
                {
                    panel.edit(|s| s.media = v);
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.favorites.connect_active_notify(move |row| {
            if let Some(panel) = weak.upgrade() {
                let on = row.is_active();
                panel.edit(|s| s.favorites = on);
            }
        });
        self.connect_date(&self.from, |s, t| s.from = t);
        self.connect_date(&self.to, |s, t| s.to = t);
        self.connect_text(&self.albums, |s, t| s.albums = t);
        self.connect_text(&self.keywords, |s, t| s.keywords = t);

        for (v, b) in &self.edits {
            let (v, weak) = (*v, Rc::downgrade(self));
            b.connect_toggled(move |b| {
                if let Some(panel) = weak.upgrade()
                    && b.is_active()
                {
                    panel.edit(|s| s.edits = v);
                }
            });
        }
        for (v, b) in &self.metadata {
            let (v, weak) = (*v, Rc::downgrade(self));
            b.connect_toggled(move |b| {
                if let Some(panel) = weak.upgrade()
                    && b.is_active()
                {
                    panel.edit(|s| s.metadata = v);
                }
            });
        }
    }

    pub fn new(core: Arc<Core>, window_toaster: Toaster) -> Rc<SettingsPanel> {
        let root = adw::PreferencesDialog::new();
        root.set_search_enabled(false);
        let toaster = Toaster::Preferences(root.clone());

        let export_button = gtk::Button::from_icon_name("folder-open-symbolic");
        export_button.set_valign(gtk::Align::Center);
        export_button.set_tooltip_text(Some("Choose folder"));
        export_button.add_css_class("flat");
        let export_row = adw::ActionRow::builder()
            .title("Export to")
            .use_markup(false)
            .build();
        export_row.add_css_class("property");
        export_row.add_suffix(&export_button);
        export_row.set_activatable_widget(Some(&export_button));

        let layout = page("Layout", "folder-symbolic");
        let folder = Template::new(
            &core,
            TemplateKind::Folder,
            "Folder",
            copy::FOLDER_PLACEHOLDER,
        );
        let filename = Template::new(
            &core,
            TemplateKind::Filename,
            "Filename",
            copy::FILENAME_PLACEHOLDER,
        );
        layout.add(&group_of(&[export_row.upcast_ref()]));
        layout.add(&group_of(&[
            folder.row.upcast_ref(),
            folder.preview_row().upcast_ref(),
        ]));
        layout.add(&group_of(&[
            filename.row.upcast_ref(),
            filename.preview_row().upcast_ref(),
        ]));

        let dates = page("Dates", "x-office-calendar-symbolic");
        let date_source = combo(
            "Taken date",
            copy::DATE_SOURCE_HINT,
            &copy::DATE_SOURCES.map(copy::date_source),
        );
        let zone = combo(
            "Folder clock",
            copy::ZONE_HINT,
            &copy::ZONES.map(copy::zone),
        );
        let offset = adw::EntryRow::builder()
            .title(copy::zone(ZoneMode::Fixed))
            .use_markup(false)
            .show_apply_button(true)
            .build();
        ui::row_placeholder(&offset, copy::OFFSET_PLACEHOLDER);
        dates.add(&group_of(&[
            date_source.upcast_ref(),
            zone.upcast_ref(),
            offset.upcast_ref(),
        ]));

        let filter = page("Filter", "edit-find-symbolic");
        let (media_box, media) = segmented(&copy::MEDIA.map(|m| (m, copy::media(m))));
        let media_row = adw::ActionRow::builder().title("Media").build();
        media_row.add_suffix(&media_box);
        let favorites = adw::SwitchRow::builder()
            .title(copy::FAVORITES)
            .use_markup(false)
            .build();
        let from = DateEntry::new("From");
        let to = DateEntry::new("To");
        let albums = list_row("Albums", copy::LIST_PLACEHOLDER);
        let keywords = list_row("Keywords", copy::LIST_PLACEHOLDER);
        let filter_group = group_of(&[
            media_row.upcast_ref(),
            favorites.upcast_ref(),
            from.row.upcast_ref(),
            to.row.upcast_ref(),
            albums.upcast_ref(),
            keywords.upcast_ref(),
        ]);
        filter.add(&filter_group);

        let files = page("Files", "document-save-symbolic");
        let (edits_box, edits) = segmented(&[(false, copy::EDITS_OFF), (true, copy::EDITS_ON)]);
        let edits_row = adw::ActionRow::builder()
            .title("Export")
            .subtitle(copy::EDITS_HINT)
            .use_markup(false)
            .build();
        edits_row.add_suffix(&edits_box);
        let (metadata_box, metadata) = segmented(&copy::METADATA.map(|m| (m, copy::metadata(m))));
        let metadata_row = adw::ActionRow::builder()
            .title("Metadata")
            .subtitle(copy::METADATA_HINT)
            .use_markup(false)
            .build();
        metadata_row.add_suffix(&metadata_box);
        files.add(&group_of(&[
            edits_row.upcast_ref(),
            metadata_row.upcast_ref(),
        ]));

        let automation = page("Automation", "alarm-symbolic");
        let automatic = adw::SwitchRow::builder()
            .title("Enable automatic exports")
            .subtitle("Select Export automatically on each source to include it. Uses your saved export settings; dry run does not apply.")
            .build();
        let interval = adw::SpinRow::builder()
            .title("Check every (minutes)")
            .adjustment(&gtk::Adjustment::new(15.0, 1.0, 1440.0, 1.0, 15.0, 0.0))
            .numeric(true)
            .build();
        let keep_running = adw::SwitchRow::builder()
            .title("Keep running after close")
            .subtitle("Reopen Photosink to see progress. Choose Quit to stop it.")
            .build();
        let open_at_login = adw::SwitchRow::builder().title("Open at login").build();
        automation.add(&group_of(&[
            automatic.upcast_ref(),
            interval.upcast_ref(),
            keep_running.upcast_ref(),
            open_at_login.upcast_ref(),
        ]));

        for p in [&layout, &dates, &filter, &files, &automation] {
            root.add(p);
        }

        let panel = Rc::new(SettingsPanel {
            core,
            toaster,
            window_toaster,
            root,
            state: RefCell::new(None),
            saved: RefCell::new(None),
            loading: Cell::new(false),
            preview_gen: Cell::new(0),
            on_saved: RefCell::new(None),
            export_row,
            export_button,
            layout,
            dates,
            filter,
            files,
            folder,
            filename,
            date_source,
            zone,
            offset,
            media,
            favorites,
            from,
            to,
            albums,
            keywords,
            filter_group,
            edits,
            metadata,
            automatic,
            interval,
            keep_running,
            open_at_login,
        });
        panel.load();
        panel.connect();
        panel
    }

    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        self.loading.set(true);
        if let Ok(enabled) = crate::startup::enabled() {
            self.open_at_login.set_active(enabled);
        }
        self.loading.set(false);
        self.root.present(Some(parent));
    }

    pub fn keep_running_after_close(&self) -> bool {
        self.saved
            .borrow()
            .as_ref()
            .is_some_and(|s| s.keep_running_after_close)
    }

    /// Called after every successful save.
    pub fn set_on_saved(&self, f: impl Fn() + 'static) {
        self.on_saved.replace(Some(Box::new(f)));
    }
}
