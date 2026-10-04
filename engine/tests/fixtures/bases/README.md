# Base-class fixtures

Sources whose definitions name base classes: one base (`Derived.java`), several in an
order that is not alphabetical (`multi.py`, `shapes.cpp`), and none (`Plain` in
`multi.py`). `abi_result_build_roundtrip` requires every count to occur and checks
the bases survive extraction and a rebuild from a cache, deep-copied (issue 40).
