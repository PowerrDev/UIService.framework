use core::cell::UnsafeCell;

use ui_text::TextScratch;

/// Single-threaded early-boot storage used before the userspace UI server
/// exists. This is intentionally private to the NXU adapter.
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

pub(crate) static TEXT_SCRATCH: StaticCell<TextScratch> = StaticCell::new(TextScratch::new());
