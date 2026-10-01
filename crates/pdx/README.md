# pdx

The command-line interface.

Open. Apache-2.0; see `LICENSE` at the repository root.

## Public API

The command surface arrives in a later task. One command exists, and is hidden:
`pdx engine-worker` serves extraction over standard input and output, and is how
isolated extraction (`PDX_ENGINE_ISOLATE=1`, `pdx_engine::isolate`) starts this program
as its worker. It is not meant to be run by hand.

## Tests

```
cargo test -p pdx
```

`engine_isolate_recovers` runs extraction in this binary as a worker, makes the engine
abort on one file through its test switch, and checks that only that file fails, with
reason `engine_crash`, and every other file, before and after it, is extracted exactly
as in process. The others check that isolation changes nothing about what is
extracted, however files are batched, and that a program which is not a worker is
refused.
