use {
    crate::{Erased, Thinnable},
    alloc::{rc::Rc, vec::Vec},
    core::{cmp, marker::PhantomData, mem::ManuallyDrop, ops::Deref, ptr},
};

pub struct ThinRc<T: ?Sized + Thinnable> {
    raw: ptr::NonNull<Erased>,
    marker: PhantomData<Rc<T>>,
}

// impl<T: ?Sized + Thinnable> !Send for ThinRc<T> {}
// impl<T: ?Sized + Thinnable> !Sync for ThinRc<T> {}

thin_holder!(for ThinRc<T> as Rc<T> with make_fat_mut);
std_traits!(for ThinRc<T> as T where T: ?Sized + Thinnable);

impl<T: ?Sized + Thinnable> ThinRc<T> {
    pub fn new(slice: Vec<T::SliceItem>, head: impl FnOnce(&[T::SliceItem]) -> T::Head) -> Self {
        // FUTURE(https://internals.rust-lang.org/t/stabilizing-a-rc-layout/11265):
        //     When/if `Rc`'s heap repr is stable, allocate directly rather than `Box` first.
        let boxed: thin::Box<T> = thin::Box::new(slice, head);
        let rc: Rc<T> = thin::Box::into_fat(boxed).into();
        rc.into()
    }
}
