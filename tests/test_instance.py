"""Tests for wallmotion.instance locking - QtCore/Network only."""

import pytest

try:
    from wallmotion.instance import acquire_single_instance_lock
    _HAVE_QT = True
except Exception:
    _HAVE_QT = False

pytestmark = pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")


class TestSingleInstanceLock:
    def test_second_acquire_fails(self):
        first = acquire_single_instance_lock()
        assert first is not None
        try:
            assert acquire_single_instance_lock() is None
        finally:
            first.unlock()
            del first

    def test_reacquire_after_release(self):
        first = acquire_single_instance_lock()
        assert first is not None
        first.unlock()
        del first
        second = acquire_single_instance_lock()
        assert second is not None
        second.unlock()
