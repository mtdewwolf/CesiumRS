//! CesiumJS `KmlTour`: playlist runner for KML `gx:Tour` entries.
use super::{ChangeEvent, KmlTourCamera, KmlTourCameraOptions};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

pub type KmlTourEntryRef = Rc<dyn KmlTourPlaylistEntry>;

/// One playlist entry played by [`KmlTour`].
pub trait KmlTourPlaylistEntry {
    fn as_any(&self) -> &dyn Any;
    fn blocking(&self) -> bool {
        true
    }
    fn play(
        &self,
        done: Rc<dyn Fn(bool)>,
        camera: Option<Rc<dyn KmlTourCamera>>,
        camera_options: Option<&KmlTourCameraOptions>,
    );
    fn stop(&self);
}

pub struct KmlTour {
    pub id: String,
    pub name: String,
    playlist_index: Cell<usize>,
    playlist: RefCell<Vec<KmlTourEntryRef>>,
    active_entries: RefCell<Vec<KmlTourEntryRef>>,
    tour_start: ChangeEvent<()>,
    tour_end: ChangeEvent<bool>,
    entry_start: ChangeEvent<KmlTourEntryRef>,
    entry_end: ChangeEvent<(KmlTourEntryRef, Option<bool>)>,
    weak: Weak<KmlTour>,
}

impl KmlTour {
    pub fn new(name: impl Into<String>, id: impl Into<String>) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            id: id.into(),
            name: name.into(),
            playlist_index: Cell::new(0),
            playlist: RefCell::new(Vec::new()),
            active_entries: RefCell::new(Vec::new()),
            tour_start: ChangeEvent::new(),
            tour_end: ChangeEvent::new(),
            entry_start: ChangeEvent::new(),
            entry_end: ChangeEvent::new(),
            weak: weak.clone(),
        })
    }

    pub fn playlist_index(&self) -> usize {
        self.playlist_index.get()
    }

    pub fn playlist(&self) -> Vec<KmlTourEntryRef> {
        self.playlist.borrow().clone()
    }

    pub fn active_entries(&self) -> Vec<KmlTourEntryRef> {
        self.active_entries.borrow().clone()
    }

    pub fn tour_start(&self) -> &ChangeEvent<()> {
        &self.tour_start
    }

    pub fn tour_end(&self) -> &ChangeEvent<bool> {
        &self.tour_end
    }

    pub fn entry_start(&self) -> &ChangeEvent<KmlTourEntryRef> {
        &self.entry_start
    }

    pub fn entry_end(&self) -> &ChangeEvent<(KmlTourEntryRef, Option<bool>)> {
        &self.entry_end
    }

    pub fn add_playlist_entry(&self, entry: KmlTourEntryRef) {
        self.playlist.borrow_mut().push(entry);
    }

    pub fn play(&self, camera: Option<Rc<dyn KmlTourCamera>>, camera_options: Option<KmlTourCameraOptions>) {
        self.tour_start.raise(());
        let weak = self.weak.clone();
        let all_done: Rc<dyn Fn(bool)> = Rc::new(move |terminated| {
            let Some(tour) = weak.upgrade() else {
                return;
            };
            tour.playlist_index.set(0);
            if !terminated {
                tour.cancel_all_entries();
            }
            tour.tour_end.raise(terminated);
        });
        self.play_entry(camera, camera_options, all_done);
    }

    pub fn stop(&self) {
        self.cancel_all_entries();
    }

    fn cancel_all_entries(&self) {
        loop {
            let entry = self.active_entries.borrow_mut().pop();
            let Some(entry) = entry else {
                break;
            };
            entry.stop();
        }
    }

    fn play_entry(
        &self,
        camera: Option<Rc<dyn KmlTourCamera>>,
        camera_options: Option<KmlTourCameraOptions>,
        all_done: Rc<dyn Fn(bool)>,
    ) {
        let entry = self.playlist.borrow().get(self.playlist_index.get()).cloned();
        let Some(entry) = entry else {
            all_done(false);
            return;
        };

        self.active_entries.borrow_mut().push(entry.clone());
        self.entry_start.raise(entry.clone());

        let weak = self.weak.clone();
        let cam_next = camera.clone();
        let opts_next = camera_options.clone();
        let done_next = all_done.clone();
        let play_next: Rc<dyn Fn(bool)> = Rc::new(move |terminated| {
            let Some(tour) = weak.upgrade() else {
                return;
            };
            tour.play_next(cam_next.clone(), opts_next.clone(), done_next.clone(), terminated);
        });

        if entry.blocking() {
            entry.play(play_next, camera, camera_options.as_ref());
        } else {
            let weak_nonblocking = self.weak.clone();
            let entry_nonblocking = entry.clone();
            let nb_done: Rc<dyn Fn(bool)> = Rc::new(move |_| {
                let Some(tour) = weak_nonblocking.upgrade() else {
                    return;
                };
                tour.entry_end.raise((entry_nonblocking.clone(), None));
                let mut active = tour.active_entries.borrow_mut();
                if let Some(idx) = active.iter().position(|e| Rc::ptr_eq(e, &entry_nonblocking)) {
                    active.remove(idx);
                }
            });
            entry.play(nb_done, None, None);
            play_next(false);
        }
    }

    fn play_next(
        &self,
        camera: Option<Rc<dyn KmlTourCamera>>,
        camera_options: Option<KmlTourCameraOptions>,
        all_done: Rc<dyn Fn(bool)>,
        terminated: bool,
    ) {
        let entry = self.playlist.borrow().get(self.playlist_index.get()).cloned();
        if let Some(entry) = entry {
            self.entry_end.raise((entry.clone(), Some(terminated)));
            if !terminated {
                let mut active = self.active_entries.borrow_mut();
                if let Some(idx) = active.iter().position(|e| Rc::ptr_eq(e, &entry)) {
                    active.remove(idx);
                }
            }
        }
        if terminated {
            all_done(terminated);
        } else {
            self.playlist_index.set(self.playlist_index.get() + 1);
            self.play_entry(camera, camera_options, all_done);
        }
    }
}
