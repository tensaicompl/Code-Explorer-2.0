# Derivation-fact fixtures

Sources whose definitions carry decorators, annotations and route bindings, and whose
calls carry arguments: a FastAPI module (`routes.py`), a Spring controller
(`UserController.java`) and an Express server (`server.js`). `abi_result_build_roundtrip`
requires each kind of fact to occur, every call positioned in the source to have a
node-type path, and all of them to survive a rebuild from a cache, deep-copied
(issues 46 and 47).
