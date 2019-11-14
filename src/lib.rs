#![no_std]
extern crate alloc;

use {
    crate::polyfill::*,
    alloc::{
        alloc::{alloc, dealloc, handle_alloc_error, Layout, LayoutErr},
        boxed::Box,
        rc::Rc,
        sync::Arc,
    },
    core::{
        cmp, hash,
        marker::PhantomData,
        mem::ManuallyDrop,
        ops::{Deref, DerefMut},
        ptr,
    },
};

mod polyfill;

pub type ErasedPtr = ptr::NonNull<priv_in_pub::Erased>;
#[doc(hidden)]
pub mod priv_in_pub {
    // This MUST be size=1 such that pointer math actually advances the pointer.
    // FUTURE(extern_types): expose as `extern type`
    // This will require casting to ptr::NonNull<u8> everywhere for pointer offsetting.
    // But that's not a bad thing. It would have saved a good deal of headache.
    pub struct Erased {
        #[allow(unused)]
        raw: u8,
    }
}

#[repr(C)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ThinData<Head, SliceItem> {
    // NB: Optimal layout packing is
    //     align(usize) < align(head) => head before len
    //     align(head) < align(usize) => len before head
    // We put len first because
    //     a) it's much simpler to go from ErasedPtr to ptr-to-length
    //     b) it's rare for types to have align > align(usize)
    // For optimality, we should use repr(Rust) and pointer projection or offset_of!
    // We don't do that for now since we can avoid the unsoundness of offset_of!,
    // and offset_of! doesn't work for ?Sized types anyway.
    // SAFETY: must be length of self.slice
    len: usize,
    pub head: Head,
    pub slice: [SliceItem],
}

impl<Head, SliceItem> ThinData<Head, SliceItem> {
    fn len(ptr: ErasedPtr) -> ptr::NonNull<usize> {
        ptr.cast()
    }

    fn erase(ptr: ptr::NonNull<Self>) -> ErasedPtr {
        ptr.cast()
    }

    unsafe fn fatten_const(ptr: ErasedPtr) -> ptr::NonNull<Self> {
        let len = ptr::read(Self::len(ptr).as_ptr());
        let slice = make_slice(ptr.cast::<SliceItem>().as_ptr(), len);
        ptr::NonNull::new_unchecked(slice as *const Self as *mut Self)
    }

    unsafe fn fatten_mut(ptr: ErasedPtr) -> ptr::NonNull<Self> {
        let len = ptr::read(Self::len(ptr).as_ptr());
        let slice = make_slice_mut(ptr.cast::<SliceItem>().as_ptr(), len);
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

        impl<Head, SliceItem> hash::Hash for $thin<Head, SliceItem>
        where
            ThinData<Head, SliceItem>: hash::Hash,
        {
            fn hash<H>(&self, state: &mut H)
            where
                H: hash::Hasher,
            {
                <ThinData<Head, SliceItem> as hash::Hash>::hash(self, state)
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

    unsafe fn alloc(len: usize, layout: Layout) -> ptr::NonNull<ThinData<Head, SliceItem>> {
        let ptr: ErasedPtr = ptr::NonNull::new(alloc(layout))
            .unwrap_or_else(|| handle_alloc_error(layout))
            .cast();
        ptr::write(ThinData::<Head, SliceItem>::len(ptr).as_ptr(), len);
        ThinData::fatten_mut(ptr.cast())
    }

    pub fn new<I>(head: Head, slice: I) -> Self
    where
        I: IntoIterator<Item = SliceItem>,
        I::IntoIter: ExactSizeIterator, // + TrustedLen
    {
        let mut items = slice.into_iter();
        let len = items.len();
        let (layout, [_, head_offset, slice_offset]) =
            Self::layout(len).unwrap_or_else(|e| panic!("oversize box: {}", e));

        unsafe {
            let ptr = Self::alloc(len, layout);
            let raw_ptr = ThinData::erase(ptr).as_ptr();
            ptr::write(raw_ptr.add(head_offset).cast(), head);
            let mut slice_ptr = raw_ptr.add(slice_offset).cast::<SliceItem>();
            for _ in 0..len {
                let slice_item = items
                    .next()
                    .expect("ExactSizeIterator over-reported length");
                ptr::write(slice_ptr, slice_item);
                slice_ptr = slice_ptr.offset(1);
            }
            assert!(
                items.next().is_none(),
                "ExactSizeIterator under-reported length"
            );
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
            len: usize,
            layout: Layout,
            head_offset: usize,
            slice_offset: usize,
        }

        impl<Head, SliceItem> Drop for InProgressThinBox<Head, SliceItem> {
            fn drop(&mut self) {
                let raw_ptr = ThinData::erase(self.raw).as_ptr();
                unsafe {
                    ptr::drop_in_place(raw_ptr.add(self.head_offset));
                    let slice = make_slice_mut(
                        raw_ptr.add(self.slice_offset).cast::<SliceItem>(),
                        self.len,
                    );
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
            let len = this.len;
            let (layout, [_, head_offset, slice_offset]) = Self::layout(len).unwrap();
            let ptr = Self::alloc(len, layout);
            let raw_ptr = ThinData::erase(ptr).as_ptr();
            ptr::write(raw_ptr.add(head_offset).cast(), this.head.clone());
            let mut in_progress = InProgressThinBox {
                raw: ptr,
                len: 0,
                layout,
                head_offset,
                slice_offset,
            };
            let slice_ptr = raw_ptr.add(slice_offset).cast::<SliceItem>();
            for slice_item in &this.slice {
                ptr::write(slice_ptr.add(in_progress.len), slice_item.clone());
                in_progress.len += 1;
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
    pub fn new<I>(head: Head, slice: I) -> Self
    where
        I: IntoIterator<Item = SliceItem>,
        I::IntoIter: ExactSizeIterator, // + TrustedLen
    {
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
    pub fn new<I>(head: Head, slice: I) -> Self
    where
        I: IntoIterator<Item = SliceItem>,
        I::IntoIter: ExactSizeIterator, // + TrustedLen
    {
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
