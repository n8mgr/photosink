use crate::ui::{self, Toaster, hbox, label, vbox};
use adw::prelude::*;
use gtk::{gio, glib};
use photosink_bridge::Core;
use photosink_bridge::settings::{Media, Metadata, Options, Settings, TemplateKind, ZoneMode};
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
    options: Options,
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
        for g in core.fields() {
            let group = vbox(4);
            group.append(&label(&g.name, &["heading"]));
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(false)
                .row_spacing(4)
                .column_spacing(4)
                .max_children_per_line(6)
                .build();
            for f in g.fields {
                let b = gtk::Button::with_label(&f.name);
                b.set_tooltip_text(Some(&f.help));
                b.add_css_class("monospace");
                flow.insert(&b, -1);
                fields.push((f.name, f.token, b));
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

fn segmented<T: Copy>(items: &[(T, String)]) -> (gtk::Box, Vec<(T, gtk::ToggleButton)>) {
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

fn combo(title: &str, subtitle: &str, labels: &[String]) -> adw::ComboRow {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .model(&gtk::StringList::new(&labels))
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
                self.toaster.show(e.to_string());
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
        for t in [&self.folder, &self.filename] {
            let p = self.core.preview(s.clone(), t.kind);
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
                glib::markup_escape_text(&p.root),
                bold(&p.dir, t.kind == TemplateKind::Folder),
                bold(&p.file, t.kind == TemplateKind::Filename),
                glib::markup_escape_text(&p.ext)
            ));
            t.note.set_text(&p.sample_note);
            let problem = p.problem.unwrap_or_default();
            t.problem.set_text(&problem);
            t.problem.set_visible(!problem.is_empty());
            ui::set_class(&t.row, "error", !problem.is_empty());
            for (name, _, button) in &t.fields {
                ui::set_class(button, "accent", p.used.contains(name));
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
        if self.core.preview(s, kind).problem.is_some() {
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
        let (start, end) = row.selection_bounds().unwrap_or_else(|| {
            let p = row.position();
            (p, p)
        });
        let ins = self.core.insert_field(
            text.clone(),
            ui::utf16_offset(&text, start),
            ui::utf16_offset(&text, end),
            token.to_owned(),
            kind,
        );
        row.set_text(&ins.text);
        row.set_position(ui::char_offset(&ins.text, ins.caret));
        self.update_previews();
        self.commit_template(kind);
    }

    fn refresh_summaries(&self) {
        let Some(s) = self.current() else { return };
        self.export_row.set_subtitle(&s.export_dir);
        let m = self.core.summaries(s);
        self.layout.set_description(&m.layout);
        self.dates.set_description(&m.dates);
        self.filter.set_description(&m.filter);
        self.files.set_description(&m.files);
        self.filter_group.set_description(Some(&m.filter_hint));
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
                self.window_toaster.show(e.to_string());
                self.root.set_sensitive(false);
                return;
            }
        };
        self.loading.set(true);
        self.folder.row.set_text(&s.path);
        self.filename.row.set_text(&s.filename);
        let o = &self.options;
        if let Some(i) = o.date_sources.iter().position(|d| d.value == s.date_source) {
            self.date_source.set_selected(i as u32);
        }
        if let Some(i) = o.zones.iter().position(|z| z.value == s.zone) {
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
            let Some(v) = panel
                .options
                .date_sources
                .get(row.selected() as usize)
                .map(|d| d.value)
            else {
                return;
            };
            panel.edit(|s| s.date_source = v);
        });
        let weak = Rc::downgrade(self);
        self.zone.connect_selected_notify(move |row| {
            let Some(panel) = weak.upgrade() else { return };
            let Some(v) = panel
                .options
                .zones
                .get(row.selected() as usize)
                .map(|z| z.value)
            else {
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
        let o = core.options();
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
        let folder = Template::new(&core, TemplateKind::Folder, "Folder", &o.folder_placeholder);
        let filename = Template::new(
            &core,
            TemplateKind::Filename,
            "Filename",
            &o.filename_placeholder,
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
            &o.date_source_hint,
            &o.date_sources
                .iter()
                .map(|d| d.label.clone())
                .collect::<Vec<_>>(),
        );
        let zone = combo(
            "Folder clock",
            &o.zone_hint,
            &o.zones.iter().map(|z| z.label.clone()).collect::<Vec<_>>(),
        );
        let fixed = o
            .zones
            .iter()
            .find(|z| z.value == ZoneMode::Fixed)
            .map(|z| z.label.clone())
            .unwrap_or_default();
        let offset = adw::EntryRow::builder()
            .title(fixed)
            .use_markup(false)
            .show_apply_button(true)
            .build();
        ui::row_placeholder(&offset, &o.offset_placeholder);
        dates.add(&group_of(&[
            date_source.upcast_ref(),
            zone.upcast_ref(),
            offset.upcast_ref(),
        ]));

        let filter = page("Filter", "edit-find-symbolic");
        let (media_box, media) = segmented(
            &o.media
                .iter()
                .map(|m| (m.value, m.label.clone()))
                .collect::<Vec<_>>(),
        );
        let media_row = adw::ActionRow::builder().title("Media").build();
        media_row.add_suffix(&media_box);
        let favorites = adw::SwitchRow::builder()
            .title(&o.favorites_label)
            .use_markup(false)
            .build();
        let from = DateEntry::new("From");
        let to = DateEntry::new("To");
        let albums = list_row("Albums", &o.list_placeholder);
        let keywords = list_row("Keywords", &o.list_placeholder);
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
        let (edits_box, edits) =
            segmented(&[(false, o.edits_off.clone()), (true, o.edits_on.clone())]);
        let edits_row = adw::ActionRow::builder()
            .title("Export")
            .subtitle(&o.edits_hint)
            .use_markup(false)
            .build();
        edits_row.add_suffix(&edits_box);
        let (metadata_box, metadata) = segmented(
            &o.metadata
                .iter()
                .map(|m| (m.value, m.label.clone()))
                .collect::<Vec<_>>(),
        );
        let metadata_row = adw::ActionRow::builder()
            .title("Metadata")
            .subtitle(&o.metadata_hint)
            .use_markup(false)
            .build();
        metadata_row.add_suffix(&metadata_box);
        files.add(&group_of(&[
            edits_row.upcast_ref(),
            metadata_row.upcast_ref(),
        ]));

        for p in [&layout, &dates, &filter, &files] {
            root.add(p);
        }

        let panel = Rc::new(SettingsPanel {
            core,
            options: o,
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
        });
        panel.load();
        panel.connect();
        panel
    }

    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        self.root.present(Some(parent));
    }

    /// Called after every successful save.
    pub fn set_on_saved(&self, f: impl Fn() + 'static) {
        self.on_saved.replace(Some(Box::new(f)));
    }
}
