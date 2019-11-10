//! These tests don't really assert anything, they just exercise the API.
//! This is mainly such that the tests can be run under miri as a sanitizer.

#![allow(unused, clippy::redundant_clone)]

use {
    std::sync::Arc,
    thin::{self, make_fat, Thinnable},
};

#[test]
fn slice() {
    let slice: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let slice: thin::Box<thin::Slice<u32>> = slice.into();
    assert_eq!(slice, [0, 1, 2, 3, 4, 5]);
    let slice = slice.clone();
}

#[derive(Debug, Default)]
struct Data {
    node_kind: u16,
    source_len: u32,
    children_len: u16,
}

#[derive(Debug)]
#[repr(C)]
struct Node {
    data: Data, // MUST include length of the following slice!
    children: [thin::Arc<Node>],
}

unsafe impl Thinnable for Node {
    type Head = Data;
    type SliceItem = thin::Arc<Node>;
    make_fat!();
    fn get_length(head: &Data) -> usize {
        head.children_len as usize
    }
}

#[test]
fn node() {
    let a: thin::Arc<Node> = thin::Arc::new(vec![], |_| Data::default());
    let b: thin::Arc<Node> = thin::Arc::new(vec![], |_| Data::default());
    let c: thin::Arc<Node> = thin::Arc::new(vec![], |_| Data::default());
    let boxed: thin::Arc<Node> = thin::Arc::new(
        vec![a.clone(), b.clone(), c.clone()],
        |children: &[thin::Arc<Node>]| Data {
            children_len: 3,
            node_kind: 1,
            source_len: children.iter().map(|child| child.data.source_len).sum(),
        },
    );
    dbg!(boxed);
}
