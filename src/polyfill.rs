//! Polyfills for unstable features `slice_from_raw_parts` and `alloc_layout_extra`,
//! along with a theoretical `fn repr_c` to compute `#[repr(C)]` layouts.

pub(crate) use self::slice_from_raw_parts::{make_slice, make_slice_mut};

#[cfg(not(slice_from_raw_parts))] // https://github.com/rust-lang/rust/issues/36925
mod slice_from_raw_parts {
    use core::slice;
    pub(crate) unsafe fn make_slice<T>(data: *const T, len: usize) -> *const [T] {
        slice::from_raw_parts(data, len) as *const [T]
    }
    pub(crate) unsafe fn make_slice_mut<T>(data: *mut T, len: usize) -> *mut [T] {
        slice::from_raw_parts_mut(data, len) as *mut [T]
    }
}

#[cfg(slice_from_raw_parts)] // https://github.com/rust-lang/rust/issues/36925
mod slice_from_raw_parts {
    use core::ptr;
    pub(crate) use ptr::slice_from_raw_parts as make_slice;
    pub(crate) use ptr::slice_from_raw_parts_mut as make_slice_mut;
}

pub(crate) use alloc_layout_extra::{extend_layout, layout_array, pad_layout_to_align};

#[cfg(not(alloc_layout_extra))] // https://github.com/rust-lang/rust/issues/55724
mod alloc_layout_extra {
    use core::{
        alloc::{Layout, LayoutErr},
        cmp,
    };

    fn layout_err() -> LayoutErr {
        Layout::from_size_align(0, 0).unwrap_err()
    }

    pub(crate) fn extend_layout(this: &Layout, next: Layout) -> Result<(Layout, usize), LayoutErr> {
        let new_align = cmp::max(this.align(), next.align());
        let pad = layout_padding_needed_for(&this, next.align());
        let offset = this.size().checked_add(pad).ok_or_else(layout_err)?;
        let new_size = offset.checked_add(next.size()).ok_or_else(layout_err)?;
        let layout = Layout::from_size_align(new_size, new_align)?;
        Ok((layout, offset))
    }

    pub(crate) fn layout_array<T>(n: usize) -> Result<Layout, LayoutErr> {
        repeat_layout(&Layout::new::<T>(), n).map(|(k, _)| k)
    }

    pub(crate) fn pad_layout_to_align(this: &Layout) -> Layout {
        let pad = layout_padding_needed_for(this, this.align());
        let new_size = this.size() + pad;
        unsafe { Layout::from_size_align_unchecked(new_size, this.align()) }
    }

    fn layout_padding_needed_for(this: &Layout, align: usize) -> usize {
        let len = this.size();
        let len_rounded_up = len.wrapping_add(align).wrapping_sub(1) & !align.wrapping_sub(1);
        len_rounded_up.wrapping_sub(len)
    }

    fn repeat_layout(this: &Layout, n: usize) -> Result<(Layout, usize), LayoutErr> {
        let padded_size = pad_layout_to_align(this).size();
        let alloc_size = padded_size.checked_mul(n).ok_or_else(layout_err)?;
        unsafe {
            Ok((
                Layout::from_size_align_unchecked(alloc_size, this.align()),
                padded_size,
            ))
        }
    }
}

#[cfg(alloc_layout_extra)] // https://github.com/rust-lang/rust/issues/55724
mod alloc_layout_extra {
    use core::alloc::{Layout, LayoutErr};
    pub(crate) fn extend_layout(this: &Layout, next: Layout) -> Result<(Layout, usize), LayoutErr> {
        this.extend(next)
    }
    pub(crate) fn layout_array<T>(n: usize) -> Result<Layout, LayoutErr> {
        Layout::array::<T>(n)
    }
    pub(crate) fn pad_layout_to_align(this: &Layout) -> Layout {
        this.pad_to_align().unwrap()
    }
}

use core::alloc::{Layout, LayoutErr};
pub fn repr_c_3(fields: [Layout; 3]) -> Result<(Layout, [usize; 3]), LayoutErr> {
    let mut offsets = [0; 3];
    let mut layout = fields[0];
    for i in 1..3 {
        let (new_layout, this_offset) = extend_layout(&layout, fields[i])?;
        layout = new_layout;
        offsets[i] = this_offset;
    }
    Ok((pad_layout_to_align(&layout), offsets))
}
