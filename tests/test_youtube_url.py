"""Tests for YouTube URL validation and format selectors.

These cover the pure logic in main.py — no display or Win32 required,
so they run on any OS (imports in main.py are platform-guarded).
"""

from main import _YT_FORMAT_MERGED, _YT_FORMAT_SINGLE, is_valid_youtube_url


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
