use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const PAGE: u32 = 200;
const PAGES: usize = 8;

type Fetch = Box<dyn Fn(u32, u32) -> Vec<glib::BoxedAnyObject>>;
type Same = Box<dyn Fn(&glib::BoxedAnyObject, &glib::BoxedAnyObject) -> bool>;
type Len = Box<dyn Fn() -> u32>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LazyList {
        pub(super) len: Cell<u32>,
        pub(super) pages: RefCell<VecDeque<(u32, Vec<glib::BoxedAnyObject>)>>,
        pub(super) fetch: RefCell<Option<Fetch>>,
        pub(super) same: RefCell<Option<Same>>,
        pub(super) count: RefCell<Option<Len>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LazyList {
        const NAME: &'static str = "PhotosinkLazyList";
        type Type = super::LazyList;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for LazyList {}

    impl ListModelImpl for LazyList {
        fn item_type(&self) -> glib::Type {
            glib::BoxedAnyObject::static_type()
        }

        fn n_items(&self) -> u32 {
            self.len.get()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            if position >= self.len.get() {
                return None;
            }
            Some(self.obj().page_item(position).upcast())
        }
    }
}

glib::wrapper! {
    /// A list model over rows owned by `Core`, fetched a page at a time.
    /// Items are `BoxedAnyObject`s holding `Option<T>`; `None` stands in for
    /// a row that vanished before the next refresh.
    pub struct LazyList(ObjectSubclass<imp::LazyList>) @implements gio::ListModel;
}

impl LazyList {
    fn fetch(&self, page: u32) -> Vec<glib::BoxedAnyObject> {
        let fetch = self.imp().fetch.borrow();
        fetch
            .as_ref()
            .map(|f| f(page * PAGE, PAGE))
            .unwrap_or_default()
    }

    fn page_item(&self, position: u32) -> glib::BoxedAnyObject {
        let page = position / PAGE;
        let offset = (position % PAGE) as usize;
        let mut pages = self.imp().pages.borrow_mut();
        let objs = match pages.iter().position(|(p, _)| *p == page) {
            Some(i) => {
                let entry = pages.remove(i).unwrap_or_default();
                pages.push_back(entry);
                &pages.back().unwrap_or_else(|| unreachable!()).1
            }
            None => {
                drop(pages);
                let fresh = self.fetch(page);
                pages = self.imp().pages.borrow_mut();
                pages.push_back((page, fresh));
                while pages.len() > PAGES {
                    pages.pop_front();
                }
                &pages.back().unwrap_or_else(|| unreachable!()).1
            }
        };
        objs.get(offset).cloned().unwrap_or_else(|| self.blank())
    }

    fn blank(&self) -> glib::BoxedAnyObject {
        glib::BoxedAnyObject::new(None::<()>)
    }

    pub fn new<T: PartialEq + 'static>(
        len: impl Fn() -> u32 + 'static,
        fetch: impl Fn(u32, u32) -> Vec<T> + 'static,
    ) -> LazyList {
        let list: LazyList = glib::Object::new();
        let imp = list.imp();
        imp.count.replace(Some(Box::new(len)));
        imp.fetch.replace(Some(Box::new(move |start, count| {
            fetch(start, count)
                .into_iter()
                .map(|row| glib::BoxedAnyObject::new(Some(row)))
                .collect()
        })));
        imp.same.replace(Some(Box::new(|a, b| {
            match (a.try_borrow::<Option<T>>(), b.try_borrow::<Option<T>>()) {
                (Ok(a), Ok(b)) => *a == *b,
                _ => false,
            }
        })));
        list
    }

    /// Re-reads the length and every cached page, telling views about rows
    /// that changed or were added. Returns whether rows were added.
    pub fn refresh(&self) -> bool {
        let imp = self.imp();
        let old = imp.len.get();
        let new = imp.count.borrow().as_ref().map_or(0, |f| f());
        if new < old {
            imp.pages.borrow_mut().clear();
            imp.len.set(new);
            self.items_changed(0, old, new);
            return false;
        }
        let cached: Vec<u32> = imp.pages.borrow().iter().map(|(p, _)| *p).collect();
        let mut changed: Vec<u32> = Vec::new();
        for page in cached {
            let fresh = self.fetch(page);
            let mut pages = imp.pages.borrow_mut();
            let Some(slot) = pages.iter_mut().find(|(p, _)| *p == page) else {
                continue;
            };
            let same = imp.same.borrow();
            for (i, before) in slot.1.iter().enumerate() {
                let pos = page * PAGE + i as u32;
                if pos >= old {
                    break;
                }
                let kept = fresh
                    .get(i)
                    .is_some_and(|after| same.as_ref().is_some_and(|f| f(before, after)));
                if !kept {
                    changed.push(pos);
                }
            }
            slot.1 = fresh;
        }
        changed.sort_unstable();
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for pos in changed {
            match runs.last_mut() {
                Some((start, n)) if *start + *n == pos => *n += 1,
                _ => runs.push((pos, 1)),
            }
        }
        for (start, n) in runs {
            self.items_changed(start, n, n);
        }
        if new > old {
            imp.len.set(new);
            self.items_changed(old, 0, new - old);
            return true;
        }
        false
    }
}
