//! A single-threaded, no-heap static cell -- the same pattern
//! `ui-service-nxu`'s own `storage.rs` uses for its big pixel/text-scratch
//! buffers. `Navigator`'s directory listing is the same shape of problem: a
//! real directory can hold enough entries that storing them as a stack
//! local (inside `VoyagerApp`, itself a stack local in the runtime's own
//! call frame) would eat a meaningful fraction of a 16 KiB kernel stack.
//! Living in `apps/voyager` instead of being reused from `ui-service-nxu`
//! because this crate has no dependency on that runtime-only one (see this
//! crate's own top-level doc comment).

use core::cell::UnsafeCell;

pub(crate) struct StaticCell<T>(UnsafeCell<T>);

unsafe impl<T> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub(crate) unsafe fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}
