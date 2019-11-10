use {
    crate::Thinnable,
    alloc::{vec::Vec, boxed::Box},
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

impl<T> From<Vec<T>> for thin::Box<ThinSlice<T>> {
    fn from(this: Vec<T>) -> Self {
        thin::Box::new(this, <[_]>::len)
    }
}

impl<T> From<Vec<T>> for thin::Arc<ThinSlice<T>> {
    fn from(this: Vec<T>) -> Self {
        thin::Arc::new(this, <[_]>::len)
    }
}

impl<T> From<Vec<T>> for thin::Rc<ThinSlice<T>> {
    fn from(this: Vec<T>) -> Self {
        thin::Rc::new(this, <[_]>::len)
    }
}

impl<T: Clone> Clone for Box<thin::Slice<T>> {
    fn clone(&self) -> Self {
        // FUTURE: do something like <Box<[T]> as Clone>::clone
        let vec: Vec<T> = self.iter().cloned().collect();
        thin::Box::into_fat(thin::Box::from(vec))
    }
}
