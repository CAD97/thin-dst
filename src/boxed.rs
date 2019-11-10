use core::alloc::Layout;
use {
    crate::{polyfill::*, Erased, Thinnable},
    alloc::{
        alloc::{alloc, handle_alloc_error},
        boxed::Box,
        vec::Vec,
    },
    core::{
        cmp,
        marker::PhantomData,
        mem::{self, ManuallyDrop},
        ops::{Deref, DerefMut},
        ptr,
    },
};

pub struct ThinBox<T: ?Sized + Thinnable> {
    raw: ptr::NonNull<Erased>,
    marker: PhantomData<Box<T>>,
}

thin_holder!(for ThinBox<T> as Box<T> with make_fat_mut);
std_traits!(for ThinBox<T> as T where T: ?Sized + Thinnable);

impl<T: ?Sized + Thinnable> DerefMut for ThinBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *T::make_fat_mut(self.raw).as_ptr() }
    }
}

impl<T: ?Sized + Thinnable> ThinBox<T> {
    pub fn new(slice: Vec<T::SliceItem>, head: impl FnOnce(&[T::SliceItem]) -> T::Head) -> Self {
        let head = head(&slice);
        let len = T::get_length(&head);

        let head_layout = Layout::new::<T::Head>();
        let (layout, slice_offset) = extend_layout(
            &head_layout,
            layout_array::<T::SliceItem>(len)
                .unwrap_or_else(|e| panic!("oversize allocation: {}", e)),
        )
        .unwrap_or_else(|e| panic!("oversize allocation: {}", e));
        let layout = pad_layout_to_align(&layout);

        unsafe {
            let ptr = ptr::NonNull::new(alloc(layout))
                .unwrap_or_else(|| handle_alloc_error(layout))
                .as_ptr();
            ptr::write(ptr.cast(), head);

            let slice_ptr = ptr.add(slice_offset);
            let slice = mem::transmute::<_, Vec<ManuallyDrop<T::SliceItem>>>(slice);
            ptr::copy_nonoverlapping(slice.as_ptr(), slice_ptr.cast(), len);

            Self::from_thin(ptr::NonNull::new_unchecked(ptr.cast()))
        }
    }
}
