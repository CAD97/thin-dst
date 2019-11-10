use {
    crate::Thinnable,
    core::{
        cmp,
        ops::{Deref, DerefMut},
    },
};

#[repr(C)]
pub struct ThinSlice<T> {
    length: usize,
    raw: [T],
}

total_std_traits!(for ThinSlice<T> as [T]);

impl<T> Deref for ThinSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl<T> DerefMut for ThinSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.raw
    }
}

unsafe impl<T> Thinnable for ThinSlice<T> {
    type Head = usize;
    type SliceItem = T;

    make_fat!();
    fn get_length(head: &usize) -> usize {
        *head
    }
}
