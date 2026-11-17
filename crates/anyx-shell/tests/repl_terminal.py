"""Unix PTY integration checks. Run after `cargo build -p anyx-shell`.

Uses Python's standard library. All commands use synthetic session values.
Widget appearance is checked separately with Ratatui's TestBackend in Rust.
"""

import argparse
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import termios
import time


class Session:
    def __init__(self, binary):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.execv(str(binary), [str(binary)])
        self.resize(120, 30)

    def resize(self, width, height):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.kill(self.pid, signal.SIGWINCH)

    def step(self, keys=b"", startup=False):
        if keys:
            os.write(self.fd, keys)
        output = b""
        deadline = time.monotonic() + 5
        idle = None if startup else time.monotonic() + 0.25
        answered = 0
        while time.monotonic() < deadline:
            if not select.select([self.fd], [], [], 0.05)[0]:
                if idle is not None and time.monotonic() > idle:
                    break
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not data:
                break
            output += data
            idle = time.monotonic() + 0.15
            queries = output.count(b"\x1b[6n")
            while answered < queries:
                os.write(self.fd, b"\x1b[1;1R")
                answered += 1
        return output

    def exit(self):
        self.step(b"\x04")
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            child, status = os.waitpid(self.pid, os.WNOHANG)
            if child:
                self.pid = None
                assert os.waitstatus_to_exitcode(status) == 0, status
                assert termios.tcgetattr(self.fd)[3] & termios.ICANON, "terminal left in raw mode"
                return
            time.sleep(0.05)
        raise AssertionError("Ctrl-D did not exit")

    def close(self):
        if self.pid is not None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
        os.close(self.fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path(__file__).resolve().parents[3] / "target/debug/anyx-shell")
    session = Session(parser.parse_args().binary.resolve())
    result = '{ name "COPY", value "hello🙂" }'.encode()
    try:
        assert session.step(startup=True), "no initial TUI output"
        # Accept scope, then method. The second acceptance must only edit input.
        session.step(b"env\tset")
        assert result not in session.step(b"\r"), "completion executed a command"
        session.step(b"KEY\t" + "hello🙂".encode())
        # Shift-Tab revisits the first edited slot; Tab visits the second and exits.
        session.step(b"\x1b[ZCOPY\t\t")
        assert result in session.step(b"\r"), "snippet navigation lost the edited values"
        session.step(b"environment / re\rCOPY\t")
        assert '"hello🙂"'.encode() in session.step(b"\r"), "session state was lost"
        session.step(b"env")
        assert b"^C" in session.step(b"\x03"), "Ctrl-C failed to cancel"
        session.step(b'\x1b[200~environment / read variable (\n"COPY")\x1b[201~')
        assert '"hello🙂"'.encode() in session.step(b"\r"), "multiline paste failed"
        session.step(b"\x1b[A")
        assert '"hello🙂"'.encode() in session.step(b"\r"), "history recall failed"
        # Explicit completion, dismissal, and small-terminal resize.
        assert session.step(b"\x00"), "Ctrl-Space did not open the menu"
        session.step(b"\x1b")
        session.resize(20, 10)
        session.step(b"env")
        session.step(b"\x03")
        session.exit()
        print("PASS: popup acceptance, snippet replacement, Tab/Shift-Tab, Unicode, execution, state, cancellation, paste, history, Ctrl-Space, resize, Ctrl-D, terminal restoration")
    finally:
        session.close()


if __name__ == "__main__":
    main()
