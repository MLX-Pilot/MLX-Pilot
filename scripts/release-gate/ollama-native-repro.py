#!/usr/bin/env python3
"""Compare the pinned native Ollama CPU backend and its AMX workaround.

Downloads Ollama 0.30.7 and verifies the GitHub asset digest, runs the baseline
requests, then restarts with the single Sapphire Rapids library quarantined.
All runtime and model data stays in the requested isolated output directory.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import socket
import subprocess
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

VERSION = '0.30.7'
MODEL = 'qwen3:0.6b'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    assert platform.system() == 'Linux'
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}[platform.machine()]
    asset = f'ollama-linux-{arch}.tar.zst'
    url = f'https://github.com/ollama/ollama/releases/download/v{VERSION}/{asset}'
    archive = root / asset
    report = {'version': VERSION, 'model': MODEL, 'arch': arch,
              'archive_url': url, 'status': 'fail', 'requests': []}

    def command(name, argv):
        result = subprocess.run(argv, capture_output=True, text=True, timeout=20)
        (root / name).write_text(result.stdout + result.stderr)

    command('cpu.txt', ['lscpu'])
    command('memory.txt', ['free', '-b'])
    command('kernel.txt', ['uname', '-a'])
    for path in ('/proc/meminfo', '/sys/fs/cgroup/memory.max', '/sys/fs/cgroup/memory.events'):
        if Path(path).is_file():
            (root / (path.replace('/', '_').strip('_') + '.txt')).write_text(Path(path).read_text())
    subprocess.run(['curl', '--fail', '--location', '--retry', '3', '--continue-at', '-',
                    '--output', str(archive), url], check=True)
    with archive.open('rb') as stream:
        report['archive_sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    headers = {'User-Agent': 'MLX-Pilot-runtime-diagnostics'}
    if os.environ.get('GH_TOKEN'):
        headers['Authorization'] = 'Bearer ' + os.environ['GH_TOKEN']
    metadata_url = f'https://api.github.com/repos/ollama/ollama/releases/tags/v{VERSION}'
    with urlopen(Request(metadata_url, headers=headers), timeout=30) as response:
        release = json.load(response)
    official = next(item for item in release['assets'] if item['name'] == asset)
    report['upstream_digest'] = official.get('digest')
    assert official.get('digest') == 'sha256:' + report['archive_sha256'], 'Upstream archive digest differs'
    extracted = root / 'runtime'
    extracted.mkdir()
    subprocess.run(['tar', '--zstd', '-xf', str(archive), '-C', str(extracted)], check=True)
    binary = extracted / 'bin/ollama'
    assert binary.is_file()
    command('binary.txt', ['file', str(binary)])
    command('dependencies.txt', ['ldd', str(binary)])
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    env = dict(os.environ)
    for key in ('GPU_DEVICE_ORDINAL', 'HIP_VISIBLE_DEVICES', 'ROCR_VISIBLE_DEVICES',
                'GGML_VK_VISIBLE_DEVICES', 'OLLAMA_LLM_LIBRARY', 'OLLAMA_VULKAN', 'CUDA_VISIBLE_DEVICES'):
        env.pop(key, None)
    env.update(OLLAMA_HOST=f'127.0.0.1:{port}', OLLAMA_MODELS=str(root / 'models'), OLLAMA_DEBUG='INFO')

    def request(name, route, body=None, timeout=180):
        sent = None if body is None else json.dumps(body).encode()
        req = Request(base + route, data=sent, headers={'Content-Type': 'application/json'})
        started = time.monotonic()
        try:
            with urlopen(req, timeout=timeout) as response:
                status, raw = response.status, response.read()
        except HTTPError as error:
            status, raw = error.code, error.read()
        except (URLError, OSError) as error:
            status, raw = 0, json.dumps({'error': str(error)}).encode()
        (root / f'{name}.json').write_bytes(raw)
        result = json.loads(raw)
        entry = {'name': name, 'route': route, 'body': body, 'status': status,
                 'seconds': round(time.monotonic() - started, 3), 'error': result.get('error')}
        report['requests'].append(entry)
        print(json.dumps(entry), flush=True)
        return status, result

    def start(log):
        server = subprocess.Popen([str(binary), 'serve'], env=env, cwd=binary.parent, stdout=log, stderr=log)
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            try:
                with urlopen(base + '/api/version', timeout=1) as response:
                    assert json.load(response)['version'] == VERSION
                return server
            except (URLError, OSError):
                assert server.poll() is None, 'Upstream daemon exited during startup'
                time.sleep(.25)
        stop(server)
        raise RuntimeError('Upstream daemon did not become ready')

    def stop(server):
        server.terminate()
        try:
            server.wait(timeout=15)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()

    chat_body = {'model': MODEL, 'messages': [{'role': 'user', 'content': 'Say hello in one short sentence.'}],
                 'stream': False, 'think': False, 'options': {'temperature': 0, 'num_predict': 128}}
    try:
        with (root / 'ollama.log').open('w') as log:
            server = start(log)
            try:
                status, result = request('pull', '/api/pull', {'model': MODEL, 'stream': False}, timeout=900)
                assert status == 200 and result.get('status') == 'success', result
                request('show', '/api/show', {'model': MODEL})
                warmup_status, _ = request('generate-warmup-default', '/api/generate', {
                    'model': MODEL, 'prompt': '', 'stream': False, 'keep_alive': '5m', 'options': {'num_predict': 1}})
                request('ps-after-warmup', '/api/ps')
                chat_status, chat = request('chat-default', '/api/chat', chat_body)
                request('ps-after-chat', '/api/ps')
                request('unload', '/api/generate', {'model': MODEL, 'keep_alive': 0})
                small_status, small = request('generate-small-cpu', '/api/generate', {
                    'model': MODEL, 'prompt': 'Say hello.', 'stream': False, 'think': False,
                    'options': {'num_ctx': 1024, 'num_batch': 32, 'num_thread': 2, 'num_gpu': 0,
                                'num_predict': 32, 'temperature': 0}})
                request('ps-after-small-cpu', '/api/ps')
                report['warmup_passed'] = warmup_status == 200
                report['default_chat_passed'] = chat_status == 200 and bool(chat.get('message', {}).get('content', '').strip())
                report['small_cpu_passed'] = small_status == 200 and bool(small.get('response', '').strip())
            finally:
                stop(server)
                report['baseline_server_exit'] = server.returncode
        baseline_log = (root / 'ollama.log').read_text(errors='replace')
        bad_requests = [entry for entry in report['requests'] if entry['status'] != 200]
        report['baseline_amx_used'] = 'AMX model buffer size' in baseline_log
        report['baseline_amx_sigsegv'] = bool(bad_requests) and report['baseline_amx_used'] and all(
            entry['status'] == 500 and re.search(r'(segmentation fault|SIGSEGV)', entry.get('error') or '', re.IGNORECASE)
            for entry in bad_requests)
        assert not bad_requests or report['baseline_amx_sigsegv'], 'Unexpected failure in the baseline requests'
        assert report['default_chat_passed'] or report['baseline_amx_sigsegv'], 'Baseline chat returned no answer'
        assert report['small_cpu_passed'] or report['baseline_amx_sigsegv'], 'Small CPU baseline returned no answer'

        library = extracted / 'lib/ollama/libggml-cpu-sapphirerapids.so'
        assert arch == 'amd64' and library.is_file() and not library.is_symlink(), 'Expected the pinned Sapphire Rapids library'
        before = {str(path.relative_to(extracted)) for path in extracted.rglob('*') if path.is_file()}
        quarantined = root / 'quarantined' / library.name
        quarantined.parent.mkdir()
        with library.open('rb') as stream:
            report['quarantined_library_sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
        library.rename(quarantined)
        after = {str(path.relative_to(extracted)) for path in extracted.rglob('*') if path.is_file()}
        assert before - after == {'lib/ollama/libggml-cpu-sapphirerapids.so'} and not after - before
        report['quarantined_library'] = 'lib/ollama/libggml-cpu-sapphirerapids.so'
        with (root / 'recovery-ollama.log').open('w') as log:
            server = start(log)
            try:
                status, answer = request('chat-after-quarantine', '/api/chat', chat_body)
                report['recovery_chat_passed'] = status == 200 and bool(answer.get('message', {}).get('content', '').strip())
                assert report['recovery_chat_passed'], answer
                request('ps-after-quarantine', '/api/ps')
            finally:
                stop(server)
                report['recovery_server_exit'] = server.returncode
        report['recovery_amx_used'] = 'AMX model buffer size' in (root / 'recovery-ollama.log').read_text(errors='replace')
        assert not report['recovery_amx_used'], 'Quarantined backend was still selected'
        report['status'] = 'pass'
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
        print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
