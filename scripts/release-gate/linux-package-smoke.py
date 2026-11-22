#!/usr/bin/env python3
"""Ubuntu VM: linux-package-smoke.py path/to/package.deb new-evidence-directory.

Requires sudo, Xvfb, xauth, dbus-x11, openbox, ffmpeg, xdotool and python3-pyatspi.
Optionally pass --ollama-model TAG and set APP_OLLAMA_BASE_URL for live inference.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
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


def exercise(binary, evidence, state, report, model):
    import pyatspi
    import gi
    gi.require_version("Gdk", "3.0")
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gdk, GLib, Gtk
    # WebKit can report object bounds relative to its window even when AT-SPI requests screen coordinates.
    def find(test):
        return pyatspi.findDescendant(pyatspi.Registry.getDesktop(0), test)

    def visible(obj):
        return obj.getState().contains(pyatspi.STATE_SHOWING)

    def focus(control):
        subprocess.run(["xdotool", "windowactivate", "--sync", window], check=True, timeout=10)
        def focused():
            try:
                if not control.getState().contains(pyatspi.STATE_FOCUSED):
                    control.queryComponent().grabFocus()
                return control.getState().contains(pyatspi.STATE_FOCUSED)
            except GLib.GError:
                return None
        until("keyboard focus on the accessible control", focused)

    def activate(control):
        focus(control)
        subprocess.run(["xdotool", "key", "--clearmodifiers", "space"], check=True)

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
    def get_json(url):
        with urlopen(url, timeout=4) as response:
            return json.load(response)

    if model:
        ollama_url = os.environ.get("APP_OLLAMA_BASE_URL", "http://127.0.0.1:11434").rstrip("/")
        env.update(APP_OLLAMA_BASE_URL=ollama_url, APP_LOCAL_PROVIDER="ollama",
                   APP_AGENT_PROVIDER="ollama", APP_AGENT_MODEL=model)
        (state / "settings.json").write_text(json.dumps({"agent": {
            "temperature": 0, "execution_mode": "locked", "max_tools_in_prompt": 0,
        }}))
        assert model in {item["name"] for item in get_json(ollama_url + "/api/tags")["models"]}
        report["ollama_model"] = model

    def health():
        try:
            info = get_json(f"{base}/health")
            return info if not model or info.get("provider_ready") else None
        except OSError:
            return None

    def session_ids():
        return {session["id"] for session in get_json(f"{base}/agent/sessions")}

    def check_policy(stage):
        config = get_json(f"{base}/agent/config")
        values = {key: config[key] for key in ("provider", "model_id", "execution_mode", "temperature")}
        report[stage] = values
        assert values == {"provider": "ollama", "model_id": model, "execution_mode": "locked", "temperature": 0}, \
            f"Agent settings changed at {stage}: {values}"

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
            def app_windows():
                result = subprocess.run(["xdotool", "search", "--all", "--onlyvisible", "--pid",
                    str(app.pid), "--name", "MLX Pilot"], capture_output=True, text=True, check=False)
                return result.stdout.splitlines()
            window = until("installed desktop window", app_windows)[0]
            report["window_title"] = output("xdotool", "getwindowname", window)
            if model:
                check_policy("startup_policy")
                (evidence / "runtime-startup.json").write_text(json.dumps(get_json(f"{base}/runtime/startup"), indent=2) + "\n")
            tab = until("Agent tab", lambda: find(lambda o: o.getRole() in
                (pyatspi.ROLE_PAGE_TAB, pyatspi.ROLE_PUSH_BUTTON) and visible(o)
                and (o.name or "").strip().casefold() in {"agent", "agente"}))
            time.sleep(1)
            activate(tab)
            before = session_ids()
            new_session = until("New conversation button", lambda: find(lambda obj:
                obj.getRole() == pyatspi.ROLE_PUSH_BUTTON and visible(obj)
                and "id:btn-new-session" in obj.getAttributes()))
            activate(new_session)
            created = until("session created from the installed UI", lambda: session_ids() - before)
            session_id, = created
            report["session_created_via_ui"] = session_id
            if model:
                check_policy("ui_policy")
            def is_composer(obj):
                if obj.getRole() != pyatspi.ROLE_ENTRY or not visible(obj):
                    return False
                attributes = " ".join(obj.getAttributes())
                return "id:agent-command-input" in attributes or "agente" in ((obj.name or "") + attributes).casefold()
            composer = until("Agent composer", lambda: find(is_composer))
            focus(composer)
            draft = "Say hello. /no_think" if model else "Teste Linux."
            subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "80", draft], check=True)
            clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
            def copied_draft():
                subprocess.run(["xdotool", "key", "--clearmodifiers", "--delay", "120", "ctrl+a", "ctrl+c"], check=True)
                return clipboard.wait_for_text() == draft
            until("draft copied from the composer", copied_draft)
            subprocess.run(["xdotool", "key", "End"], check=True)
            report["draft"] = draft
            if model:
                subprocess.run(["xdotool", "key", "Return"], check=True)
                def persisted_answer():
                    messages = get_json(f"{base}/agent/sessions/{session_id}")
                    user_sent = any(m["role"] == "user" and m["content"] == draft for m in messages)
                    return user_sent and next((m["content"] for m in messages if m["role"] == "assistant"
                        and m["content"].strip()), None)
                report["persisted_answer"] = until("model answer in the UI-created session", persisted_answer, 120)
                assert re.search(r"\b(hello|hi|olá|oi)\b", report["persisted_answer"], re.IGNORECASE), "Expected a greeting"
                assert model in {item["name"] for item in get_json(ollama_url + "/api/ps")["models"]}
                def displays_answer(obj):
                    if obj.getRole() not in (pyatspi.ROLE_PARAGRAPH, pyatspi.ROLE_TEXT, pyatspi.ROLE_STATIC) or not visible(obj):
                        return False
                    return report["persisted_answer"].strip() in (obj.name or obj.queryText().getText(0, -1))
                until("persisted answer in the WebKit window", lambda: find(displays_answer))
                report["answer_visible"] = True
                check_policy("post_conversation_policy")
                messages = get_json(f"{base}/agent/sessions/{session_id}")
                (evidence / "session.json").write_text(json.dumps(messages, ensure_ascii=False, indent=2) + "\n")
                report["session_tool_events"] = sum(bool(m.get("tool_name") or m.get("tool_call_id")) or
                    m["role"] == "tool" or m["kind"].startswith("tool_") for m in messages)
                entries = get_json(f"{base}/agent/audit?session_id={session_id}&limit=500")["entries"]
                report["audit_tool_events"] = sum(e["event_type"].startswith("tool_") for e in entries)
                assert report["session_tool_events"] == 0 and report["audit_tool_events"] == 0, "Unexpected tool event"
            time.sleep(2)
            assert recorder.poll() is None and app.poll() is None
        finally:
            stop(recorder)
            stop(app)
            stop(wm)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--ollama-model")
    parser.add_argument("--session", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    evidence = args.evidence.resolve()
    if not args.session:
        evidence.mkdir(parents=True, exist_ok=False)
        subprocess.run(["sudo", "-n", "dpkg", "-i", package], check=True)
        subprocess.run(["xvfb-run", "-a", "-s", "-screen 0 1440x1000x24", "dbus-run-session",
                        "--", sys.executable, __file__, *sys.argv[1:], "--session"], check=True)
        return
    name = output("dpkg-deb", "-f", package, "Package")
    binary, = [p for p in output("dpkg-query", "-L", name).splitlines()
               if p.startswith("/usr/bin/") and os.access(p, os.X_OK) and os.path.isfile(p)]
    report = {"package": package.name, "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
              "architecture": output("dpkg", "--print-architecture"), "installed_version": output(
                  "dpkg-query", "-W", "-f=${Version}", name), "status": "fail"}
    try:
        with tempfile.TemporaryDirectory(prefix="mlx-linux-package-") as sandbox:
            exercise(binary, evidence, Path(sandbox), report, args.ollama_model)
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
