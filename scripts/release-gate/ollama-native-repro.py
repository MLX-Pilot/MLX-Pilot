#!/usr/bin/env python3
"""Isolate Ollama 0.30.7 native inference without compiling or launching MLX Pilot.

Run on the same Ubuntu runner as the failed package test:
    python3 ollama-native-repro.py /tmp/ollama-native-repro
The result directory contains the upstream archive digest, raw HTTP responses,
server log, CPU/memory/cgroup diagnostics and a machine-readable comparison.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
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
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}[platform.machine()]
    assert platform.system() == 'Linux'
    asset = f'ollama-linux-{arch}.tar.zst'
    url = f'https://github.com/ollama/ollama/releases/download/v{VERSION}/{asset}'
    archive = root / asset
    report = {'version': VERSION, 'model': MODEL, 'arch': arch, 'archive_url': url, 'requests': []}

    def command(name, args):
        result = subprocess.run(args, capture_output=True, text=True, timeout=20)
        (root / name).write_text(result.stdout + result.stderr)

    command('cpu.txt', ['lscpu'])
    command('memory.txt', ['free', '-b'])
    command('kernel.txt', ['uname', '-a'])
    for p in ('/proc/meminfo', '/sys/fs/cgroup/memory.max', '/sys/fs/cgroup/memory.events'):
        if Path(p).is_file():
            (root / (p.replace('/', '_').strip('_') + '.txt')).write_text(Path(p).read_text())
    subprocess.run(['curl', '--fail', '--location', '--retry', '3', '--continue-at', '-', '--output', str(archive), url], check=True)
    with archive.open('rb') as f:
        report['archive_sha256'] = hashlib.file_digest(f, 'sha256').hexdigest()
    manifest_url = f'https://api.github.com/repos/ollama/ollama/releases/tags/v{VERSION}'
    headers = {'User-Agent': 'MLX-Pilot-runtime-diagnostics'}
    if os.environ.get('GH_TOKEN'):
        headers['Authorization'] = 'Bearer ' + os.environ['GH_TOKEN']
    with urlopen(Request(manifest_url, headers=headers), timeout=30) as response:
        release = json.load(response)
    official = next(a for a in release['assets'] if a['name'] == asset)
    report['upstream_digest'] = official.get('digest')
    if official.get('digest'):
        assert official['digest'] == 'sha256:' + report['archive_sha256']
    extracted = root / 'runtime'
    extracted.mkdir()
    subprocess.run(['tar', '--zstd', '-xf', str(archive), '-C', str(extracted)], check=True)
    binary = extracted / 'bin/ollama'
    assert binary.is_file()
    command('binary.txt', ['file', str(binary)])
    command('dependencies.txt', ['ldd', str(binary)])
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port = s.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    env = dict(os.environ)
    env.pop('GH_TOKEN', None)
    for key in ('GPU_DEVICE_ORDINAL', 'HIP_VISIBLE_DEVICES', 'ROCR_VISIBLE_DEVICES', 'GGML_VK_VISIBLE_DEVICES', 'OLLAMA_LLM_LIBRARY', 'OLLAMA_VULKAN', 'CUDA_VISIBLE_DEVICES'):
        env.pop(key, None)
    env.update(OLLAMA_HOST=f'127.0.0.1:{port}', OLLAMA_MODELS=str(root/'models'), OLLAMA_DEBUG='INFO')

    def request(name, route, body=None, timeout=180):
        sent = None if body is None else json.dumps(body).encode()
        req = Request(base+route, data=sent, headers={'Content-Type':'application/json'})
        start = time.monotonic()
        try:
            with urlopen(req, timeout=timeout) as response:
                status, raw = response.status, response.read()
        except HTTPError as error:
            status, raw = error.code, error.read()
        except (URLError, OSError) as error:
            status, raw = 0, json.dumps({'error':str(error)}).encode()
        (root/f'{name}.json').write_bytes(raw)
        entry={'name':name, 'route':route, 'body':body, 'status':status, 'seconds':round(time.monotonic()-start,3)}
        report['requests'].append(entry)
        print(json.dumps(entry), flush=True)
        return status, json.loads(raw)

    with (root/'ollama.log').open('w') as log:
        server=subprocess.Popen([str(binary),'serve'],env=env,cwd=binary.parent,stdout=log,stderr=log)
        try:
            deadline=time.monotonic()+45
            while time.monotonic()<deadline:
                try:
                    with urlopen(base+'/api/version',timeout=1) as r:
                        version=json.load(r)['version']
                    assert version==VERSION
                    break
                except (URLError,OSError):
                    time.sleep(.25)
            else:
                raise RuntimeError('Upstream daemon did not become ready')
            status, result=request('pull','/api/pull',{'model':MODEL,'stream':False},timeout=900)
            assert status==200 and result.get('status')=='success'
            request('show','/api/show',{'model':MODEL})
            warmup_status, warmup = request('generate-warmup-default','/api/generate',{'model':MODEL,'prompt':'','stream':False,'keep_alive':'5m','options':{'num_predict':1}})
            request('ps-after-warmup','/api/ps')
            chat_status, chat = request('chat-default','/api/chat',{'model':MODEL,'messages':[{'role':'user','content':'Say hello in one short sentence. /no_think'}],'stream':False,'think':False,'options':{'temperature':0,'num_predict':128}})
            request('ps-after-chat','/api/ps')
            # This explicitly diagnostic variant helps distinguish default scheduling
            # and batch settings. It does not change MLX Pilot's configuration.
            request('unload','/api/generate',{'model':MODEL,'keep_alive':0})
            small_status, small = request('generate-small-cpu','/api/generate',{'model':MODEL,'prompt':'Say hello. /no_think','stream':False,'think':False,'options':{'num_ctx':1024,'num_batch':32,'num_thread':2,'num_gpu':0,'num_predict':32,'temperature':0}})
            request('ps-after-small-cpu','/api/ps')
            report['warmup_passed'] = warmup_status == 200
            report['default_chat_passed'] = chat_status == 200 and bool(chat.get('message', {}).get('content', '').strip())
            report['small_cpu_passed'] = small_status == 200 and bool(small.get('response', '').strip())
            report['status'] = 'pass' if report['default_chat_passed'] else 'reproduced_failure'
        finally:
            server.terminate()
            try:server.wait(timeout=15)
            except subprocess.TimeoutExpired:server.kill();server.wait()
            report['server_exit']=server.returncode
            (root/'report.json').write_text(json.dumps(report,indent=2))
            print(json.dumps(report,indent=2),flush=True)
    if not report.get('default_chat_passed'):
        raise SystemExit(1)


if __name__=='__main__':
    main()
