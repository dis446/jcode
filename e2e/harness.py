"""Deterministic TUI harness for jcode, driven through tmux.

Works against ANY jcode binary (stock upstream or our fork build): it only uses
the public CLI and the rendered screen. No cargo, no source tree.

Determinism rules:
  * Fully isolated: temp JCODE_HOME, JCODE_RUNTIME_DIR, HOME, XDG_RUNTIME_DIR,
    private tmux server socket, scrubbed environment, telemetry and update off.
  * No network and no provider needed: the composer is exercised before any
    request is sent.
  * No fixed sleeps. Every step polls the screen until a predicate holds or a
    hard timeout fails the test with the last screen attached.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import time
import uuid

DEFAULT_TIMEOUT = 15.0


class Timeout(AssertionError):
    pass


class Session:
    def __init__(self, binary: str, files: list[str] | None = None,
                 cols: int = 120, rows: int = 32, args: list[str] | None = None):
        self.binary = binary
        self.root = tempfile.mkdtemp(prefix="jcode-e2e-")
        self.work = os.path.join(self.root, "work")
        self.home = os.path.join(self.root, "home")
        self.run = os.path.join(self.root, "run")
        for d in (self.work, self.home, self.run):
            os.makedirs(d)
        os.chmod(self.run, 0o700)
        for f in files or []:
            path = os.path.join(self.work, f)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            if f.endswith("/"):
                continue
            open(path, "w").close()
        self.tmux_sock = os.path.join(self.root, "tmux.sock")
        self.name = "e2e" + uuid.uuid4().hex[:8]
        env = {
            "HOME": self.home,
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TERM": "xterm-256color",
            "LANG": "C.UTF-8",
            "JCODE_HOME": os.path.join(self.home, ".jcode"),
            "JCODE_RUNTIME_DIR": self.run,
            "XDG_RUNTIME_DIR": self.run,
            "JCODE_NO_TELEMETRY": "1",
        }
        envargs = " ".join(f"{k}={shlex_quote(v)}" for k, v in env.items())
        cmd = f"env -i {envargs} {shlex_quote(binary)} --no-update --no-selfdev " + " ".join(args or [])
        self._tmux("new-session", "-d", "-s", self.name, "-x", str(cols), "-y", str(rows),
                   "-c", self.work, cmd)

    # -- plumbing ---------------------------------------------------------
    def _tmux(self, *a: str) -> str:
        r = subprocess.run(["tmux", "-S", self.tmux_sock, *a], capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"tmux {a}: {r.stderr.strip()}")
        return r.stdout

    def screen(self) -> str:
        try:
            out = self._tmux("capture-pane", "-p", "-t", self.name)
        except RuntimeError:
            return ""
        return "\n".join(line.rstrip() for line in out.splitlines())

    def alive(self) -> bool:
        r = subprocess.run(["tmux", "-S", self.tmux_sock, "has-session", "-t", self.name],
                           capture_output=True)
        return r.returncode == 0

    # -- input ------------------------------------------------------------
    def type(self, text: str) -> None:
        self._tmux("send-keys", "-t", self.name, "-l", text)

    def key(self, *keys: str) -> None:
        self._tmux("send-keys", "-t", self.name, *keys)

    def paste(self, text: str) -> None:
        buf = "b" + uuid.uuid4().hex[:6]
        self._tmux("set-buffer", "-b", buf, text)
        self._tmux("paste-buffer", "-p", "-b", buf, "-t", self.name)

    # -- waiting ----------------------------------------------------------
    def wait_for(self, pred, what: str, timeout: float = DEFAULT_TIMEOUT) -> str:
        deadline = time.monotonic() + timeout
        last = ""
        while time.monotonic() < deadline:
            last = self.screen()
            if pred(last):
                return last
            time.sleep(0.05)
        raise Timeout(f"timed out waiting for: {what}\n--- last screen ---\n{last}\n---")

    def wait_text(self, needle: str, timeout: float = DEFAULT_TIMEOUT) -> str:
        return self.wait_for(lambda s: needle in s, f"text {needle!r}", timeout)

    def wait_gone(self, needle: str, timeout: float = DEFAULT_TIMEOUT) -> str:
        return self.wait_for(lambda s: needle not in s, f"{needle!r} to disappear", timeout)

    def settle(self, quiet: float = 0.4, timeout: float = DEFAULT_TIMEOUT) -> str:
        """Return once the screen stops changing for `quiet` seconds."""
        deadline = time.monotonic() + timeout
        prev, since = None, time.monotonic()
        while time.monotonic() < deadline:
            cur = self.screen()
            if cur != prev:
                prev, since = cur, time.monotonic()
            elif time.monotonic() - since >= quiet:
                return cur
            time.sleep(0.05)
        raise Timeout("screen never settled\n" + (prev or ""))

    def wait_ready(self) -> str:
        """First stable frame with a composer on screen."""
        self.wait_for(lambda s: s.strip() != "", "any output", 30)
        return self.settle(quiet=0.6, timeout=30)

    def _kill_leftovers(self) -> None:
        """Kill any process (e.g. the auto-spawned jcode server) that inherited our runtime dir."""
        marker = f"JCODE_RUNTIME_DIR={self.run}".encode()
        for pid in os.listdir("/proc"):
            if not pid.isdigit() or int(pid) == os.getpid():
                continue
            try:
                with open(f"/proc/{pid}/environ", "rb") as fh:
                    if marker in fh.read().split(b"\0"):
                        os.kill(int(pid), 9)
            except (OSError, ProcessLookupError):
                pass

    def close(self) -> None:
        subprocess.run(["tmux", "-S", self.tmux_sock, "kill-server"], capture_output=True)
        self._kill_leftovers()
        shutil.rmtree(self.root, ignore_errors=True)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def shlex_quote(s: str) -> str:
    import shlex
    return shlex.quote(s)
