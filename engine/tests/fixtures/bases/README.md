# Base-class and trait-implementation fixtures

Sources whose definitions name base classes: one base (`Derived.java`), several in an
order that is not alphabetical (`multi.py`, `shapes.cpp`), and none (`Plain` in
`multi.py`). `abi_result_build_roundtrip` requires every count to occur and checks
the bases survive extraction and a rebuild from a cache, deep-copied (issue 40).

Sources with `impl Trait for Type` relations: one, from an empty block
(`one_impl.rs`), and several, among them another empty block, a trait named with its
module (`fmt::Display`) and one with type arguments (`From<f64>`) (`traits.rs`). The
same test requires files with none, one and several, and checks the relations survive
extraction and a rebuild, deep-copied (issue 42).
