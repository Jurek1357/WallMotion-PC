"""Tests for wallmotion.instance locking - QtCore/Network only."""

import pytest

try:
    from wallmotion.instance import acquire_single_instance_lock
    _HAVE_QT = True
except Exception:
    _HAVE_QT = False

pytestmark = pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")


class TestSingleInstanceLock:
    def test_second_acquire_fails(self, tmp_path):
        lockfile = str(tmp_path / "test.lock")
        first = acquire_single_instance_lock(lockfile)
        assert first is not None
        try:
            assert acquire_single_instance_lock(lockfile) is None
        finally:
            first.unlock()
            del first

    def test_reacquire_after_release(self, tmp_path):
        lockfile = str(tmp_path / "test.lock")
        first = acquire_single_instance_lock(lockfile)
        assert first is not None
        first.unlock()
        del first
        second = acquire_single_instance_lock(lockfile)
        assert second is not None
        second.unlock()
