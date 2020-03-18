# thin-dst

Boxed custom DSTs that store a slice and the length of said slice inline.
Uses the standard library collection types for full interoperability,
and also provides thin owned pointers for space-conscious use.

## Alternative

[slice-dst] is a successor to this crate, which, along with the other
[pointer-utils] crates, offers a more composable API.

This crate will continue to be reactively maintained,
but any future development will focus on pointer-utils/slice-dst instead.

  [slice-dst]: <https://lib.rs/crates/slice-dst>
  [pointer-utils]: <https://github.com/CAD97/pointer-utils>

## Examples

The simplest example is just a boxed slice:

```rust
let boxed_slice = ThinBox::new((), vec![0, 1, 2, 3, 4, 5]);
assert_eq!(&*boxed_slice, &[0, 1, 2, 3, 4, 5][..]);
let boxed_slice: Box<ThinData<(), u32>> = boxed_slice.into();
```

All of the thin collection types are constructed with a "head" and a "tail".
The head is any `Sized` type that you would like to associate with the slice.
The "tail" is the owned slice of data that you would like to store.

This creates a collection of `ThinData`, which acts like `{ head, tail }`,
and also handles the `unsafe` required for both custom slice DSTs and thin DST pointers.
The most idiomatic usage is to encapsulate the use of thin-dst with a transparent newtype:

```rust
#[repr(transparent)]
struct NodeData(ThinData<NodeHead, Node>);
struct Node(ThinArc<NodeHead, Node>);
```

And then use `NodeData` by transmuting and/or [ref-cast]ing as needed.

  [ref-cast]: <https://lib.rs/crates/ref-cast>

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE/APACHE](LICENSE/APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE/MIT](LICENSE/MIT) or http://opensource.org/licenses/MIT)

at your option.

If you are a highly paid worker at any company that prioritises profit over
people, you can still use this crate. I simply wish you will unionise and push
back against the obsession for growth, control, and power that is rampant in
your workplace. Please take a stand against the horrible working conditions
they inflict on your lesser paid colleagues, and more generally their
disrespect for the very human rights they claim to fight for.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
