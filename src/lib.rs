//! Boxed custom DSTs that store a slice and the length of said slice inline.
//! Uses the standard library collection types for full interoperability,
//! and also provides thin owned pointers for space-conscious use.
//!
//! # Examples
//!
//! The simplest example is just a boxed slice:
//!
//! ```rust
//! # use thin_dst::*;
//! let boxed_slice = ThinBox::new((), vec![0, 1, 2, 3, 4, 5]);
//! assert_eq!(&*boxed_slice, &[0, 1, 2, 3, 4, 5][..]);
//! let boxed_slice: Box<ThinData<(), u32>> = boxed_slice.into();
//! ```
//!
//! All of the thin collection types are constructed with a "head" and a "tail".
//! The head is any `Sized` type that you would like to associate with the slice.
//! The "tail" is the owned slice of data that you would like to store.
//!
//! This creates a collection of `ThinData`, which acts like `{ head, tail }`,
//! and also handles the `unsafe` required for both custom slice DSTs and thin DST pointers.
//! The most idiomatic usage is to encapsulate the use of thin-dst with a transparent newtype:
//!
//! ```rust
//! # use thin_dst::*; struct NodeHead;
//! #[repr(transparent)]
//! struct NodeData(ThinData<NodeHead, Node>);
//! struct Node(ThinArc<NodeHead, Node>);
//! ```
//!
//! And then use `NodeData` by transmuting and/or [ref-cast]ing as needed.
//!
//!   [ref-cast]: <https://lib.rs/crates/ref-cast>

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
        cmp::{self, PartialEq},
        fmt::{self, Debug},
        hash,
        marker::PhantomData,
        mem::ManuallyDrop,
        ops::{Deref, DerefMut},
        ptr::{self, NonNull},
    },
};

mod polyfill;

/// An erased pointer with size and stride of one byte.
pub type ErasedPtr = NonNull<priv_in_pub::Erased>;
#[doc(hidden)]
pub mod priv_in_pub {
    // This MUST be size=1 such that pointer math actually advances the pointer.
    // FUTURE(extern_types): expose as `extern type` (breaking)
    // This will require casting to NonNull<u8> everywhere for pointer offsetting.
    // But that's not a bad thing. It would have saved a good deal of headache.
    pub struct Erased {
        #[allow(unused)]
        raw: u8,
    }
}

/// A custom slice-holding dynamically sized type.
/// Stores slice length inline to be thin-pointer compatible.
///
/// # Stability
///
/// Note that even though this struct is `#[repr(C)]`,
/// the offsets of its public fields are _not public_.
/// A private field appears before them,
/// so their offset should be treated as being unknown.
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
    /// The sized portion of this DST.
    pub head: Head,
    /// The slice portion of this DST.
    pub slice: [SliceItem],
}

impl<Head, SliceItem> ThinData<Head, SliceItem> {
    fn len(ptr: ErasedPtr) -> NonNull<usize> {
        ptr.cast()
    }

    fn erase(ptr: NonNull<Self>) -> ErasedPtr {
        ptr.cast()
    }

    unsafe fn fatten_const(ptr: ErasedPtr) -> NonNull<Self> {
        let len = ptr::read(Self::len(ptr).as_ptr());
        let slice = make_slice(ptr.cast::<SliceItem>().as_ptr(), len);
        NonNull::new_unchecked(slice as *const Self as *mut Self)
    }

    unsafe fn fatten_mut(ptr: ErasedPtr) -> NonNull<Self> {
        let len = ptr::read(Self::len(ptr).as_ptr());
        let slice = make_slice_mut(ptr.cast::<SliceItem>().as_ptr(), len);
        NonNull::new_unchecked(slice as *mut Self)
    }
}

impl<SliceItem: PartialEq> PartialEq<[SliceItem]> for ThinData<(), SliceItem> {
    fn eq(&self, other: &[SliceItem]) -> bool {
        &self.slice == other
    }
}

macro_rules! thin_holder {
    ( #[nodrop] for $thin:ident<$($a:lifetime,)* Head, SliceItem> as $fat:ident<$($b:lifetime,)* ThinData<Head, SliceItem>> with $fatten:ident ) => {
        impl<$($a,)* Head, SliceItem> $thin<$($a,)* Head, SliceItem> {
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

        impl<$($a,)* Head, SliceItem> From<$fat<$($b,)* ThinData<Head, SliceItem>>> for $thin<$($a,)* Head, SliceItem> {
            fn from(this: $fat<$($b,)* ThinData<Head, SliceItem>>) -> $thin<$($a,)* Head, SliceItem> {
                unsafe {
                    let this = NonNull::new_unchecked($fat::into_raw(this) as *mut _);
                    Self::from_erased(ThinData::<Head, SliceItem>::erase(this))
                }
            }
        }

        impl<$($a,)* Head, SliceItem> Deref for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: Deref,
        {
            type Target = ThinData<Head, SliceItem>;
            fn deref(&self) -> &ThinData<Head, SliceItem> {
                unsafe { &*ThinData::fatten_const(self.raw).as_ptr() }
            }
        }

        impl<$($a,)* Head, SliceItem> DerefMut for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: DerefMut,
        {
            fn deref_mut(&mut self) -> &mut ThinData<Head, SliceItem> {
                unsafe { &mut *ThinData::fatten_mut(self.raw).as_ptr() }
            }
        }

        impl<$($a,)* Head, SliceItem> Debug for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: Debug,
        {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                unsafe {
                    let this = ManuallyDrop::new($fat::from_raw(ThinData::fatten_const(self.raw).as_ptr()));
                    this.fmt(f)
                }
            }
        }

        unsafe impl<$($a,)* Head, SliceItem> Send for $thin<$($a,)* Head, SliceItem> where
            $fat<$($b,)* ThinData<Head, SliceItem>>: Send
        {
        }
        unsafe impl<$($a,)* Head, SliceItem> Sync for $thin<$($a,)* Head, SliceItem> where
            $fat<$($b,)* ThinData<Head, SliceItem>>: Sync
        {
        }

        impl<$($a,)* Head, SliceItem> cmp::Eq for $thin<$($a,)* Head, SliceItem> where
            $fat<$($b,)* ThinData<Head, SliceItem>>: cmp::Eq,
        {
        }
        impl<$($a,)* Head, SliceItem> PartialEq for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: PartialEq,
        {
            fn eq(&self, other: &Self) -> bool {
                unsafe {
                    let other = ManuallyDrop::new($fat::from_raw(ThinData::fatten_const(other.raw).as_ptr()));
                    <Self as PartialEq<$fat<$($b,)* ThinData<Head, SliceItem>>>>::eq(self, &other)
                }
            }
        }
        impl<$($a,)* Head, SliceItem> PartialEq<$fat<$($b,)* ThinData<Head, SliceItem>>> for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: PartialEq,
        {
            fn eq(&self, other: &$fat<$($b,)* ThinData<Head, SliceItem>>) -> bool {
                unsafe {
                    let this = ManuallyDrop::new($fat::from_raw(ThinData::fatten_const(self.raw).as_ptr()));
                    <$fat<$($b,)* ThinData<Head, SliceItem>> as PartialEq>::eq(&this, other)
                }
            }
        }

        impl<$($a,)* Head, SliceItem> hash::Hash for $thin<$($a,)* Head, SliceItem>
        where
            $fat<$($b,)* ThinData<Head, SliceItem>>: hash::Hash,
        {
            fn hash<H>(&self, state: &mut H)
            where
                H: hash::Hasher,
            {
                unsafe {
                    let this = ManuallyDrop::new($fat::from_raw(ThinData::fatten_const(self.raw).as_ptr()));
                    <$fat<$($b,)* ThinData<Head, SliceItem>> as hash::Hash>::hash(&this, state)
                }
            }
        }
    };
    ( for $thin:ident<$($a:lifetime,)* Head, SliceItem> as $fat:ident<$($b:lifetime,)* ThinData<Head, SliceItem>> with $fatten:ident ) => {
        impl<$($a,)* Head, SliceItem> Drop for $thin<$($a,)* Head, SliceItem> {
            fn drop(&mut self) {
                let this = unsafe { $fat::from_raw(ThinData::$fatten(self.raw).as_ptr()) };
                drop::<$fat<$($b,)* ThinData<Head, SliceItem>>>(this)
            }
        }

        thin_holder!(#[nodrop] for $thin<$($a,)* Head, SliceItem> as $fat<$($b,)* ThinData<Head, SliceItem>> with $fatten );
    };
}

/// A thin version of [`Box`].
///
///   [`Box`]: <https://doc.rust-lang.org/stable/std/boxed/struct.Box.html>
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

    unsafe fn alloc(len: usize, layout: Layout) -> NonNull<ThinData<Head, SliceItem>> {
        let ptr: ErasedPtr = NonNull::new(alloc(layout))
            .unwrap_or_else(|| handle_alloc_error(layout))
            .cast();
        ptr::write(ThinData::<Head, SliceItem>::len(ptr).as_ptr(), len);
        ThinData::fatten_mut(ptr.cast())
    }

    /// Create a new boxed `ThinData` with the given head and slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice iterator incorrectly reports its length.
    pub fn new<I>(head: Head, slice: I) -> Self
    where
        I: IntoIterator<Item = SliceItem>,
        I::IntoIter: ExactSizeIterator, // + TrustedLen
    {
        struct InProgress<Head, SliceItem> {
            raw: NonNull<ThinData<Head, SliceItem>>,
            written_len: usize,
            layout: Layout,
            head_offset: usize,
            slice_offset: usize,
        }

        impl<Head, SliceItem> Drop for InProgress<Head, SliceItem> {
            fn drop(&mut self) {
                let raw_ptr = ThinData::erase(self.raw).as_ptr();
                unsafe {
                    let slice = make_slice_mut(
                        raw_ptr.add(self.slice_offset).cast::<SliceItem>(),
                        self.written_len,
                    );
                    ptr::drop_in_place(slice);
                    dealloc(raw_ptr.cast(), self.layout);
                }
            }
        }

        impl<Head, SliceItem> InProgress<Head, SliceItem> {
            fn raw_ptr(&self) -> ErasedPtr {
                ThinData::erase(self.raw)
            }

            fn new(len: usize) -> Self {
                let (layout, [_, head_offset, slice_offset]) =
                    ThinBox::<Head, SliceItem>::layout(len)
                        .unwrap_or_else(|e| panic!("oversize box: {}", e));
                InProgress {
                    raw: unsafe { ThinBox::alloc(len, layout) },
                    written_len: 0,
                    layout,
                    head_offset,
                    slice_offset,
                }
            }

            unsafe fn push(&mut self, item: SliceItem) {
                self.raw_ptr()
                    .as_ptr()
                    .add(self.slice_offset)
                    .cast::<SliceItem>()
                    .add(self.written_len)
                    .write(item);
                self.written_len += 1;
            }

            unsafe fn finish(self, head: Head) -> ThinBox<Head, SliceItem> {
                let this = ManuallyDrop::new(self);
                let ptr = this.raw_ptr();
                ptr::write(ptr.as_ptr().add(this.head_offset).cast(), head);
                let out = ThinBox::from_erased(ptr);
                assert_eq!(this.layout, Layout::for_value(&*out));
                out
            }
        }

        let mut items = slice.into_iter();
        let len = items.len();

        unsafe {
            let mut this = InProgress::new(len);

            for _ in 0..len {
                let slice_item = items
                    .next()
                    .expect("ExactSizeIterator over-reported length");
                this.push(slice_item);
            }
            assert!(
                items.next().is_none(),
                "ExactSizeIterator under-reported length"
            );

            this.finish(head)
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
    // TODO: this should be able to just be
    //     ThinBox::new(self.head.clone(), self.slice.iter().cloned())
    fn clone(&self) -> Self {
        ThinBox::new(self.head.clone(), self.slice.iter().cloned())
    }
}

/// A thin version of [`Arc`].
///
///   [`Arc`]: <https://doc.rust-lang.org/stable/std/sync/struct.Arc.html>
pub struct ThinArc<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<Arc<ThinData<Head, SliceItem>>>,
}

thin_holder!(for ThinArc<Head, SliceItem> as Arc<ThinData<Head, SliceItem>> with fatten_const);

impl<Head, SliceItem> ThinArc<Head, SliceItem> {
    /// Create a new atomically reference counted `ThinData` with the given head and slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice iterator incorrectly reports its length.
    ///
    /// # Note on allocation
    ///
    /// This currently creates a `ThinBox` first and then moves that into an `Arc`.
    /// This is required, because the heap layout of `Arc` is not stable,
    /// and custom DSTs need to be manually allocated.
    ///
    /// This will be eliminated in the future if/when the
    /// reference counted heap layout is stabilized.
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

impl<Head, SliceItem> From<ThinArc<Head, SliceItem>> for Arc<ThinData<Head, SliceItem>> {
    fn from(this: ThinArc<Head, SliceItem>) -> Self {
        unsafe {
            let this = ManuallyDrop::new(this);
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

/// A thin version of [`Rc`].
///
///   [`Rc`]: <https://doc.rust-lang.org/stable/std/rc/struct.Rc.html>
pub struct ThinRc<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<Rc<ThinData<Head, SliceItem>>>,
}

thin_holder!(for ThinRc<Head, SliceItem> as Rc<ThinData<Head, SliceItem>> with fatten_const);

impl<Head, SliceItem> ThinRc<Head, SliceItem> {
    /// Create a new reference counted `ThinData` with the given head and slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice iterator incorrectly reports its length.
    ///
    /// # Note on allocation
    ///
    /// This currently creates a `ThinBox` first and then moves that into an `Rc`.
    /// This is required, because the heap layout of `Rc` is not stable,
    /// and custom DSTs need to be manually allocated.
    ///
    /// This will be eliminated in the future if/when the
    /// reference counted heap layout is stabilized.
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

impl<Head, SliceItem> From<ThinRc<Head, SliceItem>> for Rc<ThinData<Head, SliceItem>> {
    fn from(this: ThinRc<Head, SliceItem>) -> Self {
        unsafe {
            let this = ManuallyDrop::new(this);
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

pub struct ThinRef<'a, Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<&'a ThinData<Head, SliceItem>>,
}

thin_holder!(#[nodrop] for ThinRef<'a, Head, SliceItem> as Ref<'a, ThinData<Head, SliceItem>> with fatten_const);

impl<'a, Head, SliceItem> Copy for ThinRef<'a, Head, SliceItem> where
    &'a ThinData<Head, SliceItem>: Copy
{
}
impl<'a, Head, SliceItem> Clone for ThinRef<'a, Head, SliceItem>
where
    &'a ThinData<Head, SliceItem>: Clone,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, Head, SliceItem> From<ThinRef<'a, Head, SliceItem>> for &'a ThinData<Head, SliceItem> {
    fn from(this: ThinRef<'a, Head, SliceItem>) -> Self {
        unsafe { Ref::from_raw(ThinData::fatten_const(this.raw).as_ptr()) }
    }
}

pub struct ThinRefMut<'a, Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<&'a mut ThinData<Head, SliceItem>>,
}

thin_holder!(#[nodrop] for ThinRefMut<'a, Head, SliceItem> as Ref<'a, ThinData<Head, SliceItem>> with fatten_const);

impl<'a, Head, SliceItem> From<ThinRefMut<'a, Head, SliceItem>>
    for &'a mut ThinData<Head, SliceItem>
{
    fn from(this: ThinRefMut<'a, Head, SliceItem>) -> Self {
        unsafe { RefMut::from_raw(ThinData::fatten_mut(this.raw).as_ptr()) }
    }
}

pub struct ThinPtr<Head, SliceItem> {
    raw: ErasedPtr,
    marker: PhantomData<NonNull<ThinData<Head, SliceItem>>>,
}

thin_holder!(#[nodrop] for ThinPtr<Head, SliceItem> as NonNull<ThinData<Head, SliceItem>> with fatten_mut);

impl<Head, SliceItem> Copy for ThinPtr<Head, SliceItem> where
    NonNull<ThinData<Head, SliceItem>>: Copy
{
}
impl<Head, SliceItem> Clone for ThinPtr<Head, SliceItem>
where
    NonNull<ThinData<Head, SliceItem>>: Clone,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<Head, SliceItem> From<ThinPtr<Head, SliceItem>> for NonNull<ThinData<Head, SliceItem>> {
    fn from(this: ThinPtr<Head, SliceItem>) -> Self {
        unsafe { ThinData::fatten_mut(this.raw) }
    }
}

#[allow(
    missing_docs,
    clippy::missing_safety_doc,
    clippy::should_implement_trait
)]
impl<Head, SliceItem> ThinPtr<Head, SliceItem> {
    pub unsafe fn as_ptr(self) -> *mut ThinData<Head, SliceItem> {
        let nn: NonNull<_> = self.into();
        nn.as_ptr()
    }
    pub unsafe fn as_ref(&self) -> &ThinData<Head, SliceItem> {
        &*self.as_ptr()
    }
    pub unsafe fn as_mut(&mut self) -> &mut ThinData<Head, SliceItem> {
        &mut *self.as_ptr()
    }
}

// helpers for implementing ThinRef[Mut] and ThinPtr[Mut]

unsafe trait RawExt<T: ?Sized> {
    unsafe fn from_raw(ptr: *const T) -> Self;
    unsafe fn into_raw(self) -> *const T;
}

unsafe trait RawMutExt<T: ?Sized> {
    unsafe fn from_raw(ptr: *mut T) -> Self;
    unsafe fn into_raw(self) -> *mut T;
}

type Ref<'a, T> = &'a T;
unsafe impl<'a, T: ?Sized> RawExt<T> for Ref<'a, T> {
    unsafe fn from_raw(ptr: *const T) -> Self {
        &*ptr
    }

    unsafe fn into_raw(self) -> *const T {
        self
    }
}

type RefMut<'a, T> = &'a mut T;
unsafe impl<'a, T: ?Sized> RawMutExt<T> for RefMut<'a, T> {
    unsafe fn from_raw(ptr: *mut T) -> Self {
        &mut *ptr
    }

    unsafe fn into_raw(self) -> *mut T {
        self
    }
}

unsafe impl<T: ?Sized> RawMutExt<T> for NonNull<T> {
    unsafe fn from_raw(ptr: *mut T) -> Self {
        NonNull::new_unchecked(ptr)
    }

    unsafe fn into_raw(self) -> *mut T {
        NonNull::as_ptr(self)
    }
}
