//! Test that we don't leak Head in ThinBox::clone().

#![allow(unused, clippy::redundant_clone)]

use std::panic::UnwindSafe;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct DontLeakMe(Arc<()>);

#[derive(Debug)]
struct PanicsOnClone;
impl Clone for PanicsOnClone {
    fn clone(&self) -> Self {
        panic!("PanicsOnClone panicking on clone");
    }
}

fn test_box<B: Clone + UnwindSafe + 'static, F: FnOnce(DontLeakMe, PanicsOnClone) -> B>(
    make_box: F,
) {
    let mut leak_detector = DontLeakMe(Arc::new(()));
    let boxed = make_box(leak_detector.clone(), PanicsOnClone);

    std::panic::catch_unwind(move || {
        let _unreachable = boxed.clone();
        // The above clone should panic.
    })
    .expect_err("PanicsOnClone didn't panic");

    // Now there should only be our copy of leak_detector still around!
    assert!(Arc::get_mut(&mut leak_detector.0).is_some());
}

#[test]
fn test_std_box() {
    // This tests that the test is correct, since it's rather complex and involves intentional panics!
    test_box(|leaker, panicker| Box::new((leaker, vec![panicker])));
}

#[test]
fn test_thinbox() {
    use thin_dst::ThinBox;
    test_box(|leaker, panicker| ThinBox::new(leaker, std::iter::once(panicker)));
}
