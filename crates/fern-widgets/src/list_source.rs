//! Type-erased data source for list-backed widgets.
//!
//! Wraps both `ListModel<T>` and any `ListDataSource<Item = T>` behind a
//! uniform set of `Rc<dyn Fn(..)>` closures so consumers like `ListView` and
//! `ComboBox` don't have to carry a generic source parameter or duplicate the
//! wrapping code.

use std::rc::Rc;

use fern_core::ObserverHandle;
use fern_core::widget::Widget;
use fern_data::{DataChange, ListDataSource, ListModel};

pub(crate) struct ListSource<T: 'static> {
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    pub(crate) with_item_fn:
        Rc<dyn Fn(usize, &dyn Fn(&T) -> Box<dyn Widget>) -> Option<Box<dyn Widget>>>,
    pub(crate) observe_fn: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
    /// Only populated when backed by `ListModel` — external sources can't
    /// reorder in place.
    pub(crate) move_item_fn: Option<Rc<dyn Fn(usize, usize)>>,
}

impl<T: 'static> ListSource<T> {
    pub(crate) fn from_model(model: ListModel<T>) -> Self {
        let m1 = model.clone();
        let m2 = model.clone();
        let m3 = model.clone();
        let m4 = model.clone();
        Self {
            len_fn: Rc::new(move || m1.len()),
            with_item_fn: Rc::new(move |index, f| m2.with_item(index, |item| f(item))),
            observe_fn: Rc::new(move |f| m3.observe_changes(move |c| f(c))),
            move_item_fn: Some(Rc::new(move |from, to| m4.move_item(from, to))),
        }
    }

    pub(crate) fn from_data_source<S: ListDataSource<Item = T>>(source: S) -> Self {
        let s = Rc::new(source);
        let s1 = s.clone();
        let s2 = s.clone();
        let s3 = s.clone();
        Self {
            len_fn: Rc::new(move || s1.len()),
            with_item_fn: Rc::new(move |index, f| s2.with_item(index, |item| f(item))),
            observe_fn: Rc::new(move |f| s3.observe_changes(move |c| f(c))),
            move_item_fn: None,
        }
    }

    /// Build from a `len / item-at / observe` closure triple where the
    /// item getter returns an owned clone instead of borrowing. The
    /// resulting `with_item_fn` clones the item out, then hands a
    /// reference to the delegate. Used by `ComboBox`'s `ItemSource`,
    /// which fronts both `ListModel<T>` and `ListDataSource<T>` behind
    /// cloning accessors and doesn't carry a `&T` lifetime.
    pub(crate) fn from_cloning_accessors(
        len_fn: Rc<dyn Fn() -> usize>,
        item_at: Rc<dyn Fn(usize) -> Option<T>>,
        observe_fn: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
    ) -> Self
    where
        T: Clone,
    {
        Self {
            len_fn,
            with_item_fn: Rc::new(move |index, f| item_at(index).as_ref().map(|item| f(item))),
            observe_fn,
            move_item_fn: None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        (self.len_fn)()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
