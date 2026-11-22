#!/usr/bin/env python3
"""Exercise managed Ollama installation and reuse through the installed desktop.

This test downloads the actual approved runtime on its first launch. It preserves
that state for a second launch and checks that the active installation is reused.
Use --model to also pull a small real model and request a response via the daemon.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from urllib.request import Request, urlopen


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def request(base, route, body=None, timeout=10):
    data = None if body is None else json.dumps(body).encode()
    req = Request(base + route, data, {'Content-Type': 'application/json'})
    with urlopen(req, timeout=timeout) as response:
        return json.load(response)


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('evidence', type=Path)
    parser.add_argument('--model', default='qwen3:0.6b')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    state = evidence / 'state'
    state.mkdir()
    runtime = state / 'ollama'
    ollama_base = f'http://127.0.0.1:{port()}'
    env = {**os.environ,
           'APP_SETTINGS_PATH': str(state / 'settings.json'),
           'APP_AGENT_WORKSPACE': str(state / 'workspace'),
           'APP_MODELS_DIR': str(state / 'models'),
           'APP_REMOTE_DOWNLOADS_DIR': str(state / 'downloads'),
           'APP_OLLAMA_RUNTIME_ROOT': str(runtime),
           'OLLAMA_MODELS': str(runtime / 'models'),
           'APP_OLLAMA_BASE_URL': ollama_base,
           'APP_LOCAL_PROVIDER': 'ollama',
           'APP_AGENT_PROVIDER': 'ollama',
           'APP_AGENT_MODEL': args.model,
           'XDG_DATA_HOME': str(state / 'data'),
           'XDG_CONFIG_HOME': str(state / 'config')}
    # These are the shipping defaults. The explicit assertion prevents the test
    # from silently inheriting the disabled-install setup of other QA scripts.
    env.update(APP_OLLAMA_AUTO_START='true', APP_OLLAMA_AUTO_INSTALL='true')
    results = {'status': 'fail', 'binary': str(binary), 'launches': []}
    manifest_before = None
    try:
        for attempt in (1, 2):
            daemon_base = f'http://127.0.0.1:{port()}'
            env['APP_BIND_ADDR'] = daemon_base.removeprefix('http://')
            desktop_log = evidence / f'desktop-{attempt}.log'
            env['MLX_PILOT_DESKTOP_LOG'] = str(desktop_log)
            current = {'attempt': attempt, 'phases': []}
            results['launches'].append(current)
            with (evidence / f'console-{attempt}.log').open('w') as log:
                app = subprocess.Popen([str(binary)], env=env, cwd=state, stdout=log, stderr=log)
                started = time.monotonic()
                try:
                    deadline = started + (1500 if attempt == 1 else 120)
                    ready = False
                    while time.monotonic() < deadline:
                        assert app.poll() is None, f'Desktop exited with {app.returncode}'
                        try:
                            startup = request(daemon_base, '/runtime/startup')
                            (evidence / f'startup-{attempt}.json').write_text(json.dumps(startup, indent=2))
                            ollama = next(p for p in startup['providers'] if p['provider'] == 'ollama')
                            phase = ollama['phase']
                            if not current['phases'] or current['phases'][-1] != phase:
                                current['phases'].append(phase)
                                print(f'Launch {attempt}: {phase}', flush=True)
                            if phase in ('failed', 'cancelled'):
                                raise AssertionError(ollama.get('error') or ollama)
                            ready = ollama['ready']
                            if ready:
                                break
                        except (OSError, ValueError):
                            pass
                        time.sleep(.5)
                    assert ready, 'Managed Ollama did not become ready before the QA deadline'
                    current['ready_after_seconds'] = round(time.monotonic() - started, 2)
                    deadline = time.monotonic() + 30
                    while time.monotonic() < deadline:
                        if desktop_log.exists() and 'Conectado' in desktop_log.read_text(errors='replace'):
                            break
                        time.sleep(.5)
                    else:
                        raise AssertionError('Native WebView did not connect to the embedded daemon')
                    active = json.loads((runtime / 'active.json').read_text())
                    runtime_state = json.loads((runtime / 'runtime-state.json').read_text())
                    assert runtime_state['provider_ready'] and runtime_state['process_origin'] == 'managed'
                    executable = runtime / 'versions' / active['version'] / active['executable_relative_path']
                    with executable.open('rb') as stream:
                        assert hashlib.file_digest(stream, 'sha256').hexdigest() == active['executable_sha256']
                    current.update(installed_version=active['version'], managed_runtime=True, webview_connected=True)
                    if attempt == 1:
                        manifest_before = {key: value for key, value in active.items() if key != 'validated_at_epoch_ms'}
                        print(f'Pulling model {args.model}', flush=True)
                        pulled = request(ollama_base, '/api/pull', {'model': args.model, 'stream': False}, timeout=900)
                        assert pulled.get('status') == 'success', pulled
                    else:
                        assert {key: value for key, value in active.items() if key != 'validated_at_epoch_ms'} == manifest_before, 'Reopen replaced the validated installation'
                        assert 'downloading' not in current['phases'] and 'installing' not in current['phases'], current
                        current['reused_active_manifest'] = True
                    answer = request(daemon_base, '/chat', {
                        'model_id': f'ollama::{args.model}',
                        'messages': [{'role': 'user', 'content': 'Say hello in one short sentence. /no_think'}],
                        'options': {'temperature': 0, 'max_tokens': 64},
                    }, timeout=180)
                    assert answer.get('message', {}).get('content', '').strip(), answer
                    current['answer'] = answer['message']['content']
                    current['status'] = 'pass'
                finally:
                    stop(app)
        results['status'] = 'pass'
    except Exception as error:
        results['error'] = str(error)
        raise
    finally:
        (evidence / 'report.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
        print(json.dumps(results, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    main()
