"""Film a built Tauri .app on macOS 15+ and verify navigation via Accessibility.

Usage: python3 smoke.py 'MLX Pilot.app' new-evidence-directory [--check-answer]
Requires Xcode Command Line Tools, ffprobe, screen capture and Accessibility.
QA_OLLAMA_URL and QA_OLLAMA_MODEL select an existing local provider for the run.
QA_MLX_COMMAND optionally selects the local MLX-LM executable.
"""
import argparse
import atexit
import json
import os
import re
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import URLError
from urllib.request import urlopen


args = argparse.ArgumentParser(description=__doc__)
args.add_argument("bundle", type=Path)
args.add_argument("evidence", type=Path)
args.add_argument("--check-answer", action="store_true", help="Ask the configured Ollama model through the native agent and verify its answer")
options = args.parse_args()
bundle = options.bundle.resolve(strict=True)
evidence = options.evidence.resolve()
if bundle.suffix != ".app":
    raise SystemExit("Provide the built .app bundle")
build_dir = tempfile.TemporaryDirectory(prefix="mlx-native-tools-")
atexit.register(build_dir.cleanup)
tool = Path(build_dir.name) / "window"
subprocess.run(["xcrun", "swiftc", "-parse-as-library", str(Path(__file__).with_name("window.swift")), "-o", str(tool)], check=True)
evidence.mkdir(parents=True, exist_ok=False)


def invoke(*args, check=True):
    return subprocess.run(
        [str(tool), *map(str, args)], check=check, capture_output=True, text=True, timeout=12
    )


def save(name, value):
    (evidence / name).write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def get_json(url):
    with urlopen(url, timeout=2) as response:
        return json.load(response)


def wait_for(check, timeout, description):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, URLError, TimeoutError):
            pass
        time.sleep(0.35)
    raise RuntimeError(f"Timeout waiting for {description}")


def click(*selectors):
    for command, *args in selectors:
        result = invoke(command, bundle, *args, check=False)
        if result.returncode == 0:
            time.sleep(0.8)
            return result.stdout.strip()
    return None


def tree_with(control):
    result = invoke("tree", bundle, check=False)
    if result.returncode == 0 and f"[id={control}]" in result.stdout:
        return result.stdout
    return None


def session_ids(base_url):
    return {row["id"] for row in get_json(base_url + "/agent/sessions")}


if invoke("permissions").stdout.strip() != "ScreenCapture=true Accessibility=true":
    raise SystemExit("Screen recording and accessibility access are required.")
if invoke("pid", bundle).stdout.strip():
    raise SystemExit("An instance of this exact bundle is already running")
with socket.socket() as unused_socket:
    unused_socket.bind(("127.0.0.1", 0))
    port = unused_socket.getsockname()[1]

log = evidence / "capture.log"
stop = evidence / "stop"
movie = evidence / "macos-window.mp4"
with tempfile.TemporaryDirectory(prefix="mlx-native-test-") as isolated:
    runtime = Path(isolated)
    workspace = runtime / "workspace"
    workspace.mkdir()
    local_ollama = os.environ.get("QA_OLLAMA_URL", "http://127.0.0.1:11434")
    model = os.environ.get("QA_OLLAMA_MODEL", "qwen3:0.6b")
    with log.open("w") as stream:
        recorder = None
        launched = False
        try:
            mlx_command = os.environ.get("QA_MLX_COMMAND")
            launch_options = ["--env", f"APP_MLX_COMMAND={mlx_command}"] if mlx_command else []
            subprocess.run([
                "open", "-n", "-a", str(bundle),
                *launch_options,
                "--env", f"APP_BIND_ADDR=127.0.0.1:{port}",
                "--env", f"APP_SETTINGS_PATH={runtime}/settings.json",
                "--env", f"APP_AGENT_WORKSPACE={workspace}",
                "--env", f"APP_MODELS_DIR={runtime}/models",
                "--env", f"APP_OLLAMA_RUNTIME_ROOT={runtime}/ollama",
                "--env", f"APP_REMOTE_DOWNLOADS_DIR={runtime}/downloads",
                "--env", "APP_LOCAL_PROVIDER=ollama",
                "--env", "APP_AGENT_PROVIDER=ollama",
                "--env", f"APP_AGENT_MODEL={model}",
                "--env", f"APP_OLLAMA_BASE_URL={local_ollama}",
                "--env", "APP_OLLAMA_AUTO_INSTALL=false",
                "--env", "APP_OLLAMA_AUTO_START=false",
                "--env", "APP_LLAMACPP_AUTO_INSTALL=false",
                "--env", "APP_LLAMACPP_AUTO_START=false",
                "--env", f"MLX_PILOT_DESKTOP_LOG={runtime}/desktop.log",
            ], check=True)
            launched = True
            wait_for(lambda: invoke("pid", bundle).stdout.strip(), 30, "bundle launch")
            recorder = subprocess.Popen([
                str(tool), "record", str(bundle), str(movie), str(stop), "440"
            ], stdout=stream, stderr=stream)
            wait_for(lambda: "RECORDING" in log.read_text(), 120, "window recording")
            print("Native window recording started.", flush=True)
            pid = invoke("pid", bundle).stdout.strip()
            health_url = f"http://127.0.0.1:{port}"
            wait_for(lambda: get_json(health_url + "/health"), 120, "embedded health")
            listener = subprocess.check_output([
                "/usr/sbin/lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN", "-t"
            ], text=True).split()
            if pid not in listener:
                raise RuntimeError("The process serving /health differs from the native bundle PID")
            save("process.json", {"bundle": str(bundle), "pid": int(pid), "embedded_daemon_port": port})
            startup_url = health_url + "/runtime/startup"
            wait_for(lambda: get_json(startup_url).get("app_ready"), 240, "native runtime readiness")
            start = get_json(startup_url)
            save("startup.json", {field: start.get(field) for field in ("phase", "step", "app_ready", "degraded", "providers")})
            save("health.json", get_json(health_url + "/health"))
            initial_tree = wait_for(lambda: tree_with("model-trigger"), 30, "rendered application header")
            (evidence / "initial-accessibility.txt").write_text(initial_tree)
            active = wait_for(lambda: click(("press", "AXButton", "Agente"), ("press", "AXRadioButton", "Agent")), 25, "agent navigation")
            agent_tree = wait_for(lambda: tree_with("btn-new-session"), 10, "agent session toolbar")
            (evidence / "agent-accessibility.txt").write_text(agent_tree)
            before = session_ids(health_url)
            created = wait_for(lambda: click(("press-id", "btn-new-session")), 10, "new conversation control")
            wait_for(lambda: session_ids(health_url) - before, 12, "new session in the embedded daemon")
            after = session_ids(health_url)
            if len(after - before) != 1 or not before.issubset(after):
                raise RuntimeError("New conversation must add exactly one session and preserve existing sessions")
            save("sessions.json", {"before": sorted(before), "after": sorted(after), "created": sorted(after - before)})
            config = wait_for(lambda: click(("press-id", "agent-tab-config"), ("press-in", "Agent view", "Config")), 12, "agent configuration")
            config_tree = wait_for(lambda: tree_with("agent-provider-select"), 10, "agent runtime selector")
            (evidence / "configuration-accessibility.txt").write_text(config_tree)
            back = wait_for(lambda: click(("press-id", "agent-tab-workspace"), ("press-in", "Agent view", "Workspace")), 12, "return to conversation")
            conversation_tree = wait_for(lambda: tree_with("agent-command-input"), 10, "agent message composer")
            (evidence / "conversation-accessibility.txt").write_text(conversation_tree)
            if options.check_answer:
                session_id, = after - before
                question = "Quanto é 2 + 2? Responda somente com o número, sem usar ferramentas."
                invoke("type-id", bundle, "agent-command-input", question)
                sent = click(("press-id", "agent-send-btn"))
                if not sent:
                    raise RuntimeError("Could not activate the native Send button")

                def answered():
                    messages = get_json(f"{health_url}/agent/sessions/{session_id}")
                    user_sent = any(row["role"] == "user" and row["content"] == question for row in messages)
                    for row in reversed(messages):
                        text = re.sub(r"<think>.*?</think>", "", row["content"], flags=re.DOTALL).strip()
                        if user_sent and row["role"] == "assistant" and re.fullmatch(r"4[.!]?", text):
                            return text
                    return None

                answer = wait_for(answered, 180, "Ollama answer persisted in the UI-created session")
                def rendered_answer():
                    result = invoke("tree", bundle, check=False).stdout
                    return result if question in result and any(
                        line.startswith("AXStaticText ") and line.removeprefix("AXStaticText ").strip() == answer
                        for line in result.splitlines()
                    ) else None
                transcript = wait_for(rendered_answer, 30, "Ollama answer visible in the native agent conversation")
                (evidence / "answer-accessibility.txt").write_text(transcript)
                save("conversation.json", {"session_id": session_id, "model": model, "sent_via": sent,
                                           "question": question, "persisted_answer": answer, "rendered": True})
            save("interactions.json", {"navigation": [active, created, config, back]})
            time.sleep(1)
        except Exception:
            if launched:
                failure = invoke("tree", bundle, check=False)
                (evidence / "failure-accessibility.txt").write_text(failure.stdout or failure.stderr)
            raise
        finally:
            stop.touch()
            if recorder and "WINDOW=" not in log.read_text():
                recorder.terminate()
            try:
                status = recorder.wait(timeout=20) if recorder else 1
            except subprocess.TimeoutExpired:
                recorder.terminate()
                status = recorder.wait(timeout=5)
            if launched:
                invoke("quit", bundle, check=False)
            if (runtime / "desktop.log").is_file():
                shutil.copyfile(runtime / "desktop.log", evidence / "desktop.log")
            if movie.exists():
                probe = subprocess.check_output([
                    "ffprobe", "-v", "error", "-show_entries",
                    "stream=codec_name,width,height,nb_frames:format=duration,size",
                    "-of", "json", str(movie)
                ], text=True)
                save("video.json", json.loads(probe))
            if status != 0:
                print("Recorder status:", status, file=sys.stderr)
            stop.unlink(missing_ok=True)

if status != 0 or not movie.exists():
    raise SystemExit("Native recording failed")
video = json.loads((evidence / "video.json").read_text())
if float(video["format"]["duration"]) < 2 or int(video["streams"][0]["nb_frames"]) < 4:
    raise SystemExit("Recording too short to demonstrate UI interactions")
print(evidence)
