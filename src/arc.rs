use {
    crate::{Erased, Thinnable},
    alloc::{sync::Arc, vec::Vec},
    core::{cmp, marker::PhantomData, mem::ManuallyDrop, ops::Deref, ptr},
};

pub struct ThinArc<T: ?Sized + Thinnable> {
    raw: ptr::NonNull<Erased>,
    marker: PhantomData<Arc<T>>,
}

unsafe impl<T: ?Sized + Thinnable> Send for ThinArc<T> where T: Send {}
unsafe impl<T: ?Sized + Thinnable> Sync for ThinArc<T> where T: Send {}

thin_holder!(for ThinArc<T> as Arc<T> with make_fat_mut);
std_traits!(for ThinArc<T> as T where T: ?Sized + Thinnable);

impl<T: ?Sized + Thinnable> ThinArc<T> {
    pub fn new(slice: Vec<T::SliceItem>, head: impl FnOnce(&[T::SliceItem]) -> T::Head) -> Self {
        // FUTURE(https://internals.rust-lang.org/t/stabilizing-a-rc-layout/11265):
        //     When/if `Arc`'s heap repr is stable, allocate directly rather than `Box` first.
        let boxed: thin::Box<T> = thin::Box::new(slice, head);
        let arc: Arc<T> = thin::Box::into_fat(boxed).into();
        arc.into()
    }
}
