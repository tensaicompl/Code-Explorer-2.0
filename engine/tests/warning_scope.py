#!/usr/bin/env python3
"""The warning exemptions reach vendored sources and never the interface layer.

    warning_scope.py <build dir>

For each warning class exempt in vendored code, a small probe that triggers it is
compiled twice, with the exact flags the build gives an interface source and with
those it gives a vendored one, read from the build's compile commands. With the
interface's flags the probe must fail, naming the class; with the vendored flags it
must compile, and still warn, so the class stays visible there. A new interface
source is built like the existing ones, so this is what adding such code to
engine/api would meet.
"""
import json
import pathlib
import shlex
import subprocess
import sys
import tempfile

# One probe per exempt class: the smallest C that triggers it and nothing else.
PROBES = {
    "unused-function": "static int never_called(void) { return 1; }\nint probe(void) { return 0; }\n",
    "unused-parameter": "int probe(int unused) { return 0; }\n",
    "unused-variable": "int probe(void) { int unused; return 0; }\n",
    "unused-but-set-variable": "int probe(void) { int set = 0; set = 1; return 0; }\n",
    "unused-value": "int probe(int x) { x + 1; return x; }\n",
    "comment": "/* a /* nested opener */\nint probe(void) { return 0; }\n",
    "sign-compare": "int probe(int a, unsigned b) { return a < b; }\n",
}


def command_for(entries, suffix):
    for e in entries:
        if e["file"].endswith(suffix):
            return e
    sys.exit(f"error: no compile command for {suffix}; build with the tests enabled first")


def compile_probe(entry, probe_path, out_path):
    args = shlex.split(entry["command"]) if "command" in entry else list(entry["arguments"])
    source = entry["file"]
    rebuilt, skip = [], False
    for i, a in enumerate(args):
        if skip:
            skip = False
            continue
        if a == "-o":
            rebuilt += ["-o", str(out_path)]
            skip = True
        elif a == source or pathlib.Path(entry["directory"], a).resolve() == pathlib.Path(source):
            rebuilt.append(str(probe_path))
        else:
            rebuilt.append(a)
    done = subprocess.run(rebuilt, cwd=entry["directory"], capture_output=True, text=True)
    return done.returncode, done.stderr


def main():
    build = pathlib.Path(sys.argv[1])
    entries = json.loads((build / "compile_commands.json").read_text())
    interface = command_for(entries, "/engine/api/pdxe.c")
    vendored = command_for(entries, "/engine/src/helpers.c")
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for warning, text in PROBES.items():
            probe = pathlib.Path(tmp, f"probe_{warning.replace('-', '_')}.c")
            probe.write_text(text)
            out = pathlib.Path(tmp, "probe.o")
            rc_api, err_api = compile_probe(interface, probe, out)
            rc_vendored, err_vendored = compile_probe(vendored, probe, out)
            # gcc names the class -Werror=<class> when it is an error, clang -Werror,-W<class>.
            named = f"-W{warning}"
            as_error = (f"-Werror={warning}" in err_api or f"-Werror,{named}" in err_api)
            problems = []
            if rc_api == 0 or not as_error:
                problems.append(f"interface flags did not reject it as an error (exit {rc_api}):"
                                f"\n{err_api}")
            if rc_vendored != 0:
                problems.append(f"vendored flags rejected it (exit {rc_vendored}):\n{err_vendored}")
            elif named not in err_vendored:
                problems.append("vendored flags did not even warn, so the probe proves nothing")
            if problems:
                failures += 1
                print(f"FAIL {warning}: " + "; ".join(problems))
            else:
                print(f"ok   {warning}: an error in the interface layer, a warning in vendored code")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
