"""Tests for wallmotion.rotation - pure queue logic, no Qt needed."""

from wallmotion.rotation import (
    INTERVALS,
    RotationQueue,
    format_interval,
)


class TestFormatInterval:
    def test_minutes(self):
        assert format_interval(60) == "1 min"
        assert format_interval(300) == "5 min"

    def test_hours(self):
        assert format_interval(3600) == "1 h"

    def test_seconds(self):
        assert format_interval(30) == "30 s"

    def test_garbage(self):
        assert format_interval("x") == ""


class TestQueue:
    def test_add_and_next_sequential(self, tmp_path):
        a = tmp_path / "a.mp4"
        b = tmp_path / "b.mp4"
        a.write_bytes(b"x")
        b.write_bytes(b"x")
        q = RotationQueue()
        assert q.add(str(a)) is True
        assert q.add(str(a)) is False  # duplicate
        assert q.add(str(b)) is True
        assert q.add(str(tmp_path / "missing.mp4")) is False
        assert q.next_file() == str(b)
        assert q.next_file() == str(a)
        assert q.next_file() == str(b)

    def test_shuffle_visits_all(self, tmp_path):
        paths = []
        for i in range(5):
            p = tmp_path / f"v{i}.mp4"
            p.write_bytes(b"x")
            paths.append(str(p))
        q = RotationQueue(paths, shuffle=True, seed=42)
        seen = {q.next_file() for _ in range(5)}
        assert seen == set(paths)

    def test_skips_missing(self, tmp_path):
        a = tmp_path / "a.mp4"
        a.write_bytes(b"x")
        missing = str(tmp_path / "gone.mp4")
        q = RotationQueue([missing, str(a)])
        assert q.next_file() == str(a)

    def test_empty(self):
        assert RotationQueue().next_file() is None
        assert RotationQueue().restart() is None

    def test_restart_starts_from_top(self, tmp_path):
        paths = []
        for name in ("a.mp4", "b.mp4", "c.mp4"):
            p = tmp_path / name
            p.write_bytes(b"x")
            paths.append(str(p))
        q = RotationQueue(paths)
        assert q.next_file() == paths[1]
        assert q.restart() == paths[0]
        assert q.next_file() == paths[1]

    def test_remove(self, tmp_path):
        a = tmp_path / "a.mp4"
        b = tmp_path / "b.mp4"
        a.write_bytes(b"x")
        b.write_bytes(b"x")
        q = RotationQueue([str(a), str(b)])
        assert q.remove(str(a)) is True
        assert q.remove(str(a)) is False
        assert q.next_file() == str(b)

    def test_clear(self, tmp_path):
        a = tmp_path / "a.mp4"
        a.write_bytes(b"x")
        q = RotationQueue([str(a)])
        q.clear()
        assert q.files == [] and q.next_file() is None

    def test_prune_missing(self, tmp_path):
        a = tmp_path / "a.mp4"
        a.write_bytes(b"x")
        q = RotationQueue([str(a), str(tmp_path / "gone.mp4")])
        assert q.prune_missing() == 1
        assert q.files == [str(a)]

    def test_roundtrip_config(self, tmp_path):
        a = tmp_path / "a.mp4"
        a.write_bytes(b"x")
        q = RotationQueue([str(a)], index=0, shuffle=True)
        q2 = RotationQueue.from_config(q.to_config())
        assert q2.files == [str(a)] and q2.shuffle is True

    def test_from_config_garbage(self):
        assert RotationQueue.from_config(None).files == []
        assert RotationQueue.from_config({"files": "junk"}).files == []

    def test_intervals_sane(self):
        assert len(INTERVALS) >= 3
        assert all(isinstance(v, int) and v > 0 for v in INTERVALS)
