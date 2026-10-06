"""Tests for YouTube URL validation and format selectors.

These cover the pure logic in wallmotion.youtube — no display or Win32
required, so they run on any OS.
"""

from wallmotion.youtube import (
    _YT_FORMAT_MERGED,
    _YT_FORMAT_SINGLE,
    is_playlist_url,
    is_valid_youtube_url,
    map_download_error,
    parse_playlist_entries,
    parse_progress_percent,
)


class TestValidUrls:
    def test_watch_url(self):
        assert is_valid_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ")

    def test_watch_with_extra_params(self):
        assert is_valid_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s&list=PLtest"
        )

    def test_short_link(self):
        assert is_valid_youtube_url("https://youtu.be/dQw4w9WgXcQ")

    def test_shorts(self):
        assert is_valid_youtube_url("https://www.youtube.com/shorts/dQw4w9WgXcQ")

    def test_embed(self):
        assert is_valid_youtube_url("https://www.youtube.com/embed/dQw4w9WgXcQ")

    def test_live(self):
        assert is_valid_youtube_url("https://www.youtube.com/live/dQw4w9WgXcQ")

    def test_nocookie_embed(self):
        assert is_valid_youtube_url(
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"
        )

    def test_playlist(self):
        assert is_valid_youtube_url(
            "https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf"
        )

    def test_http_scheme_allowed(self):
        assert is_valid_youtube_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ")

    def test_subdomain(self):
        assert is_valid_youtube_url("https://m.youtube.com/watch?v=dQw4w9WgXcQ")


class TestRejectedUrls:
    def test_empty(self):
        assert not is_valid_youtube_url("")
        assert not is_valid_youtube_url("   ")

    def test_not_string(self):
        assert not is_valid_youtube_url(None)
        assert not is_valid_youtube_url(12345)

    def test_file_scheme(self):
        assert not is_valid_youtube_url("file:///C:/Windows/system32/cmd.exe")

    def test_ftp_scheme(self):
        assert not is_valid_youtube_url("ftp://youtube.com/watch?v=x")

    def test_no_scheme(self):
        assert not is_valid_youtube_url("youtube.com/watch?v=dQw4w9WgXcQ")

    def test_localhost(self):
        assert not is_valid_youtube_url("http://localhost:8080/watch?v=x")
        assert not is_valid_youtube_url("https://localhost/watch?v=x")

    def test_raw_ip(self):
        assert not is_valid_youtube_url("http://127.0.0.1/watch?v=x")
        assert not is_valid_youtube_url("http://[::1]/watch?v=x")
        assert not is_valid_youtube_url("https://8.8.8.8/")

    def test_lookalike_domain(self):
        assert not is_valid_youtube_url("https://youtube.com.evil.com/watch?v=x")
        assert not is_valid_youtube_url("https://notyoutube.com/watch?v=x")
        assert not is_valid_youtube_url("https://youtu.be.evil.com/abc")

    def test_youtube_homepage_no_video(self):
        assert not is_valid_youtube_url("https://www.youtube.com/")
        assert not is_valid_youtube_url("https://www.youtube.com/feed/subscriptions")

    def test_nocookie_requires_embed(self):
        assert not is_valid_youtube_url(
            "https://www.youtube-nocookie.com/watch?v=dQw4w9WgXcQ"
        )

    def test_youtu_be_requires_id(self):
        assert not is_valid_youtube_url("https://youtu.be/")

    def test_whitespace_inside(self):
        assert not is_valid_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9Wg XcQ"
        )

    def test_too_long(self):
        url = "https://www.youtube.com/watch?v=" + "a" * 3000
        assert not is_valid_youtube_url(url)


class TestFormatSelectors:
    """Every fallback branch must keep the H.264 + <=1080p invariants —
    Qt Multimedia on Windows cannot decode AV1/VP9."""

    def test_merged_branches_are_h264_capped(self):
        for branch in _YT_FORMAT_MERGED.split("/"):
            assert "vcodec^=avc1" in branch
            assert "height<=1080" in branch

    def test_single_branches_are_h264_capped(self):
        for branch in _YT_FORMAT_SINGLE.split("/"):
            assert "vcodec^=avc1" in branch
            assert "height<=1080" in branch

    def test_branches_always_require_audio(self):
        for selector in (_YT_FORMAT_MERGED, _YT_FORMAT_SINGLE):
            for branch in selector.split("/"):
                assert "ba[" in branch or "acodec" in branch


class TestPlaylistUrls:
    def test_playlist_path(self):
        assert is_playlist_url(
            "https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf"
        )

    def test_watch_is_not_playlist(self):
        assert not is_playlist_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ")

    def test_watch_with_list_is_single_video(self):
        # /watch&list= downloads just that video (no surprise bulk download).
        assert not is_playlist_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLtest"
        )

    def test_invalid_url_is_not_playlist(self):
        assert not is_playlist_url("")
        assert not is_playlist_url("https://notyoutube.com/playlist?list=x")


class TestParsePlaylistEntries:
    def test_extracts_id_title_url(self):
        info = {"entries": [
            {"id": "abc123", "title": "First video"},
            {"id": "def456", "title": "Second video"},
        ]}
        entries = parse_playlist_entries(info)
        assert entries == [
            {"id": "abc123", "title": "First video",
             "url": "https://www.youtube.com/watch?v=abc123"},
            {"id": "def456", "title": "Second video",
             "url": "https://www.youtube.com/watch?v=def456"},
        ]

    def test_skips_entries_without_id(self):
        info = {"entries": [
            {"title": "no id here"},
            None,
            "junk",
            {"id": "ok1", "title": "OK"},
        ]}
        entries = parse_playlist_entries(info)
        assert [e["id"] for e in entries] == ["ok1"]

    def test_missing_title_falls_back_to_id(self):
        entries = parse_playlist_entries({"entries": [{"id": "xyz"}]})
        assert entries[0]["title"] == "xyz"

    def test_respects_limit(self):
        info = {"entries": [{"id": f"v{i}", "title": f"V{i}"} for i in range(10)]}
        assert len(parse_playlist_entries(info, limit=3)) == 3

    def test_empty_info(self):
        assert parse_playlist_entries({}) == []
        assert parse_playlist_entries({"entries": None}) == []


class TestParseProgressPercent:
    def test_plain(self):
        assert parse_progress_percent("42.5%") == 42.5
        assert parse_progress_percent("  7%  ") == 7.0
        assert parse_progress_percent("100%") == 100.0
        assert parse_progress_percent("0%") == 0.0

    def test_garbage(self):
        assert parse_progress_percent("") is None
        assert parse_progress_percent("N/A") is None
        assert parse_progress_percent(None) is None
        assert parse_progress_percent("150%") is None
        assert parse_progress_percent("-5%") is None


class TestMapDownloadError:
    def test_signin(self):
        assert map_download_error(
            "ERROR: Sign in to confirm you're not a bot") == "NEED_SIGNIN"

    def test_unavailable(self):
        assert map_download_error("ERROR: Private video") == "UNAVAILABLE"
        assert map_download_error("ERROR: Video unavailable") == "UNAVAILABLE"

    def test_timeout(self):
        assert map_download_error("ERROR: Read timed out") == "TIMEOUT"
        assert map_download_error("ERROR: Operation timed out") == "TIMEOUT"

    def test_passthrough(self):
        assert map_download_error("soubor se nenasel") == "soubor se nenasel"

    def test_ffmpeg_hint(self, monkeypatch):
        import wallmotion.youtube as yt
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)
        monkeypatch.setattr(yt, "_ffmpeg_works", lambda *a, **k: False)
        assert map_download_error(
            "ERROR: Requested format is not available") == "NEED_FFMPEG"
        assert map_download_error(
            "ERROR: You have requested merging of multiple formats "
            "but ffmpeg is not installed.") == "NEED_FFMPEG"
        monkeypatch.setattr(yt, "_ffmpeg_works", lambda *a, **k: True)
        assert map_download_error(
            "ERROR: Requested format is not available").startswith("ERROR:")


class TestFfmpegWorks:
    def test_no_exe(self, monkeypatch):
        import wallmotion.youtube as yt
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)
        monkeypatch.setattr(yt, "_ffmpeg_exe", lambda: None)
        assert yt._ffmpeg_works() is False

    def test_caches_result(self, monkeypatch):
        import wallmotion.youtube as yt
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)
        monkeypatch.setattr(yt, "_ffmpeg_exe", lambda: "/fake/ffmpeg")
        calls = []

        class FakeProc:
            returncode = 0

        def fake_run(*a, **k):
            calls.append(a)
            return FakeProc()

        monkeypatch.setattr("subprocess.run", fake_run)
        assert yt._ffmpeg_works() is True
        assert yt._ffmpeg_works() is True
        assert len(calls) == 1  # second call served from cache
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)

    def test_failing_exe(self, monkeypatch):
        import wallmotion.youtube as yt
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)
        monkeypatch.setattr(yt, "_ffmpeg_exe", lambda: "/fake/ffmpeg")

        def fake_run(*a, **k):
            raise OSError("blocked")
        monkeypatch.setattr("subprocess.run", fake_run)
        assert yt._ffmpeg_works() is False
        monkeypatch.setattr(yt, "_FFMPEG_OK", None)
