"""Wallpaper rotation: cycle through a list of files on an interval.

Pure state machine (no Qt): the UI owns a QTimer and calls next_file().
Supports sequential and shuffle modes; missing files are skipped.
"""

from __future__ import annotations

import os
import random

INTERVALS = (60, 300, 900, 1800, 3600)


def format_interval(seconds: int) -> str:
    """Human label for an interval choice. Pure, unit-tested."""
    try:
        seconds = int(seconds)
    except Exception:
        return ""
    if seconds < 60:
        return f"{seconds} s"
    minutes = seconds // 60
    if minutes < 60:
        return f"{minutes} min"
    hours = minutes / 60
    return f"{hours:g} h"


class RotationQueue:
    """Ordered file list with a cursor. All logic, no I/O."""

    def __init__(self, files=None, index: int = 0, shuffle: bool = False,
                 seed=None, repeat: bool = True):
        self.files = list(files or [])
        self.index = max(0, int(index))
        self.shuffle = bool(shuffle)
        self.repeat = bool(repeat)
        self._rng = random.Random(seed)
        self._order = self._build_order()

    def _build_order(self) -> list:
        order = list(range(len(self.files)))
        if self.shuffle:
            self._rng.shuffle(order)
        return order

    def add(self, path: str) -> bool:
        """Append a file if it exists and is not listed. Returns added."""
        try:
            if not path or not os.path.exists(path):
                return False
            if path in self.files:
                return False
            self.files.append(path)
            self._order = self._build_order()
            return True
        except Exception:
            return False

    def remove(self, path: str) -> bool:
        try:
            if path not in self.files:
                return False
            pos = self.files.index(path)
            del self.files[pos]
            self._order = [i if i < pos else i - 1
                           for i in self._order if i != pos]
            if self.index >= len(self.files):
                self.index = 0
            return True
        except Exception:
            return False

    def set_shuffle(self, shuffle: bool) -> None:
        """Switch shuffle mode and rebuild the play order from the top."""
        try:
            self.shuffle = bool(shuffle)
            self._order = self._build_order()
            self.index = 0
        except Exception:
            pass

    def clear(self) -> None:
        self.files = []
        self._order = []
        self.index = 0

    def prune_missing(self) -> int:
        """Drop files that no longer exist. Returns removed count."""
        try:
            before = len(self.files)
            kept = [p for p in self.files if os.path.exists(p)]
            removed = before - len(kept)
            self.files = kept
            self._order = self._build_order()
            if self.index >= len(self.files):
                self.index = 0
            return removed
        except Exception:
            return 0

    def next_file(self) -> str | None:
        """Advance the cursor and return the next existing file (or None).

        Without repeat the queue stops at the last item (returns None
        instead of wrapping) so the final wallpaper stays on.
        """
        try:
            if not self.files or not self._order:
                return None
            if not self.repeat and self.index >= len(self._order) - 1:
                last = self.files[self._order[-1]]
                return last if os.path.exists(last) else None
            for _ in range(len(self._order)):
                self.index = (self.index + 1) % len(self._order)
                path = self.files[self._order[self.index]]
                if os.path.exists(path):
                    return path
            return None
        except Exception:
            return None

    def restart(self) -> str | None:
        """Start over from the top: next_file() returns the first item."""
        try:
            self.index = -1
        except Exception:
            pass
        return self.next_file()

    def to_config(self) -> dict:
        return {"files": list(self.files), "index": self.index,
                "shuffle": self.shuffle, "repeat": self.repeat}

    @classmethod
    def from_config(cls, data: dict | None) -> RotationQueue:
        try:
            data = data or {}
            files = data.get("files", [])
            if not isinstance(files, list):
                files = []
            return cls(files=[str(p) for p in files],
                       index=int(data.get("index", 0)),
                       shuffle=bool(data.get("shuffle", False)),
                       repeat=bool(data.get("repeat", True)))
        except Exception:
            return cls()
