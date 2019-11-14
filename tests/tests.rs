//! These tests don't really assert anything, they just exercise the API.
//! This is mainly such that the tests can be run under miri as a sanitizer.

#![allow(unused, clippy::redundant_clone)]

use {std::sync::Arc, thin_dst::*};

#[test]
fn slice() {
    let slice: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let slice: ThinBox<(), u32> = ThinBox::new((), slice);
    assert_eq!(slice.slice, [0, 1, 2, 3, 4, 5]);
    let slice = slice.clone();
}

#[test]
fn zst() {
    let slice: Vec<()> = vec![(); 16];
    let slice: ThinBox<(), ()> = ThinBox::new((), slice);
    let slice = slice.clone();
}

type Data = usize;
#[repr(transparent)]
#[derive(Debug, Clone)]
struct Node(ThinArc<Data, Node>);

// NB: the wrapper type is required, as the type alias version
//     type Node = ThinArc<Data, Node>;
// is rejected as an infinitely recursive type alias expansion.

impl Node {
    fn new<I>(head: Data, children: I) -> Self
    where
        I: IntoIterator<Item = Node>,
        I::IntoIter: ExactSizeIterator, // + TrustedLen
    {
        Node(ThinArc::new(head, children))
    }

    fn data(&self) -> usize {
        self.0.head
    }
}

#[test]
fn node() {
    let a = Node::new(1, vec![]);
    let b = Node::new(2, vec![]);
    let c = Node::new(3, vec![]);
    let children = vec![a.clone(), b.clone(), c.clone()];
    let boxed = Node::new(children.iter().map(|node| node.data()).sum(), children);
    dbg!(boxed);
}
