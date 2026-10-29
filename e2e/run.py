#!/usr/bin/env python3
"""Run the jcode TUI e2e suites against any binary, in parallel, deterministically.

  run.py --binary $(which jcode) --suite baseline
  run.py --binary ~/Code/Personal/jcode/target/selfdev/jcode --suite all

Each test owns a fully isolated sandbox (see harness.py), so tests run in
parallel safely. Exit 0 only if every selected test passes. Artifacts (the last
screen of every failure) go to --artifacts.
"""
import argparse
import concurrent.futures as cf
import importlib
import os
import subprocess
import sys
import time
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

SUITES = {"baseline": "suite_baseline", "at_file": "suite_at_file"}


def version_of(binary):
    out = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout
    # "jcode v0.99.1 (7206efd0c)"
    for tok in out.split():
        if tok.startswith("v") and tok[1:2].isdigit():
            return tok[1:]
    return None


def run_one(binary, version, modname, name, timeout):
    mod = importlib.import_module(modname)
    fn = getattr(mod, name)
    start = time.monotonic()
    try:
        fn(binary, version)
        return modname, name, True, time.monotonic() - start, ""
    except BaseException:
        return modname, name, False, time.monotonic() - start, traceback.format_exc()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--suite", default="baseline", choices=[*SUITES, "all"])
    ap.add_argument("-k", default="", help="substring filter on test name")
    ap.add_argument("-j", type=int, default=6)
    ap.add_argument("--repeat", type=int, default=1, help="run N times, fail on any flake")
    ap.add_argument("--artifacts", default=os.path.expanduser("~/tmp/jcode-e2e-artifacts"))
    a = ap.parse_args()

    binary = os.path.realpath(os.path.expanduser(a.binary))
    version = version_of(binary)
    print(f"binary: {binary}\nversion: {version}")
    names = list(SUITES.values()) if a.suite == "all" else [SUITES[a.suite]]
    jobs = []
    for modname in names:
        mod = importlib.import_module(modname)
        for n in sorted(dir(mod)):
            if n.startswith("t_") and a.k in n:
                jobs += [(modname, n)] * a.repeat

    os.makedirs(a.artifacts, exist_ok=True)
    t0 = time.monotonic()
    failed = []
    with cf.ThreadPoolExecutor(max_workers=a.j) as ex:
        futs = [ex.submit(run_one, binary, version, m, n, 60) for m, n in jobs]
        for f in cf.as_completed(futs):
            m, n, ok, dt, tb = f.result()
            print(f"{'PASS' if ok else 'FAIL'}  {m}.{n}  {dt:4.1f}s")
            if not ok:
                failed.append((m, n))
                with open(os.path.join(a.artifacts, f"{m}.{n}.txt"), "a") as fh:
                    fh.write(tb + "\n")
                print("   " + tb.strip().splitlines()[-1][:200])
    print(f"\n{len(jobs) - len(failed)}/{len(jobs)} passed in {time.monotonic() - t0:.1f}s")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
