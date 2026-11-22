#!/usr/bin/env python3
"""Ubuntu VM: linux-package-smoke.py path/to/package.deb new-evidence-directory.

Requires sudo, Xvfb, xauth, dbus-x11, openbox, ffmpeg, xdotool and python3-pyatspi.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import urlopen


def output(*command):
    return subprocess.check_output(command, text=True).strip()


def until(label, inspect, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = inspect()
        if result:
            return result
        time.sleep(0.25)
    raise RuntimeError(f"Timed out waiting for {label}")


def stop(proc):
    if proc and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


def exercise(binary, evidence, state, report):
    import pyatspi
    import gi
    gi.require_version("Gdk", "3.0")
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gdk, Gtk
    # WebKit's accessibility tree and the pointer operate on this isolated X display.
    def find(test):
        return pyatspi.findDescendant(pyatspi.Registry.getDesktop(0), test)

    def visible(obj):
        return obj.getState().contains(pyatspi.STATE_SHOWING)

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    env = {**os.environ, "GDK_BACKEND": "x11", "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
           "APP_BIND_ADDR": f"127.0.0.1:{port}", "APP_SETTINGS_PATH": str(state / "settings.json"),
           "APP_AGENT_WORKSPACE": str(state / "workspace"), "APP_MODELS_DIR": str(state / "models"),
           "APP_OLLAMA_RUNTIME_ROOT": str(state / "ollama"), "APP_REMOTE_DOWNLOADS_DIR": str(state / "downloads"),
           "APP_OLLAMA_AUTO_START": "false", "APP_OLLAMA_AUTO_INSTALL": "false",
           "APP_LLAMACPP_AUTO_START": "false", "APP_LLAMACPP_AUTO_INSTALL": "false",
           "XDG_DATA_HOME": str(state / "data"), "XDG_CONFIG_HOME": str(state / "config"),
           "MLX_PILOT_DESKTOP_LOG": str(evidence / "desktop.log")}
    def health():
        try:
            with urlopen(f"{base}/health", timeout=2) as response:
                return json.load(response)
        except OSError:
            return None

    def session_ids():
        with urlopen(f"{base}/agent/sessions", timeout=4) as response:
            return {session["id"] for session in json.load(response)}

    wm = recorder = app = None
    with (evidence / "native.log").open("w") as log:
        try:
            wm = subprocess.Popen(["openbox"], stdout=log, stderr=log)
            recorder = subprocess.Popen(["ffmpeg", "-nostdin", "-loglevel", "warning", "-f", "x11grab",
                "-framerate", "15", "-video_size", "1440x1000", "-i", os.environ["DISPLAY"],
                "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p",
                "-movflags", "+faststart", str(evidence / "linux-agent.mp4")], stdout=log, stderr=log)
            app = subprocess.Popen([binary], env=env, cwd=state, stdout=log, stderr=log)
            report["pid"] = app.pid
            report["health"] = until("embedded daemon", health)
            report["listener_owned_by_app"] = f"pid={app.pid}," in output("ss", "-ltnp", f"sport = :{port}")
            assert report["listener_owned_by_app"], "The listener does not belong to the installed app"
            report["executable"] = str(Path(f"/proc/{app.pid}/exe").resolve())
            assert Path(report["executable"]) == Path(binary).resolve()
            tab = until("Agent tab", lambda: find(lambda o: o.getRole() in
                (pyatspi.ROLE_PAGE_TAB, pyatspi.ROLE_PUSH_BUTTON) and visible(o)
                and (o.name or "").strip().casefold() in {"agent", "agente"}))
            time.sleep(1)
            assert tab.queryAction().doAction(0), "Agent tab did not open"
            before = session_ids()
            new_session = until("New conversation button", lambda: find(lambda obj:
                obj.getRole() == pyatspi.ROLE_PUSH_BUTTON and (obj.name or "").casefold()
                in {"nova sessão", "nova sessao", "nova conversa"} and visible(obj)))
            assert new_session.queryAction().doAction(0), "New conversation button did not activate"
            created = until("session created from the installed UI", lambda: session_ids() - before)
            session_id, = created
            report["session_created_via_ui"] = session_id
            def is_composer(obj):
                if obj.getRole() != pyatspi.ROLE_ENTRY or not visible(obj):
                    return False
                attributes = " ".join(obj.getAttributes())
                return "id:agent-command-input" in attributes or "agente" in ((obj.name or "") + attributes).casefold()
            composer = until("Agent composer", lambda: find(is_composer))
            composer.queryComponent().grabFocus()
            draft = "Teste da janela instalada no Ubuntu."
            subprocess.run(["xdotool", "type", "--delay", "45", draft], check=True)
            subprocess.run(["xdotool", "key", "--clearmodifiers", "ctrl+a", "ctrl+c"], check=True)
            clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
            until("draft copied from the composer", lambda: clipboard.wait_for_text() == draft)
            subprocess.run(["xdotool", "key", "End"], check=True)
            report["draft"] = draft
            time.sleep(2)
            assert recorder.poll() is None and app.poll() is None
        finally:
            stop(recorder)
            stop(app)
            stop(wm)


def main():
    package = Path(sys.argv[1]).resolve(strict=True)
    evidence = Path(sys.argv[2]).resolve()
    if len(sys.argv) == 3:
        evidence.mkdir(parents=True, exist_ok=False)
        subprocess.run(["sudo", "-n", "dpkg", "-i", package], check=True)
        subprocess.run(["xvfb-run", "-a", "-s", "-screen 0 1440x1000x24", "dbus-run-session",
                        "--", sys.executable, __file__, package, evidence, "--session"], check=True)
        return
    name = output("dpkg-deb", "-f", package, "Package")
    binary, = [p for p in output("dpkg-query", "-L", name).splitlines()
               if p.startswith("/usr/bin/") and os.access(p, os.X_OK) and os.path.isfile(p)]
    report = {"package": package.name, "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
              "architecture": output("dpkg", "--print-architecture"), "installed_version": output(
                  "dpkg-query", "-W", "-f=${Version}", name), "status": "fail"}
    try:
        with tempfile.TemporaryDirectory(prefix="mlx-linux-package-") as sandbox:
            exercise(binary, evidence, Path(sandbox), report)
        duration = float(output("ffprobe", "-v", "error", "-show_entries", "format=duration",
                                "-of", "default=nw=1:nk=1", evidence / "linux-agent.mp4"))
        assert duration > 3
        report.update(status="pass", video_seconds=duration)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        (evidence / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
        print(json.dumps(report, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
