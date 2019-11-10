#![no_std]
extern crate alloc;
extern crate self as thin;

use {crate::polyfill::*, core::ptr};

mod polyfill;

use priv_in_pub::Erased;
#[doc(hidden)]
pub mod priv_in_pub {
    // FUTURE(extern_types): expose as `extern type`
    pub struct Erased {
        #[allow(unused)]
        raw: (),
    }
}

/// Implement `Thinnable::make_fat`.
///
/// All this does is an `as`-cast between the erased slice type and a self pointer.
#[macro_export]
macro_rules! make_fat {
    () => {
        fn make_fat(ptr: *mut [$crate::priv_in_pub::Erased]) -> *mut Self {
            ptr as *mut Self
        }
    };
}

/// Types that hold an inline slice and the length of that slice in the same allocation.
///
/// # Example
///
/// Consider an `Arc`-tree node:
///
/// ```rust
/// # struct Data; use std::sync::Arc;
/// struct Node {
///     data: Data,
///     children: Vec<Arc<Node>>
/// }
/// ```
///
/// This incurs a double indirection: once to the node and once to the children,
/// even though `Node` may always only be exposed behind an `Arc`.
///
/// Instead, you can store the children slice inline with the data allocation:
///
/// ```rust
/// # struct Data { children_len: usize }; use std::sync::Arc;
/// # use thin::{Thinnable, make_fat};
/// #[repr(C)]
/// struct Node {
///     data: Data, // MUST include length of the following slice!
///     children: [Arc<Node>],
/// }
///
/// unsafe impl Thinnable for Node {
///     type Head = Data;
///     type SliceItem = Arc<Node>;
///
///     make_fat!();
///     fn get_length(head: &Data) -> usize {
///         head.children_len
///     }
/// }
/// ```
///
/// `&Node` and `Arc<Node>` will be like `&[_]` and `Arc<[_]>`; a fat `(pointer, length)` pair.
/// You can also use `thin::Box` and `thin::Arc` to get owned thin pointers.
/// These types are also how you allocate a type defined like this.
///
/// The type _MUST_ be `#[repr(C)]` and composed of just a head the slice tail,
/// otherwise arbitrary functionality of this crate may cause undefined behavior.
pub unsafe trait Thinnable {
    /// The sized part of the allocation.
    type Head: Sized;

    /// The item type of the slice part of the allocation.
    type SliceItem: Sized;

    /// Extract the slice length from the head.
    fn get_length(head: &Self::Head) -> usize;

    /// Make a fat pointer from an erased one.
    ///
    /// This is implemented by calling `make_fat!()`.
    ///
    /// Note that the input erased pointer is not actually a thin pointer;
    /// `make_fat_const` and `make_fat_mut` take a thin erased pointer.
    fn make_fat(ptr: *mut [Erased]) -> *mut Self;

    /// Make an erased thin pointer from a fat one.
    fn make_thin(fat: ptr::NonNull<Self>) -> ptr::NonNull<Erased> {
        fat.cast()
    }

    /// Make a fat pointer from an erased one, using `*const` functions.
    ///
    /// # Safety
    ///
    /// `thin` must be a valid erased pointer to `Self`.
    ///
    /// This materializes a `&`-reference on stable, thus unique mutation
    /// after using this function is not necessarily allowed. Use
    /// `make_fat_mut` instead if you have unique access.
    unsafe fn make_fat_const(thin: ptr::NonNull<Erased>) -> ptr::NonNull<Self> {
        let len = Self::get_length(thin.cast().as_ref());
        ptr::NonNull::new_unchecked(Self::make_fat(make_slice(thin.as_ptr(), len) as *mut _))
    }

    /// Make a fat pointer from an erased one, using `*mut` functions.
    ///
    /// # Safety
    ///
    /// `thin` must be a valid erased pointer to `Self`.
    ///
    /// This materializes a `&mut`-reference on stable, thus should only be
    /// used when the thin pointer is sourced from a unique borrow. Use
    /// `make_fat_const` instead if you have shared access.
    unsafe fn make_fat_mut(thin: ptr::NonNull<Erased>) -> ptr::NonNull<Self> {
        let len = Self::get_length(thin.cast().as_ref());
        ptr::NonNull::new_unchecked(Self::make_fat(make_slice_mut(thin.as_ptr(), len)))
    }
}

macro_rules! thin_holder {
    ( for $thin:ident<T> as $fat:ident<T> with $make_fat:ident ) => {
        impl<T: ?Sized + Thinnable> Drop for $thin<T> {
            fn drop(&mut self) {
                unsafe { drop::<$fat<T>>($fat::from_raw(T::$make_fat(self.raw).as_ptr())) }
            }
        }

        impl<T: ?Sized + Thinnable> $thin<T> {
            /// Construct an owned thin pointer from a raw thin pointer.
            ///
            /// # Safety
            ///
            /// This pointer must logically own a valid instance of `Self`.
            pub unsafe fn from_thin(thin: ptr::NonNull<Erased>) -> Self {
                Self {
                    raw: thin,
                    marker: PhantomData,
                }
            }

            /// Convert this owned thin pointer into a raw thin pointer.
            ///
            /// To avoid a memory leak the pointer must be converted back
            /// using `Self::from_raw`.
            pub fn into_thin(this: Self) -> ptr::NonNull<Erased> {
                let this = ManuallyDrop::new(this);
                this.raw
            }

            /// Construct an owned thin pointer from a raw fat pointer.
            ///
            /// # Safety
            ///
            /// This pointer must logically own a valid instance of `Self`.
            pub unsafe fn from_fat(fat: ptr::NonNull<T>) -> Self {
                Self::from_thin(T::make_thin(fat))
            }

            /// Convert this owned thin pointer into an owned fat pointer.
            ///
            /// This is the std type that this type pretends to be.
            /// You can convert freely between the two representations
            /// with this function and `Self::from(fat)`.
            pub fn into_fat(this: Self) -> $fat<T> {
                unsafe {
                    let this = ManuallyDrop::new(this);
                    $fat::from_raw(T::$make_fat(this.raw).as_ptr())
                }
            }
        }

        impl<T: ?Sized + Thinnable> From<$fat<T>> for $thin<T> {
            fn from(this: $fat<T>) -> $thin<T> {
                unsafe {
                    $thin::from_fat(ptr::NonNull::new_unchecked($fat::into_raw(this) as *mut _))
                }
            }
        }

        impl<T: ?Sized + Thinnable> Clone for $thin<T>
        where
            $fat<T>: Clone,
        {
            fn clone(&self) -> Self {
                unsafe {
                    let this =
                        ManuallyDrop::new($fat::from_raw(T::make_fat_const(self.raw).as_ptr()));
                    ManuallyDrop::into_inner(ManuallyDrop::clone(&this)).into()
                }
            }
        }

        impl<T: ?Sized + Thinnable> Deref for $thin<T> {
            type Target = T;
            fn deref(&self) -> &T {
                unsafe { &*T::make_fat_const(self.raw).as_ptr() }
            }
        }
    };
}

macro_rules! std_traits {
    (for $ty:ident $(<$T:ident>)? as $raw:ty $(where $($tt:tt)*)?) => {
        impl $(<$T>)? core::fmt::Debug for $ty $(<$T>)? where $raw: core::fmt::Debug, $($($tt)*)? {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                <$raw as core::fmt::Debug>::fmt(self, f)
            }
        }
        impl $(<$T>)? core::fmt::Display for $ty $(<$T>)? where $raw: core::fmt::Display, $($($tt)*)? {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                <$raw as core::fmt::Display>::fmt(self, f)
            }
        }
        impl<$($T,)? O: ?Sized> cmp::PartialOrd<O> for $ty $(<$T>)? where $raw: cmp::PartialOrd<O>, $($($tt)*)? {
            fn partial_cmp(&self, other: &O) -> Option<cmp::Ordering> {
                <$raw as cmp::PartialOrd<O>>::partial_cmp(self, other)
            }
        }
        impl<$($T,)? O: ?Sized> cmp::PartialEq<O> for $ty $(<$T>)? where $raw: cmp::PartialEq<O>, $($($tt)*)? {
            fn eq(&self, other: &O) -> bool {
                <$raw as cmp::PartialEq<O>>::eq(self, other)
            }
        }
    };
}

// FUTURE: I think it requires specialization? Get PartialEq/PartialOrd/Eq/Ord for `thin_holder!`s
macro_rules! total_std_traits {
    (for $ty:ident $(<$T:ident>)? as $raw:ty $(where $($tt:tt)*)?) => {
        std_traits!(for $ty $(<$T>)? as $raw $(where $($tt:tt)*)?);

        impl $(<$T>)? cmp::Eq for $ty $(<$T>)? where $raw: cmp::Eq, $($($tt)*)? {}
        impl $(<$T>)? cmp::Ord for $ty $(<$T>)? where $raw: cmp::Ord, $($($tt)*)? {
            fn cmp(&self, other: &Self) -> cmp::Ordering {
                <$raw as cmp::Ord>::cmp(self, other)
            }
        }
        impl $(<$T>)? cmp::PartialEq for $ty $(<$T>)? where $raw: cmp::PartialEq, $($($tt)*)? {
            fn eq(&self, other: &Self) -> bool {
                <$raw as cmp::PartialEq>::eq(self, other)
            }
        }
        impl $(<$T>)? cmp::PartialOrd for $ty $(<$T>)? where $raw: cmp::PartialOrd, $($($tt)*)? {
            fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
                <$raw as cmp::PartialOrd>::partial_cmp(self, other)
            }
        }
    };
}

mod slice;
pub use slice::ThinSlice as Slice;

// TODO: consider pros/cons of ThinStr

mod boxed;
pub use boxed::ThinBox as Box;

mod rc;
pub use rc::ThinRc as Rc;

mod arc;
pub use arc::ThinArc as Arc;
