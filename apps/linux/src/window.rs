use crate::settings_panel::SettingsPanel;
use crate::tab_page::TabPage;
use crate::ui::{Toaster, hbox, label};
use adw::prelude::*;
use gtk::gio;
use photosink_bridge::catalog::Tab;
use photosink_bridge::{Change, Core, Status};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// The main window: a sidebar of tabs beside the selected tab's page.
pub struct Window {
    core: Arc<Core>,
    toaster: Toaster,
    pub window: adw::ApplicationWindow,
    settings: Rc<SettingsPanel>,
    split: adw::NavigationSplitView,
    sidebar: gtk::ListBox,
    rows: Vec<(Tab, gtk::ListBoxRow)>,
    pages: HashMap<Tab, Rc<TabPage>>,
    content_page: adw::NavigationPage,
    content: gtk::Stack,
    header: gtk::Stack,
    title: adw::WindowTitle,
    current: Cell<Option<Tab>>,
    last_status: RefCell<Option<Status>>,
}

fn icon(tab: Tab) -> &'static str {
    match tab {
        Tab::ApplePhotos => "image-x-generic-symbolic",
        Tab::OneDrive => "folder-remote-symbolic",
        Tab::Dropbox => "folder-download-symbolic",
        Tab::GoogleTakeout => "package-x-generic-symbolic",
        Tab::Consolidate => "view-sort-ascending-symbolic",
        Tab::Clean => "user-trash-symbolic",
        Tab::Dates => "x-office-calendar-symbolic",
    }
}

fn page_name(tab: Tab) -> String {
    format!("{tab:?}")
}

impl Window {
    fn refresh_title(&self) {
        let Some(tab) = self.current.get() else {
            return;
        };
        let st = self.core.status();
        let subtitle = match self.core.blocker(tab) {
            Some(b) if st.running.is_none() => b,
            _ => std::iter::once(st.text.clone())
                .chain(
                    self.core
                        .stats(tab)
                        .into_iter()
                        .map(|s| format!("{} {}", s.label, s.value)),
                )
                .collect::<Vec<_>>()
                .join(" · "),
        };
        self.title.set_subtitle(&subtitle);
    }

    fn select(&self, tab: Tab) {
        let Some(page) = self.pages.get(&tab) else {
            return;
        };
        self.current.set(Some(tab));
        self.content.set_visible_child_name(&page_name(tab));
        self.header.set_visible_child_name(&page_name(tab));
        self.title.set_title(&page.info.name);
        self.content_page.set_title(&page.info.name);
        if let Some((_, row)) = self.rows.iter().find(|(t, _)| *t == tab)
            && !row.is_selected()
        {
            self.sidebar.select_row(Some(row));
        }
        self.refresh_title();
    }

    fn status(&self) {
        let st = self.core.status();
        for (tab, row) in &self.rows {
            row.set_sensitive(st.running.is_none_or(|t| t == *tab));
        }
        for page in self.pages.values() {
            page.refresh_run();
            page.view.refresh_clear();
            if let Some(d) = &page.dates {
                d.refresh();
            }
        }
        let fresh_error = st.error && self.last_status.borrow().as_ref() != Some(&st);
        if fresh_error {
            self.toaster.show(&st.text);
        }
        self.last_status.replace(Some(st));
        self.refresh_title();
    }

    pub fn apply(&self, change: Change) {
        match change {
            Change::Status => self.status(),
            Change::Grid { tab } => {
                if let Some(page) = self.pages.get(&tab) {
                    page.view.refresh();
                }
                if self.current.get() == Some(tab) {
                    self.refresh_title();
                }
            }
            Change::Dates => {
                for page in self.pages.values() {
                    if let Some(d) = &page.dates {
                        d.refresh();
                    }
                }
            }
        }
    }

    pub fn show_preferences(&self) {
        self.settings.present(&self.window);
    }

    pub fn new(app: &adw::Application, core: Arc<Core>) -> Rc<Window> {
        let overlay = adw::ToastOverlay::new();
        let toaster = Toaster::Window(overlay.clone());
        let settings = SettingsPanel::new(core.clone(), toaster.clone());

        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("navigation-sidebar");
        let content = gtk::Stack::new();
        content.set_transition_type(gtk::StackTransitionType::Crossfade);
        let header = gtk::Stack::new();
        header.set_hhomogeneous(false);
        let mut rows = Vec::new();
        let mut pages = HashMap::new();
        let mut first_in_group: HashMap<i32, String> = HashMap::new();
        for group in core.groups() {
            for (i, info) in group.tabs.iter().enumerate() {
                let row_box = hbox(12);
                row_box.append(&gtk::Image::from_icon_name(icon(info.tab)));
                row_box.append(&label(&info.name, &[]));
                let row = gtk::ListBoxRow::builder().child(&row_box).build();
                sidebar.append(&row);
                if i == 0 {
                    first_in_group.insert(row.index(), group.name.clone());
                }
                rows.push((info.tab, row));
                let page = TabPage::new(core.clone(), info.clone(), toaster.clone());
                content.add_named(&page.content, Some(&page_name(info.tab)));
                header.add_named(&page.header, Some(&page_name(info.tab)));
                pages.insert(info.tab, page);
            }
        }
        sidebar.set_header_func(move |row, _| {
            let heading = first_in_group.get(&row.index()).map(|name| {
                let l = label(name, &["heading", "dim-label"]);
                l.set_margin_start(12);
                l.set_margin_top(if row.index() == 0 { 6 } else { 18 });
                l.set_margin_bottom(6);
                l
            });
            row.set_header(heading.as_ref());
        });

        let menu = gio::Menu::new();
        menu.append(Some("_Preferences"), Some("app.preferences"));
        menu.append(Some("_About photosink"), Some("app.about"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .tooltip_text("Main Menu")
            .build();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.pack_end(&menu_button);
        let sidebar_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&sidebar)
            .build();
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.set_content(Some(&sidebar_scroller));
        let sidebar_page = adw::NavigationPage::builder()
            .title("photosink")
            .child(&sidebar_view)
            .build();

        let title = adw::WindowTitle::new("", "");
        let content_header = adw::HeaderBar::new();
        content_header.set_title_widget(Some(&title));
        content_header.pack_end(&header);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&content_header);
        content_view.set_content(Some(&content));
        let content_page = adw::NavigationPage::builder()
            .title("photosink")
            .child(&content_view)
            .build();

        let split = adw::NavigationSplitView::new();
        split.set_min_sidebar_width(220.0);
        split.set_sidebar(Some(&sidebar_page));
        split.set_content(Some(&content_page));
        overlay.set_child(Some(&split));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("photosink")
            .default_width(1100)
            .default_height(760)
            .width_request(480)
            .height_request(400)
            .content(&overlay)
            .build();
        if let Ok(condition) = adw::BreakpointCondition::parse("max-width: 720sp") {
            let bp = adw::Breakpoint::new(condition);
            bp.add_setter(&split, "collapsed", Some(&true.to_value()));
            for page in pages.values() {
                bp.add_setter(&page.view.follow, "visible", Some(&false.to_value()));
                bp.add_setter(&page.view.clear, "visible", Some(&false.to_value()));
            }
            window.add_breakpoint(bp);
        }

        let win = Rc::new(Window {
            core,
            toaster,
            window,
            settings,
            split,
            sidebar,
            rows,
            pages,
            content_page,
            content,
            header,
            title,
            current: Cell::new(None),
            last_status: RefCell::new(None),
        });

        let weak = Rc::downgrade(&win);
        win.sidebar.connect_row_selected(move |_, row| {
            let (Some(win), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            let Some(tab) = win.rows.iter().find(|(_, r)| r == row).map(|(t, _)| *t) else {
                return;
            };
            if win.current.get() != Some(tab) {
                win.select(tab);
            }
            win.split.set_show_content(true);
        });
        let weak = Rc::downgrade(&win);
        win.settings.set_on_saved(move || {
            if let Some(win) = weak.upgrade() {
                for page in win.pages.values() {
                    page.refresh_run();
                }
                win.refresh_title();
            }
        });

        if let Some((tab, _)) = win.rows.first() {
            win.select(*tab);
        }
        win.status();
        win
    }
}
