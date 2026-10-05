"""Single instance: forward CLI commands to the running app.

First instance listens on a QLocalServer; later invocations send their
command dict as one JSON line and exit. Stale socket files (crash
leftovers) are cleaned by removing and re-listening.
"""

from __future__ import annotations

import json
import os
import tempfile

from PySide6.QtCore import QLockFile, QObject, Signal
from PySide6.QtNetwork import QLocalServer, QLocalSocket

SERVER_NAME = "wallmotion-single-instance"
LOCK_NAME = "wallmotion-single-instance.lock"


def lock_path() -> str:
    try:
        return os.path.join(tempfile.gettempdir(), LOCK_NAME)
    except Exception:
        return LOCK_NAME


def acquire_single_instance_lock() -> QLockFile | None:
    """Take the app lock. None = another instance is running.

    QLockFile detects dead owners by PID, so a crash never wedges
    the lock. Keep the returned object alive for the app lifetime.
    Needs no QApplication.
    """
    try:
        lock = QLockFile(lock_path())
        if lock.tryLock(0):
            return lock
    except Exception:
        pass
    return None


class InstanceServer(QObject):
    """Listens for commands from later CLI invocations."""

    command_received = Signal(dict)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._server = None

    def start(self) -> bool:
        """Start listening. False when another instance owns the server."""
        try:
            probe = QLocalSocket(self)
            probe.connectToServer(SERVER_NAME)
            if probe.waitForConnected(500):
                try:
                    probe.disconnectFromServer()
                except Exception:
                    pass
                return False  # someone is already listening
        except Exception:
            pass
        try:
            QLocalServer.removeServer(SERVER_NAME)  # stale socket cleanup
            self._server = QLocalServer(self)
            self._server.newConnection.connect(self._on_connection)
            return bool(self._server.listen(SERVER_NAME))
        except Exception:
            return False

    def _on_connection(self):
        try:
            socket = self._server.nextPendingConnection()
            if socket is None:
                return
            socket.waitForReadyRead(2000)
            raw = bytes(socket.readAll().data()).decode("utf-8", "replace")
            socket.disconnectFromServer()
            for line in raw.splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    cmd = json.loads(line)
                except Exception:
                    continue
                if isinstance(cmd, dict):
                    self.command_received.emit(cmd)
        except Exception:
            pass

    def stop(self):
        try:
            if self._server is not None:
                self._server.close()
        except Exception:
            pass
        self._server = None


def send_command(cmd: dict, timeout_ms: int = 2000) -> bool:
    """Send a command to the running instance. True when delivered."""
    try:
        socket = QLocalSocket()
        socket.connectToServer(SERVER_NAME)
        if not socket.waitForConnected(timeout_ms):
            return False
        payload = (json.dumps(cmd) + "\n").encode("utf-8")
        socket.write(payload)
        socket.flush()
        socket.waitForBytesWritten(timeout_ms)
        try:
            socket.disconnectFromServer()
        except Exception:
            pass
        return True
    except Exception:
        return False
