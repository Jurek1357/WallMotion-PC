"""Tests for wallmotion.platform.linux - Qt-free, headless-safe."""

import json
import os
import sys

import pytest

from wallmotion.platform import linux as L


class TestDetectSession:
    def test_x11(self):
        info = L.detect_session({"XDG_SESSION_TYPE": "x11",
                                 "XDG_CURRENT_DESKTOP": "XFCE"})
        assert info["session"] == "x11"
        assert info["desktops"] == ["xfce"]
        assert not info["is_gnome"]

    def test_wayland_kde(self):
        info = L.detect_session({"XDG_SESSION_TYPE": "wayland",
                                 "XDG_CURRENT_DESKTOP": "KDE"})
        assert info["session"] == "wayland"
        assert info["is_kde"]

    def test_gnome_colon_list(self):
        info = L.detect_session({"XDG_SESSION_TYPE": "wayland",
                                 "XDG_CURRENT_DESKTOP": "ubuntu:GNOME"})
        assert info["is_gnome"]

    def test_wayland_fallback_via_display(self):
        info = L.detect_session({"WAYLAND_DISPLAY": "wayland-0"})
        assert info["session"] == "wayland"

    def test_empty_env(self):
        info = L.detect_session({})
        assert info["session"] == "unknown"
        assert info["desktops"] == []

    def test_case_insensitive(self):
        info = L.detect_session({"XDG_SESSION_TYPE": "X11"})
        assert info["session"] == "x11"


class TestCandidateBackends:
    def test_x11(self):
        assert L.candidate_backends({"session": "x11"}) == ["x11"]

    def test_kde_wayland(self):
        info = {"session": "wayland", "is_kde": True,
                "is_wlroots": False, "is_gnome": False}
        assert L.candidate_backends(info) == ["kde"]

    def test_wlroots(self):
        info = {"session": "wayland", "is_kde": False,
                "is_wlroots": True, "is_gnome": False}
        assert L.candidate_backends(info) == ["wlroots"]

    def test_gnome(self):
        info = {"session": "wayland", "is_kde": False,
                "is_wlroots": False, "is_gnome": True}
        assert L.candidate_backends(info) == ["gnome"]

    def test_tty_has_no_backend(self):
        assert L.candidate_backends({"session": "tty"}) == []


class TestCommands:
    def test_feh(self):
        assert L.feh_command("/a/b.png") == ["feh", "--bg-fill", "/a/b.png"]

    def test_swww(self):
        assert L.swww_command("/a/b.png") == [
            "swww", "img", "--resize", "crop", "/a/b.png"]

    def test_gsettings_sets_both_keys(self):
        cmds = L.gsettings_commands("/a/b.png")
        assert len(cmds) == 2
        assert cmds[0][:3] == ["gsettings", "set",
                               "org.gnome.desktop.background"]
        assert cmds[0][3] == "picture-uri"
        assert cmds[1][3] == "picture-uri-dark"
        assert cmds[0][4].startswith("file://")
        assert cmds[0][4] == cmds[1][4]

    def test_plasma(self):
        assert L.plasma_command("/a/b.png") == [
            "plasma-apply-wallpaperimage", "/a/b.png"]

    def test_mpv_options_muted(self):
        # mute=yes (not no-audio): the audio stream must stay alive so a
        # later `set_property mute no` over IPC can unmute.
        opts = L.mpv_options(True, 0.8)
        assert "mute=yes" in opts
        assert "no-audio" not in opts
        assert "loop-file=inf" in opts

    def test_mpv_options_volume(self):
        assert L.mpv_options(False, 0.6) == "loop-file=inf volume=60 mute=no"

    def test_mpv_options_clamped(self):
        assert "volume=100" in L.mpv_options(False, 5.0)
        assert "volume=0" in L.mpv_options(False, -1.0)

    def test_mpvpaper_all_outputs(self):
        cmd = L.mpvpaper_command("/v.mp4", True, 0.3)
        assert cmd[0] == "mpvpaper" and "*" in cmd and "/v.mp4" in cmd

    def test_mpvpaper_named_output_and_ipc(self):
        cmd = L.mpvpaper_command("/v.mp4", False, 0.5,
                                 output="DP-1", ipc_socket="/tmp/x.sock")
        assert "DP-1" in cmd
        assert any("input-ipc-server=/tmp/x.sock" in a for a in cmd)

    def test_xwinwrap_fullscreen_wid_placeholder(self):
        cmd = L.xwinwrap_command("/v.mp4", True, 0.3)
        # -b (below) keeps the window under the desktop icons; -ni -nf
        # make it input-transparent and unfocusable.
        assert cmd[:7] == ["xwinwrap", "-fs", "-b", "-ni", "-nf",
                           "-ov", "-fdt"]
        assert "--" in cmd
        assert "WID" in cmd
        assert cmd[cmd.index("--") + 1] == "mpv"

    def test_xwinwrap_geometry(self):
        assert L.xwinwrap_geometry(None) is None
        assert L.xwinwrap_geometry({}) is None
        mon = {"x": 1920, "y": 0, "w": 1920, "h": 1080}
        assert L.xwinwrap_geometry(mon) == "1920x1080+1920+0"
        cmd = L.xwinwrap_command("/v.mp4", False, 0.5,
                                 geometry="1920x1080+1920+0")
        assert "-g" in cmd
        assert "1920x1080+1920+0" in cmd
        assert "-fs" not in cmd


class TestIpc:
    def test_message_format(self):
        msg = L.mpv_ipc_message("volume", 60)
        assert json.loads(msg) == {"command": ["set_property", "volume", 60]}
        assert msg.endswith(b"\n")

    def test_set_to_nowhere_fails_quietly(self):
        assert L.mpv_ipc_set("/nonexistent/wallmotion-test.sock",
                             "volume", 10) is False


class TestBattery:
    def _make_psu(self, tmp_path, name, type_, status):
        d = tmp_path / name
        d.mkdir()
        (d / "type").write_text(type_, encoding="utf-8")
        (d / "status").write_text(status, encoding="utf-8")

    def test_discharging(self, tmp_path):
        self._make_psu(tmp_path, "BAT0", "Battery\n", "Discharging\n")
        assert L.read_battery_status(str(tmp_path)) is True

    def test_charging_is_not_battery(self, tmp_path):
        self._make_psu(tmp_path, "BAT0", "Battery\n", "Charging\n")
        assert L.read_battery_status(str(tmp_path)) is False

    def test_mains_ignored(self, tmp_path):
        self._make_psu(tmp_path, "AC", "Mains\n", "Charging\n")
        assert L.read_battery_status(str(tmp_path)) is False

    def test_missing_dir(self, tmp_path):
        assert L.read_battery_status(str(tmp_path / "nope")) is False


class TestDetectBackend:
    def _env(self, session, desktop):
        return {"XDG_SESSION_TYPE": session, "XDG_CURRENT_DESKTOP": desktop}

    def test_x11_backend_when_tools_present(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: f"/usr/bin/{name}")
        backend = L.detect_backend(self._env("x11", "XFCE"))
        assert isinstance(backend, L.X11Backend)

    def test_wlroots_backend(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: f"/usr/bin/{name}")
        backend = L.detect_backend(self._env("wayland", "Hyprland"))
        assert isinstance(backend, L.WlrootsBackend)

    def test_kde_backend(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: f"/usr/bin/{name}")
        backend = L.detect_backend(self._env("wayland", "KDE"))
        assert isinstance(backend, L.KdeBackend)

    def test_gnome_backend_video_refused(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(L, "hanabi_state", lambda: "missing")
        backend = L.detect_backend(self._env("wayland", "GNOME"))
        assert isinstance(backend, L.GnomeBackend)
        assert backend.set_video("/v.mp4", True, 0.3) is False

    def test_no_tools_no_backend(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: None)
        assert L.detect_backend(self._env("x11", "XFCE")) is None

    def test_tty_no_backend(self, monkeypatch):
        monkeypatch.setattr(L, "_which", lambda name: f"/usr/bin/{name}")
        assert L.detect_backend({"XDG_SESSION_TYPE": "tty"}) is None


class TestBundledTools:
    def test_bundled_dirs_xdg(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
        bindir = tmp_path / "wallmotion" / "bin"
        bindir.mkdir(parents=True)
        dirs = L.bundled_bin_dirs()
        assert str(bindir) in dirs

    def test_bundled_dirs_missing_ok(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path / "nope"))
        # AppImage dir may or may not exist under pytest (not frozen),
        # but nothing may raise and every entry must be a real dir.
        import os
        assert all(os.path.isdir(d) for d in L.bundled_bin_dirs())

    def test_tool_env_prepends_bundled(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
        bindir = tmp_path / "wallmotion" / "bin"
        bindir.mkdir(parents=True)
        monkeypatch.setenv("PATH", "/usr/bin")
        env = L.tool_env()
        assert env["PATH"].split(os.pathsep)[0] == str(bindir)

    @pytest.mark.skipif(sys.platform == "win32",
                        reason="extensionless `mpv` never resolves through "
                               "PATHEXT; bundled-tool lookup is POSIX-only")
    def test_which_finds_bundled_tool(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
        bindir = tmp_path / "wallmotion" / "bin"
        bindir.mkdir(parents=True)
        tool = bindir / "mpv"
        tool.write_text("#!/bin/sh\n", encoding="utf-8")
        tool.chmod(0o755)
        assert L._which("mpv") == str(tool)


class TestSessionTool:
    """Session tools must be the real system binaries: a user-prefix
    shadow (Linuxbrew gsettings -> keyfile backend) must lose to
    /usr/bin even when PATH lists the shadow first."""

    def _exe(self, dirpath, name):
        p = dirpath / name
        p.write_text("#!/bin/sh\n", encoding="utf-8")
        p.chmod(0o755)
        return p

    def test_system_dir_beats_path(self, tmp_path, monkeypatch):
        sysdir = tmp_path / "sysbin"
        sysdir.mkdir()
        pathdir = tmp_path / "brewbin"
        pathdir.mkdir()
        real = self._exe(sysdir, "gsettings")
        self._exe(pathdir, "gsettings")
        monkeypatch.setattr(L, "SYSTEM_BIN_DIRS", (str(sysdir),))
        monkeypatch.setenv("PATH", f"{pathdir}{os.pathsep}{sysdir}")
        assert L.session_tool("gsettings") == str(real)

    def test_falls_back_to_which(self, tmp_path, monkeypatch):
        empty = tmp_path / "empty"
        empty.mkdir()
        monkeypatch.setattr(L, "SYSTEM_BIN_DIRS", (str(empty),))
        monkeypatch.setattr(L, "_which", lambda name: f"/opt/bin/{name}")
        assert L.session_tool("gsettings") == "/opt/bin/gsettings"

    def test_missing_returns_none(self, tmp_path, monkeypatch):
        empty = tmp_path / "empty"
        empty.mkdir()
        monkeypatch.setattr(L, "SYSTEM_BIN_DIRS", (str(empty),))
        monkeypatch.setattr(L, "_which", lambda name: None)
        assert L.session_tool("gsettings") is None
        assert "gsettings" in L.GnomeBackend().missing_tools()

    def test_gnome_uses_resolved_exe(self, monkeypatch):
        calls = []
        monkeypatch.setattr(
            L, "session_tool", lambda name: f"/sys/bin/{name}")
        monkeypatch.setattr(L, "hanabi_state", lambda: "missing")
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(list(cmd))
            or (True, "''\n", ""))
        assert L.GnomeBackend().set_image("/new.png") is True
        assert all(c[0] == "/sys/bin/gsettings" for c in calls)


class TestIpcSocket:
    def test_clear_unlinks_stale(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
        stale = tmp_path / L.MPV_IPC_SOCKET_NAME
        stale.write_text("", encoding="utf-8")
        L.clear_ipc_socket()
        assert not stale.exists()

    def test_clear_missing_ok(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
        L.clear_ipc_socket()  # must not raise

    def test_spawn_clears_socket(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
        stale = tmp_path / L.MPV_IPC_SOCKET_NAME
        stale.write_text("", encoding="utf-8")

        class FakeProc:
            pid = 1

            def poll(self):
                return 0

            def wait(self, timeout=None):
                return 0

        spawned = []
        monkeypatch.setattr(
            L.subprocess, "Popen",
            lambda cmd, **kw: spawned.append(cmd) or FakeProc())
        assert L.X11Backend()._spawn(["true"]) is True
        assert spawned == [["true"]]
        assert not stale.exists()


class TestHanabi:
    UUID = L.HANABI_UUID

    def _fake_gext(self, installed, enabled, monkeypatch):
        installed_list = f"foo@bar\n{self.UUID}\nbaz@qux\n" if installed \
            else "foo@bar\nbaz@qux\n"
        enabled_list = f"{self.UUID}\n" if enabled else ""

        def fake_run(cmd, timeout=15):
            if "--enabled" in cmd:
                return True, enabled_list, ""
            return True, installed_list, ""

        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(L, "run_command", fake_run)
        monkeypatch.setattr(L, "_hanabi_dir",
                            lambda: "/nonexistent-hanabi-dir")

    def test_state_missing(self, monkeypatch):
        self._fake_gext(False, False, monkeypatch)
        assert L.hanabi_state() == "missing"

    def test_state_installed(self, monkeypatch):
        self._fake_gext(True, False, monkeypatch)
        assert L.hanabi_state() == "installed"

    def test_state_enabled(self, monkeypatch):
        self._fake_gext(True, True, monkeypatch)
        assert L.hanabi_state() == "enabled"

    def test_state_unknown_without_tool(self, monkeypatch):
        monkeypatch.setattr(L, "session_tool", lambda name: None)
        assert L.hanabi_state() == "unknown"

    def test_state_dir_fallback(self, monkeypatch, tmp_path):
        # freshly installed: not in `list` yet, but files are on disk
        self._fake_gext(False, False, monkeypatch)
        monkeypatch.setattr(L, "_hanabi_dir", lambda: str(tmp_path))
        assert L.hanabi_state() == "queued"

    def test_state_exact_match_only(self, monkeypatch):
        # a uuid sharing our prefix must not count as installed
        def fake_run(cmd, timeout=15):
            return True, f"{self.UUID}-extra\n", ""
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(L, "run_command", fake_run)
        monkeypatch.setattr(L, "_hanabi_dir",
                            lambda: "/nonexistent-hanabi-dir")
        assert L.hanabi_state() == "missing"

    def test_install_from_bundled_zip(self, monkeypatch, tmp_path):
        calls = []
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        import wallmotion.utils
        monkeypatch.setattr(wallmotion.utils, "_asset_path",
                            lambda name: str(tmp_path / name))
        (tmp_path / L.HANABI_ZIP).write_bytes(b"PK")
        ok, _ = L.hanabi_install()
        assert ok
        assert calls[0][:2] == ["/usr/bin/gnome-extensions", "install"]
        assert "--force" in calls[0]

    def test_install_fallback_rejects_zip_traversal(self, monkeypatch,
                                                    tmp_path):
        import io
        import zipfile
        monkeypatch.setattr(L, "session_tool", lambda name: None)
        monkeypatch.setattr(L, "_hanabi_dir", lambda: str(tmp_path / "ext"))
        import wallmotion.utils
        monkeypatch.setattr(wallmotion.utils, "_asset_path",
                            lambda name: str(tmp_path / name))
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("../evil.txt", "x")
        (tmp_path / L.HANABI_ZIP).write_bytes(buf.getvalue())
        ok, err = L.hanabi_install()
        assert not ok
        assert "evil.txt" in err
        assert not (tmp_path / "evil.txt").exists()

    def test_install_missing_zip(self, monkeypatch, tmp_path):
        monkeypatch.setattr(L, "session_tool", lambda name: None)
        import wallmotion.utils
        monkeypatch.setattr(wallmotion.utils, "_asset_path",
                            lambda name: str(tmp_path / name))
        ok, _ = L.hanabi_install()
        assert not ok

    def test_enable_queues_in_enabled_extensions(self, monkeypatch):
        calls = []

        def fake_run(cmd, timeout=15):
            calls.append(cmd)
            if cmd[:3] == ["/usr/bin/gnome-extensions", "enable",
                           self.UUID]:
                return False, "", "does not exist"
            if cmd[1] == "get":
                return True, "['foo@bar']\n", ""
            return True, "", ""

        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(L, "run_command", fake_run)
        assert L.hanabi_enable() is True
        set_cmd = [c for c in calls
                   if c[:2] == ["/usr/bin/gsettings", "set"]]
        assert set_cmd
        assert self.UUID in set_cmd[0][-1]

    def test_enable_live(self, monkeypatch):
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(
            L, "run_command", lambda cmd, timeout=15: (True, "", ""))
        assert L.hanabi_enable() is True

    def test_video_commands(self, monkeypatch):
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        cmds = L.hanabi_video_commands("/v.mp4", True, 0.5)
        assert cmds[0][-4:] == ["set", L.HANABI_SCHEMA, "mute", "true"]
        assert cmds[1][-3:] == [L.HANABI_SCHEMA, "volume", "50"]
        assert cmds[-1][-2:] == ["video-path", "/v.mp4"]
        assert all("GSETTINGS_SCHEMA_DIR=" in c[1] for c in cmds)

    def test_gnome_video_refused_when_not_enabled(self, monkeypatch):
        monkeypatch.setattr(L, "hanabi_state", lambda: "missing")
        assert L.GnomeBackend().set_video("/v.mp4", True, 0.3) is False

    def test_gnome_video_writes_schema(self, monkeypatch):
        calls = []
        monkeypatch.setattr(L, "hanabi_state", lambda: "enabled")
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        assert L.GnomeBackend().set_video("/v.mp4", True, 0.5) is True
        schemas = [c[4] for c in calls]
        assert all(s == L.HANABI_SCHEMA for s in schemas)
        assert calls[-1][-2:] == ["video-path", "/v.mp4"]

    def test_gnome_video_writes_then_enables(self, monkeypatch):
        # installed-but-disabled: keys written BEFORE the enable call,
        # so Hanabi's renderer launches straight into playback.
        calls = []
        monkeypatch.setattr(L, "hanabi_state", lambda: "installed")
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        assert L.GnomeBackend().set_video("/v.mp4", True, 0.5) is True
        enable_idx = next(i for i, c in enumerate(calls)
                          if c[:2] == ["/usr/bin/gnome-extensions",
                                       "enable"])
        path_idx = next(i for i, c in enumerate(calls)
                        if c[-2:] == ["video-path", "/v.mp4"])
        assert path_idx < enable_idx

    def test_gnome_stop_disables_extension(self, monkeypatch):
        calls = []
        monkeypatch.setattr(L, "hanabi_state", lambda: "enabled")
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        L.GnomeBackend().stop()
        assert ["set", L.HANABI_SCHEMA, "video-path", ""] \
            in [c[-4:] for c in calls]
        assert ["/usr/bin/gnome-extensions", "disable",
                L.HANABI_UUID] in calls

    def test_gnome_pause_restores_path(self, monkeypatch):
        calls = []
        states = iter(["enabled", "installed"])  # before/after disable
        monkeypatch.setattr(L, "hanabi_state", lambda: next(states))
        monkeypatch.setattr(L, "session_tool",
                            lambda name: f"/usr/bin/{name}")

        def fake_run(cmd, timeout=15):
            calls.append(cmd)
            if "get" in cmd:
                return True, "'/v.mp4'\n", ""
            return True, "", ""

        monkeypatch.setattr(L, "run_command", fake_run)
        b = L.GnomeBackend()
        b.set_paused(True)
        tails = [c[-4:] for c in calls]
        assert ["set", L.HANABI_SCHEMA, "video-path", ""] in tails
        assert ["/usr/bin/gnome-extensions", "disable",
                L.HANABI_UUID] in calls
        b.set_paused(False)
        tails = [c[-4:] for c in calls]
        assert ["set", L.HANABI_SCHEMA, "video-path", "/v.mp4"] in tails
        assert ["/usr/bin/gnome-extensions", "enable",
                L.HANABI_UUID] in calls


class TestRestore:
    def test_gnome_snapshot_and_restore(self, monkeypatch):
        calls = []

        def fake_run(cmd, timeout=15):
            calls.append(list(cmd))
            if cmd[1] == "get":
                return True, "'file:///usr/share/old.png'\n", ""
            return True, "", ""

        monkeypatch.setattr(L, "run_command", fake_run)
        monkeypatch.setattr(L, "_which", lambda name: "/usr/bin/" + name)
        monkeypatch.setattr(L, "hanabi_state", lambda: "missing")
        backend = L.GnomeBackend()
        assert backend.set_image("/new.png") is True
        backend.restore()
        sets = [c for c in calls if c[1] == "set"]
        # 2 set_image calls + 2 restore calls
        assert len(sets) == 4
        assert sets[2][4] == "'file:///usr/share/old.png'"
        assert sets[3][3] == "picture-uri-dark"

    def test_gnome_restore_without_apply_noops(self, monkeypatch):
        calls = []
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        L.GnomeBackend().restore()
        assert calls == []

    @pytest.mark.skipif(sys.platform == "win32",
                        reason="POSIX-only: expanduser('~') ignores HOME on "
                               "Windows and restore replays via /bin/sh")
    def test_x11_fehbg_snapshot(self, tmp_path, monkeypatch):
        fehbg = tmp_path / ".fehbg"
        fehbg.write_text("#!/bin/sh\nfeh --bg-fill '/old.png'\n",
                         encoding="utf-8")
        monkeypatch.setenv("HOME", str(tmp_path))
        monkeypatch.setattr(L, "_which", lambda name: "/usr/bin/" + name)
        calls = []
        monkeypatch.setattr(
            L, "run_command",
            lambda cmd, timeout=15: calls.append(cmd) or (True, "", ""))
        backend = L.X11Backend()
        assert backend.set_image("/new.png") is True
        backend.restore()
        # restore replays the saved script through sh
        assert calls[-1][0] == "/bin/sh"
        assert "old.png" in fehbg.read_text(encoding="utf-8")

    def test_wlroots_snapshot_parses_query(self, monkeypatch):
        def fake_run(cmd, timeout=15):
            if cmd[:2] == ["swww", "query"]:
                return True, ("eDP-1: 1920x1080, scale: 1, "
                              "currently displaying: image: /old.png\n"), ""
            return True, "", ""

        monkeypatch.setattr(L, "run_command", fake_run)
        monkeypatch.setattr(L, "_which", lambda name: "/usr/bin/" + name)
        backend = L.WlrootsBackend()
        assert backend.set_image("/new.png") is True
        assert backend._swww_backup == "/old.png"


class TestAutostart:
    def test_entry_content(self):
        text = L.autostart_entry("/usr/bin/wallmotion")
        assert "Exec=/usr/bin/wallmotion" in text
        assert text.startswith("[Desktop Entry]")

    def test_write_autostart(self, tmp_path, monkeypatch):
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        assert L.write_autostart("/usr/bin/wallmotion") is True
        content = (tmp_path / "autostart" / "wallmotion.desktop").read_text(
            encoding="utf-8")
        assert "Exec=/usr/bin/wallmotion" in content


class TestMisc:
    def test_file_uri(self):
        uri = L.file_uri("/a/b.png")
        assert uri.startswith("file://")
        assert uri.endswith("b.png")
        assert "a" in uri

    def test_describe_session(self, monkeypatch):
        monkeypatch.setenv("XDG_SESSION_TYPE", "x11")
        monkeypatch.setenv("XDG_CURRENT_DESKTOP", "XFCE")
        assert "x11" in L.describe_session()
        del os.environ["XDG_SESSION_TYPE"]
        del os.environ["XDG_CURRENT_DESKTOP"]
