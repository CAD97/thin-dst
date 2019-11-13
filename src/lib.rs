#![no_std]
extern crate alloc;

use {
    crate::polyfill::*,
    alloc::{
        alloc::{alloc, dealloc, handle_alloc_error, Layout, LayoutErr},
        boxed::Box,
        rc::Rc,
        sync::Arc,
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

mod polyfill;

pub type ErasedPtr = ptr::NonNull<priv_in_pub::Erased>;
#[doc(hidden)]
pub mod priv_in_pub {
    // FUTURE(extern_types): expose as `extern type`
    pub struct Erased {
        #[allow(unused)]
        raw: (),
    }
}

#[repr(C)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ThinData<Head, SliceItem> {
    // NB: Optimal layout packing is
    //     align(usize) < align(head) => head before length
    //     align(head) < align(usize) => length before head
    // We put length first because
    //     a) it's much simpler to go from ErasedPtr to ptr-to-length
    //     b) it's rare for types to have align > align(usize)
    // For optimality, we should use repr(Rust) and pointer projection or offset_of!
    // We don't do that for now since we can avoid the unsoundness of offset_of!,
    // and offset_of! doesn't work for ?Sized types anyway.
    // SAFETY: must be length of self.slice
    length: usize,
    pub head: Head,
    pub slice: [SliceItem],
}

impl<Head, SliceItem> ThinData<Head, SliceItem> {
    fn length(ptr: ErasedPtr) -> ptr::NonNull<usize> {
        ptr.cast()
    }

    fn erase(ptr: ptr::NonNull<Self>) -> ErasedPtr {
        ptr.cast()
    }

    unsafe fn fatten_const(ptr: ErasedPtr) -> ptr::NonNull<Self> {
        let len = ptr::read(Self::length(ptr).as_ptr());
        let slice = make_slice(ptr.as_ptr(), len);
        ptr::NonNull::new_unchecked(slice as *const Self as *mut Self)
    }

    unsafe fn fatten_mut(ptr: ErasedPtr) -> ptr::NonNull<Self> {
        let len = ptr::read(Self::length(ptr).as_ptr());
        let slice = make_slice_mut(ptr.as_ptr(), len);
        ptr::NonNull::new_unchecked(slice as *mut Self)
    }
}

macro_rules! thin_holder {
    ( for $thin:ident<Head, SliceItem> as $fat:ident<ThinData<Head, SliceItem>> with $fatten:ident ) => {
        impl<Head, SliceItem> Drop for $thin<Head, SliceItem> {
            fn drop(&mut self) {
                let this = unsafe { $fat::from_raw(ThinData::$fatten(self.raw).as_ptr()) };
                drop::<$fat<ThinData<Head, SliceItem>>>(this)
            }
        }

        impl<Head, SliceItem> $thin<Head, SliceItem> {
            /// Construct an owned pointer from an erased pointer.
            ///
            /// # Safety
            ///
            /// This pointer must logically own a valid instance of `Self`.
            pub unsafe fn from_erased(ptr: ErasedPtr) -> Self {
                Self {
                    raw: ptr,
                    marker: PhantomData,
                }
            }

            /// Convert this owned pointer into an erased pointer.
            ///
            /// To avoid a memory leak the pointer must be converted back
            /// using `Self::from_erased`.
            pub fn erase(this: Self) -> ErasedPtr {
                let this = ManuallyDrop::new(this);
                this.raw
            }
        }

        impl<Head, SliceItem> From<$fat<ThinData<Head, SliceItem>>> for $thin<Head, SliceItem> {
            fn from(this: $fat<ThinData<Head, SliceItem>>) -> $thin<Head, SliceItem> {
                unsafe {
                    let this = ptr::NonNull::new_unchecked($fat::into_raw(this) as *mut _);
                    Self::from_erased(ThinData::<Head, SliceItem>::erase(this))
                }
            }
        }

        impl<Head, SliceItem> Deref for $thin<Head, SliceItem>
        where
            $fat<ThinData<Head, SliceItem>>: Deref,
        {
            type Target = ThinData<Head, SliceItem>;
            fn deref(&self) -> &ThinData<Head, SliceItem> {
                unsafe { &*ThinData::fatten_const(self.raw).as_ptr() }
            }
        }

        impl<Head, SliceItem> DerefMut for $thin<Head, SliceItem>
        where
            $fat<ThinData<Head, SliceItem>>: DerefMut,
        {
            fn deref_mut(&mut self) -> &mut ThinData<Head, SliceItem> {
                unsafe { &mut *ThinData::fatten_mut(self.raw).as_ptr() }
            }
        }

        impl<Head, SliceItem> core::fmt::Debug for $thin<Head, SliceItem>
        where
            ThinData<Head, SliceItem>: core::fmt::Debug,
        {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                <ThinData<Head, SliceItem> as core::fmt::Debug>::fmt(self, f)
            }
        }

        unsafe impl<Head, SliceItem> Send for $thin<Head, SliceItem> where
            $fat<ThinData<Head, SliceItem>>: Send
        {
        }
        unsafe impl<Head, SliceItem> Sync for $thin<Head, SliceItem> where
            $fat<ThinData<Head, SliceItem>>: Sync
        {
        }

        impl<Head, SliceItem> cmp::Eq for $thin<Head, SliceItem> where
            ThinData<Head, SliceItem>: cmp::Eq
        {
        }
        impl<Head, SliceItem> cmp::PartialEq for $thin<Head, SliceItem>
        where
            ThinData<Head, SliceItem>: cmp::PartialEq,
        {
            fn eq(&self, other: &Self) -> bool {
                <ThinData<Head, SliceItem> as cmp::PartialEq>::eq(self, other)
            }
        }
    };
}

pub struct ThinBox<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<Box<ThinData<Head, SliceItem>>>,
}

thin_holder!(for ThinBox<Head, SliceItem> as Box<ThinData<Head, SliceItem>> with fatten_mut);

impl<Head, SliceItem> ThinBox<Head, SliceItem> {
    fn layout(len: usize) -> Result<(Layout, [usize; 3]), LayoutErr> {
        let length_layout = Layout::new::<usize>();
        let head_layout = Layout::new::<Head>();
        let slice_layout = layout_array::<SliceItem>(len)?;
        repr_c_3([length_layout, head_layout, slice_layout])
    }

    fn alloc(len: usize, layout: Layout) -> ptr::NonNull<ThinData<Head, SliceItem>> {
        unsafe {
            let ptr =
                ptr::NonNull::new(alloc(layout)).unwrap_or_else(|| handle_alloc_error(layout));
            // SAFETY: length is at offset 0
            ptr::write(ptr.as_ptr().cast(), len);
            ThinData::fatten_mut(ptr.cast())
        }
    }

    pub fn new(head: Head, slice: Vec<SliceItem>) -> Self {
        let len = slice.len();
        let (layout, [_, head_offset, slice_offset]) =
            Self::layout(len).unwrap_or_else(|e| panic!("oversize box: {}", e));
        let ptr = Self::alloc(len, layout);
        let raw_ptr = ThinData::erase(ptr).as_ptr();

        unsafe {
            ptr::write(raw_ptr.add(head_offset).cast(), head);
            let slice_ptr = raw_ptr.add(slice_offset);
            let slice: Vec<ManuallyDrop<SliceItem>> = mem::transmute(slice);
            ptr::copy_nonoverlapping(slice.as_ptr(), slice_ptr.cast(), len);
            Self::from_erased(ThinData::erase(ptr))
        }
    }
}

impl<Head, SliceItem> From<ThinBox<Head, SliceItem>> for Box<ThinData<Head, SliceItem>> {
    fn from(this: ThinBox<Head, SliceItem>) -> Self {
        unsafe {
            let this = ManuallyDrop::new(this);
            Box::from_raw(ThinData::fatten_mut(this.raw).as_ptr())
        }
    }
}

impl<Head, SliceItem> Clone for ThinBox<Head, SliceItem>
where
    Head: Clone,
    SliceItem: Clone,
{
    fn clone(&self) -> Self {
        struct InProgressThinBox<Head, SliceItem> {
            raw: ptr::NonNull<ThinData<Head, SliceItem>>,
            length: usize,
            layout: Layout,
            head_offset: usize,
            slice_offset: usize,
        }

        impl<Head, SliceItem> Drop for InProgressThinBox<Head, SliceItem> {
            fn drop(&mut self) {
                let raw_ptr = ThinData::erase(self.raw).as_ptr();
                unsafe {
                    ptr::drop_in_place(raw_ptr.add(self.head_offset));
                    let slice = make_slice_mut(raw_ptr.add(self.slice_offset), self.length);
                    ptr::drop_in_place(slice);
                    dealloc(raw_ptr.cast(), self.layout);
                }
            }
        }

        impl<Head, SliceItem> InProgressThinBox<Head, SliceItem> {
            unsafe fn finish(self) -> ThinBox<Head, SliceItem> {
                let this = ManuallyDrop::new(self);
                ThinBox::from_erased(ThinData::erase(this.raw))
            }
        }

        unsafe {
            let this = ThinData::<Head, SliceItem>::fatten_const(self.raw);
            let this = this.as_ref();
            let len = this.length;
            let (layout, [_, head_offset, slice_offset]) = Self::layout(len).unwrap();
            let ptr = Self::alloc(len, layout);
            let raw_ptr = ThinData::erase(ptr).as_ptr();
            ptr::write(raw_ptr.add(head_offset).cast(), this.head.clone());
            let mut in_progress = InProgressThinBox {
                raw: ptr,
                length: 0,
                layout,
                head_offset,
                slice_offset,
            };
            for slice_item in &this.slice {
                ptr::write(
                    raw_ptr.add(slice_offset).add(in_progress.length).cast(),
                    slice_item.clone(),
                );
                in_progress.length += 1;
            }
            in_progress.finish()
        }
    }
}

pub struct ThinArc<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<Arc<ThinData<Head, SliceItem>>>,
}

thin_holder!(for ThinArc<Head, SliceItem> as Arc<ThinData<Head, SliceItem>> with fatten_const);

impl<Head, SliceItem> ThinArc<Head, SliceItem> {
    pub fn new(head: Head, slice: Vec<SliceItem>) -> Self {
        // FUTURE(https://internals.rust-lang.org/t/stabilizing-a-rc-layout/11265):
        //     When/if `Arc`'s heap repr is stable, allocate directly rather than `Box` first.
        let boxed: Box<ThinData<Head, SliceItem>> = ThinBox::new(head, slice).into();
        let arc: Arc<ThinData<Head, SliceItem>> = boxed.into();
        arc.into()
    }
}

impl<Head, SliceItem> Into<Arc<ThinData<Head, SliceItem>>> for ThinArc<Head, SliceItem> {
    fn into(self) -> Arc<ThinData<Head, SliceItem>> {
        unsafe {
            let this = ManuallyDrop::new(self);
            Arc::from_raw(ThinData::fatten_const(this.raw).as_ptr())
        }
    }
}

impl<Head, SliceItem> Clone for ThinArc<Head, SliceItem>
where
    Arc<ThinData<Head, SliceItem>>: Clone,
{
    fn clone(&self) -> Self {
        unsafe {
            let this = ManuallyDrop::new(Arc::from_raw(ThinData::fatten_const(self.raw).as_ptr()));
            ManuallyDrop::into_inner(ManuallyDrop::clone(&this)).into()
        }
    }
}

pub struct ThinRc<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<Rc<ThinData<Head, SliceItem>>>,
}

thin_holder!(for ThinRc<Head, SliceItem> as Rc<ThinData<Head, SliceItem>> with fatten_const);

impl<Head, SliceItem> ThinRc<Head, SliceItem> {
    pub fn new(head: Head, slice: Vec<SliceItem>) -> Self {
        // FUTURE(https://internals.rust-lang.org/t/stabilizing-a-rc-layout/11265):
        //     When/if `Rc`'s heap repr is stable, allocate directly rather than `Box` first.
        let boxed: Box<ThinData<Head, SliceItem>> = ThinBox::new(head, slice).into();
        let arc: Rc<ThinData<Head, SliceItem>> = boxed.into();
        arc.into()
    }
}

impl<Head, SliceItem> Into<Rc<ThinData<Head, SliceItem>>> for ThinRc<Head, SliceItem> {
    fn into(self) -> Rc<ThinData<Head, SliceItem>> {
        unsafe {
            let this = ManuallyDrop::new(self);
            Rc::from_raw(ThinData::fatten_const(this.raw).as_ptr())
        }
    }
}

impl<Head, SliceItem> Clone for ThinRc<Head, SliceItem>
where
    Rc<ThinData<Head, SliceItem>>: Clone,
{
    fn clone(&self) -> Self {
        unsafe {
            let this = ManuallyDrop::new(Rc::from_raw(ThinData::fatten_const(self.raw).as_ptr()));
            ManuallyDrop::into_inner(ManuallyDrop::clone(&this)).into()
        }
    }
}
