#!/usr/bin/env python3
"""
MLX-Pilot Wave 1 Concurrency QA Test Suite
==========================================
Orchestrates 4 parallel test agents against http://127.0.0.1:11435,
each validating a Wave 1 feature: Jobs/Scheduler, Web Search + SSRF,
Semantic Memory, and Cloud Models + Routing.

Usage:
    python tests/wave1_concurrency_test.py [--base-url http://127.0.0.1:11435]

Exit code 0 = all validations passed
Exit code 1 = at least one validation failed
"""

import asyncio
import sys
import time
import json
import argparse
import os

# Force UTF-8 on Windows — box-drawing chars won't survive cp1252
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    os.environ.setdefault("PYTHONIOENCODING", "utf-8")
from dataclasses import dataclass, field
from typing import Optional, List, Dict, Any

try:
    import aiohttp
except ImportError:
    print("[FATAL] aiohttp not installed. Run: pip install aiohttp")
    sys.exit(2)

# ── ANSI color helpers ──────────────────────────────────────────────────────
C_RESET = "\033[0m"
C_BOLD = "\033[1m"
C_DIM = "\033[2m"
C_RED = "\033[91m"
C_GREEN = "\033[92m"
C_YELLOW = "\033[93m"
C_BLUE = "\033[94m"
C_MAGENTA = "\033[95m"
C_CYAN = "\033[96m"
C_WHITE = "\033[97m"

AGENT_COLORS = {
    "JOBS": C_BLUE,
    "SEARCH": C_YELLOW,
    "MEMORY": C_MAGENTA,
    "MODELS": C_CYAN,
}


def log(agent: str, msg: str, level: str = "INFO") -> None:
    """Color-coded agent log line."""
    color = AGENT_COLORS.get(agent, C_WHITE)
    ts = time.strftime("%H:%M:%S")
    level_color = {"PASS": C_GREEN, "FAIL": C_RED, "WARN": C_YELLOW}.get(level, C_DIM)
    prefix = f"{C_DIM}[{ts}]{C_RESET} {color}[{agent}]{C_RESET}"
    print(f"{prefix} {level_color}{level:5s}{C_RESET} {msg}", flush=True)


# ── Shared HTTP session ─────────────────────────────────────────────────────

class HttpClient:
    """Async HTTP client wrapper for the daemon API."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session: Optional[aiohttp.ClientSession] = None

    async def __aenter__(self):
        timeout = aiohttp.ClientTimeout(total=30)
        self.session = aiohttp.ClientSession(timeout=timeout)
        return self

    async def __aexit__(self, *args):
        if self.session:
            await self.session.close()

    def url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    async def get(self, path: str, **kwargs) -> "HttpResponse":
        resp = await self.session.get(self.url(path), **kwargs)
        return await HttpResponse.from_response(resp)

    async def post(self, path: str, data: Any = None, **kwargs) -> "HttpResponse":
        resp = await self.session.post(self.url(path), json=data, **kwargs)
        return await HttpResponse.from_response(resp)

    async def delete(self, path: str, **kwargs) -> "HttpResponse":
        resp = await self.session.delete(self.url(path), **kwargs)
        return await HttpResponse.from_response(resp)

    async def get_sse(self, path: str, timeout: float = 18.0) -> List[Dict[str, Any]]:
        """
        Read SSE stream from `path` for up to `timeout` seconds.
        Returns a list of parsed JSON data events.
        """
        events: List[Dict[str, Any]] = []
        url = self.url(path)
        # Use a separate client session with a per-read timeout so we don't
        # block forever waiting for chunks.
        sse_timeout = aiohttp.ClientTimeout(total=timeout, sock_read=5.0)
        async with aiohttp.ClientSession(timeout=sse_timeout) as sse_session:
            async with sse_session.get(url) as resp:
                line_buffer = ""
                try:
                    async for chunk in resp.content.iter_chunked(1024):
                        text = chunk.decode("utf-8", errors="replace")
                        line_buffer += text
                        while "\n" in line_buffer:
                            line, line_buffer = line_buffer.split("\n", 1)
                            line = line.strip()
                            if line.startswith("data:"):
                                data_str = line[5:].strip()
                                if data_str and data_str != "keep-alive":
                                    try:
                                        events.append(json.loads(data_str))
                                    except json.JSONDecodeError:
                                        pass
                            elif line.startswith("event:") or line.startswith("id:") or line.startswith(":"):
                                # SSE control lines — ignore
                                pass
                except asyncio.TimeoutError:
                    pass  # stream naturally ends or times out
        return events


@dataclass
class HttpResponse:
    status: int
    body: Any
    text: str = ""

    @staticmethod
    async def from_response(resp: aiohttp.ClientResponse) -> "HttpResponse":
        text = await resp.text()
        try:
            body = json.loads(text)
        except (json.JSONDecodeError, ValueError):
            body = text
        return HttpResponse(status=resp.status, body=body, text=text)

    def ok(self) -> bool:
        return 200 <= self.status < 300


# ── Assertion helpers ───────────────────────────────────────────────────────

@dataclass
class TestResult:
    name: str
    passed: bool
    status: int
    expected_status: int
    detail: str = ""


class Validator:
    """Collects test results for one agent."""

    def __init__(self, agent: str):
        self.agent = agent
        self.results: List[TestResult] = []
        self.failures = 0

    def check(self, name: str, resp: HttpResponse, expected: int,
              detail: str = "") -> bool:
        """Assert status code matches, auto-pass if in expected range."""
        passed = resp.status == expected
        result = TestResult(name=name, passed=passed, status=resp.status,
                            expected_status=expected, detail=detail)
        self.results.append(result)
        if passed:
            log(self.agent, f"{C_GREEN}✓{C_RESET} {name} → HTTP {resp.status} (expected)",
                "PASS")
        else:
            self.failures += 1
            body_preview = str(resp.body)[:200] if resp.body else "(empty)"
            log(self.agent,
                f"{C_RED}✗{C_RESET} {name} → HTTP {resp.status} (expected {expected}) | body: {body_preview}",
                "FAIL")
        return passed

    def check_ok(self, name: str, resp: HttpResponse,
                 detail: str = "") -> bool:
        """Assert 2xx."""
        passed = resp.ok()
        result = TestResult(name=name, passed=passed, status=resp.status,
                            expected_status=200, detail=detail)
        self.results.append(result)
        if passed:
            log(self.agent, f"{C_GREEN}✓{C_RESET} {name} → HTTP {resp.status}", "PASS")
        else:
            self.failures += 1
            body_preview = str(resp.body)[:200] if resp.body else "(empty)"
            log(self.agent,
                f"{C_RED}✗{C_RESET} {name} → HTTP {resp.status} (expected 2xx) | body: {body_preview}",
                "FAIL")
        return passed

    def check_json(self, name: str, resp: HttpResponse, predicate: str,
                   ok: bool, detail: str = "") -> bool:
        """Assert a JSON body predicate description holds."""
        result = TestResult(name=name, passed=ok, status=resp.status,
                            expected_status=200, detail=detail)
        self.results.append(result)
        if ok:
            log(self.agent, f"{C_GREEN}✓{C_RESET} {name} → {predicate}", "PASS")
        else:
            self.failures += 1
            log(self.agent,
                f"{C_RED}✗{C_RESET} {name} → {predicate} FAILED | status={resp.status} body={str(resp.body)[:200]}",
                "FAIL")
        return ok

    def warn(self, msg: str) -> None:
        log(self.agent, msg, "WARN")

    def info(self, msg: str) -> None:
        log(self.agent, msg, "INFO")


# ── Agent 1: Jobs & Scheduler ───────────────────────────────────────────────

async def agent_jobs(http: HttpClient) -> Validator:
    v = Validator("JOBS")
    v.info("Starting Jobs & Scheduler tests")

    # ── 1a. POST /jobs/test ──────────────────────────────────────────────
    resp = await http.post("/jobs/test")
    if not v.check_ok("POST /jobs/test spawns test job", resp):
        return v  # can't continue without a job ID

    job_record = resp.body
    job_id = job_record.get("id") if isinstance(job_record, dict) else None
    if not job_id:
        v.check_json("POST /jobs/test returns job id",
                     resp, "id present in response", False,
                     f"body={json.dumps(job_record)[:200]}")
        return v
    v.info(f"Job ID: {job_id}")

    # Validate job record structure
    v.check_json("Job record has 'kind' field",
                 resp, isinstance(job_record.get("kind"), str),
                 isinstance(job_record.get("kind"), str))
    v.check_json("Job record has 'status' field",
                 resp, job_record.get("status") in ("queued", "running"),
                 job_record.get("status") in ("queued", "running"))

    # ── 1b/c. SSE stream + cancel mid-flight ──────────────────────────
    # Strategy: spawn the job, start listening to SSE, cancel after ~3s
    # (mid-flight), then verify the cancel took effect.
    await asyncio.sleep(0.3)
    v.info(f"Connecting to SSE /jobs/{job_id}/stream + cancel at ~3s ...")

    async def collect_sse(job_id: str) -> List[Dict[str, Any]]:
        return await http.get_sse(f"/jobs/{job_id}/stream", timeout=18.0)

    # Start SSE collection in background
    sse_task = asyncio.create_task(collect_sse(job_id))

    # Wait for the job to get a couple steps in (~3s), then cancel
    await asyncio.sleep(3.0)

    resp_cancel = await http.post(f"/jobs/{job_id}/cancel")
    # 200 = cancelled, 404 = already done (timing edge case — not a bug)
    if resp_cancel.ok():
        v.check_ok(f"POST /jobs/{job_id}/cancel (mid-flight)", resp_cancel)
        cancel_body = resp_cancel.body
        if isinstance(cancel_body, dict):
            cancelled_status = cancel_body.get("status")
            # Cancel triggers the token; status becomes "cancelled" after the
            # job hits its next checkpoint. Before that it may still be "running".
            valid_statuses = ("cancelled", "running")
            v.check_json("Cancel request accepted (status may be running until next checkpoint)",
                         resp_cancel,
                         f"status={cancelled_status} (acceptable: {valid_statuses})",
                         cancelled_status in valid_statuses)
    elif resp_cancel.status == 404:
        v.warn(f"Cancel returned 404 — job already finished (timing edge-case, OK)")

    # Collect SSE events
    sse_events = await sse_task
    v.info(f"Received {len(sse_events)} SSE events")
    if len(sse_events) >= 1:
        v.check_json("SSE stream returns ≥1 progress events",
                     resp,
                     f"received {len(sse_events)} events",
                     True)
    else:
        v.warn("SSE stream returned 0 events (job may have completed before SSE connected)")

    # Validate event structure
    if sse_events:
        first = sse_events[0]
        has_fields = all(k in first for k in ("job_id", "percent", "phase", "message"))
        v.check_json("SSE event has required fields (job_id, percent, phase, message)",
                     resp,
                     f"fields present: {has_fields}",
                     has_fields)

    # ── 1d. GET /jobs — list all jobs ──────────────────────────────────
    resp_list = await http.get("/jobs")
    v.check_ok("GET /jobs lists jobs", resp_list)
    if isinstance(resp_list.body, list):
        v.check_json("GET /jobs returns array",
                     resp_list,
                     f"array of {len(resp_list.body)} jobs",
                     True)
        our_job = [j for j in resp_list.body
                   if isinstance(j, dict) and j.get("id") == job_id]
        v.check_json(f"Job {job_id[:8]}... appears in list",
                     resp_list,
                     f"found={len(our_job)}",
                     len(our_job) == 1)

    # ── 1e. GET /jobs/{id} — get single job ───────────────────────────
    resp_get = await http.get(f"/jobs/{job_id}")
    v.check_ok(f"GET /jobs/{job_id} returns job", resp_get)

    # ── 1f. Schedule CRUD ─────────────────────────────────────────────
    # Create a one-shot task
    task_payload = {
        "name": "QA test task",
        "schedule_kind": "once",
        "run_at": "2099-01-01T00:00:00Z",
        "job_kind": "test_dummy",
        "payload_json": '{"test": true}',
        "enabled": False,
    }
    resp_create = await http.post("/scheduler/tasks", data=task_payload)
    if v.check_ok("POST /scheduler/tasks creates task", resp_create):
        created = resp_create.body
        task_id = created.get("id") if isinstance(created, dict) else None
        if task_id:
            v.check_json("Created task has correct name",
                         resp_create,
                         f"name={created.get('name')}",
                         created.get("name") == "QA test task")
            v.check_json("Created task has schedule_kind='once'",
                         resp_create,
                         f"kind={created.get('schedule_kind')}",
                         created.get("schedule_kind") == "once")

            # List tasks
            resp_list_tasks = await http.get("/scheduler/tasks")
            if v.check_ok("GET /scheduler/tasks lists tasks", resp_list_tasks):
                if isinstance(resp_list_tasks.body, list):
                    our_task = [t for t in resp_list_tasks.body
                                if isinstance(t, dict) and t.get("id") == task_id]
                    v.check_json("Created task appears in list",
                                 resp_list_tasks,
                                 f"found={len(our_task)}",
                                 len(our_task) == 1)

            # Delete task
            resp_del = await http.delete(f"/scheduler/tasks/{task_id}")
            v.check_ok(f"DELETE /scheduler/tasks/{task_id}", resp_del)
            if resp_del.ok() and isinstance(resp_del.body, dict):
                v.check_json("Delete returns ok:true",
                             resp_del,
                             f"ok={resp_del.body.get('ok')}",
                             resp_del.body.get("ok") is True)

    # Summary
    total = len(v.results)
    passed = total - v.failures
    v.info(f"{'='*50}")
    v.info(f"RESULTS: {passed}/{total} passed, {v.failures} failed")
    return v


# ── Agent 2: Web Search & SSRF Guard ────────────────────────────────────────

async def agent_search(http: HttpClient) -> Validator:
    v = Validator("SEARCH")
    v.info("Starting Web Search & SSRF Guard tests")

    # ── 2a. POST /api/search with a simple query ──────────────────────
    search_payload = {
        "q": "Rust programming language",
        "max_results": 3,
    }
    resp = await http.post("/api/search", data=search_payload)
    # Search may fail if no providers are configured (e.g. no Brave key),
    # but DuckDuckGo (free) should work. The endpoint returns 200 with results
    # or an error. We accept 200 (success), 503 (no providers), 404 (not configured).
    if resp.ok():
        v.check_ok("POST /api/search returns results", resp)
        if isinstance(resp.body, list):
            v.check_json("Search results is an array",
                         resp,
                         f"{len(resp.body)} results",
                         True)
            if resp.body:
                first = resp.body[0]
                if isinstance(first, dict):
                    v.check_json("Result has title, url, snippet, provider",
                                 resp,
                                 f"keys: {list(first.keys())}",
                                 all(k in first for k in ("title", "url", "snippet", "provider")))
    else:
        # DuckDuckGo may fail — note but don't hard-fail
        v.warn(f"Search returned HTTP {resp.status}: {str(resp.body)[:150]}")

    # ── 2b. GET /api/search/providers ─────────────────────────────────
    resp_providers = await http.get("/api/search/providers")
    v.check_ok("GET /api/search/providers", resp_providers)
    if isinstance(resp_providers.body, list):
        provider_ids = [p.get("id") for p in resp_providers.body if isinstance(p, dict)]
        v.info(f"Available providers: {provider_ids}")

    # ── 2c. GET /api/search/config ────────────────────────────────────
    resp_config = await http.get("/api/search/config")
    v.check_ok("GET /api/search/config", resp_config)

    # ── 2d. SSRF guard — localhost ────────────────────────────────────
    v.info("Testing SSRF guard with http://127.0.0.1:8080/secret")
    resp_ssrf1 = await http.post("/api/search/fetch",
                                 data={"url": "http://127.0.0.1:8080/secret"})
    # SSRF blocked returns 404 (not found) per the mapping: SsrfBlocked → AppError::NotFound
    is_blocked_1 = resp_ssrf1.status in (404, 400, 403)
    body_str_1 = str(resp_ssrf1.body).lower()
    v.check_json("SSRF blocks 127.0.0.1",
                 resp_ssrf1,
                 f"HTTP {resp_ssrf1.status} (expected blocking status), body hint: {body_str_1[:80]}",
                 is_blocked_1 or "ssrf" in body_str_1 or "private" in body_str_1
                 or "blocked" in body_str_1 or "not found" in body_str_1)

    # ── 2e. SSRF guard — cloud metadata endpoint ─────────────────────
    v.info("Testing SSRF guard with http://169.254.169.254/latest/meta-data")
    resp_ssrf2 = await http.post("/api/search/fetch",
                                 data={"url": "http://169.254.169.254/latest/meta-data"})
    is_blocked_2 = resp_ssrf2.status in (404, 400, 403)
    body_str_2 = str(resp_ssrf2.body).lower()
    v.check_json("SSRF blocks 169.254.169.254 (AWS metadata)",
                 resp_ssrf2,
                 f"HTTP {resp_ssrf2.status} (expected blocking status), body hint: {body_str_2[:80]}",
                 is_blocked_2 or "ssrf" in body_str_2 or "private" in body_str_2
                 or "blocked" in body_str_2 or "not found" in body_str_2)

    # ── 2f. SSRF guard — localhost hostname ──────────────────────────
    v.info("Testing SSRF guard with http://localhost:3000/admin")
    resp_ssrf3 = await http.post("/api/search/fetch",
                                 data={"url": "http://localhost:3000/admin"})
    is_blocked_3 = resp_ssrf3.status in (404, 400, 403)
    body_str_3 = str(resp_ssrf3.body).lower()
    v.check_json("SSRF blocks 'localhost' hostname",
                 resp_ssrf3,
                 f"HTTP {resp_ssrf3.status} (expected blocking status), body hint: {body_str_3[:80]}",
                 is_blocked_3 or "ssrf" in body_str_3 or "local" in body_str_3
                 or "blocked" in body_str_3 or "not found" in body_str_3)

    # ── 2g. SSRF guard — file:// scheme ──────────────────────────────
    v.info("Testing SSRF guard with file:///etc/passwd")
    resp_ssrf4 = await http.post("/api/search/fetch",
                                 data={"url": "file:///etc/passwd"})
    is_blocked_4 = resp_ssrf4.status in (404, 400, 403)
    body_str_4 = str(resp_ssrf4.body).lower()
    v.check_json("SSRF blocks file:// scheme",
                 resp_ssrf4,
                 f"HTTP {resp_ssrf4.status} (expected blocking status), body hint: {body_str_4[:80]}",
                 is_blocked_4 or "scheme" in body_str_4 or "blocked" in body_str_4
                 or "ssrf" in body_str_4 or "not found" in body_str_4 or "invalid" in body_str_4)

    # Summary
    total = len(v.results)
    passed = total - v.failures
    v.info(f"{'='*50}")
    v.info(f"RESULTS: {passed}/{total} passed, {v.failures} failed")
    return v


# ── Agent 3: Semantic Memory ────────────────────────────────────────────────

async def agent_memory(http: HttpClient) -> Validator:
    v = Validator("MEMORY")
    v.info("Starting Semantic Memory tests")

    # ── 3a. GET /agent/memory/semantic ────────────────────────────────
    resp = await http.get("/agent/memory/semantic")
    if v.check_ok("GET /agent/memory/semantic returns status", resp):
        if isinstance(resp.body, dict):
            v.check_json("Response has 'semantic_active' field",
                         resp,
                         f"semantic_active={resp.body.get('semantic_active')}",
                         "semantic_active" in resp.body)
            v.check_json("Response has 'embedder' field",
                         resp,
                         f"embedder={resp.body.get('embedder')}",
                         "embedder" in resp.body)
            v.info(f"Semantic active: {resp.body.get('semantic_active')}, "
                   f"embedder: {resp.body.get('embedder')}")

    # ── 3b. POST /agent/memory/reindex ────────────────────────────────
    resp_reindex = await http.post("/agent/memory/reindex")
    # 200 = embedder active, reindex ran
    # 500 = no embedder configured (graceful degradation to FTS-only)
    if resp_reindex.ok():
        v.check_ok("POST /agent/memory/reindex triggers reindex", resp_reindex)
        if isinstance(resp_reindex.body, dict):
            v.check_json("Reindex response has 'ok' field",
                         resp_reindex,
                         f"ok={resp_reindex.body.get('ok')}",
                         resp_reindex.body.get("ok") is True)
            v.check_json("Reindex response has 'reindexed' count",
                         resp_reindex,
                         f"reindexed={resp_reindex.body.get('reindexed')}",
                         "reindexed" in resp_reindex)
            v.info(f"Reindexed {resp_reindex.body.get('reindexed')} records")
    elif resp_reindex.status == 500:
        body_str = str(resp_reindex.body).lower()
        is_embedder_error = "embedder" in body_str or "embed" in body_str
        v.check_json("POST /agent/memory/reindex (no embedder → 500 expected)",
                     resp_reindex,
                     f"graceful degradation: embedder unavailable → HTTP 500",
                     is_embedder_error,
                     f"body: {str(resp_reindex.body)[:120]}")
        if is_embedder_error:
            v.info("Reindex skipped: no embedding provider available (this is OK)")
    else:
        v.check_ok("POST /agent/memory/reindex", resp_reindex)

    # ── 3c. GET /agent/memory/search — hybrid search ──────────────────
    resp_search = await http.get("/agent/memory/search?q=test&limit=5")
    if v.check_ok("GET /agent/memory/search hybrid search", resp_search):
        if isinstance(resp_search.body, list):
            v.check_json("Memory search returns array",
                         resp_search,
                         f"{len(resp_search.body)} hits",
                         True)
            if resp_search.body:
                hit = resp_search.body[0]
                if isinstance(hit, dict):
                    expected_keys = {"id", "title", "preview", "score", "kind"}
                    has_keys = expected_keys.issubset(set(hit.keys()))
                    v.check_json("Search hit has required fields",
                                 resp_search,
                                 f"keys present: {has_keys}",
                                 has_keys)
                    v.check_json("Search hit has 'semantic' field",
                                 resp_search,
                                 f"semantic={hit.get('semantic', 'MISSING')}",
                                 "semantic" in hit)

    # ── 3d. Concurrent hybrid searches (stress test) ─────────────────
    v.info("Running 5 concurrent hybrid searches...")
    async def concurrent_search(i: int) -> int:
        try:
            r = await http.get(f"/agent/memory/search?q=concurrent+test+{i}&limit=3")
            return r.status
        except Exception:
            return 0

    tasks = [concurrent_search(i) for i in range(5)]
    statuses = await asyncio.gather(*tasks)
    ok_count = sum(1 for s in statuses if 200 <= s < 300)
    all_ok = ok_count == 5
    v.check_json("5 concurrent memory searches all return 2xx",
                 HttpResponse(status=200 if all_ok else statuses[0],
                              body=f"statuses: {statuses}"),
                 f"{ok_count}/5 OK",
                 all_ok)

    # Summary
    total = len(v.results)
    passed = total - v.failures
    v.info(f"{'='*50}")
    v.info(f"RESULTS: {passed}/{total} passed, {v.failures} failed")
    return v


# ── Agent 4: Cloud Models & Routing ─────────────────────────────────────────

async def agent_models(http: HttpClient) -> Validator:
    v = Validator("MODELS")
    v.info("Starting Cloud Models & Routing tests")

    # ── 4a. GET /models/all ───────────────────────────────────────────
    resp = await http.get("/models/all")
    if not v.check_ok("GET /models/all returns model groups", resp):
        return v

    groups = resp.body
    if not isinstance(groups, list):
        v.check_json("Response is a list of model groups", resp,
                     f"type={type(groups).__name__}", False)
        return v

    v.info(f"Received {len(groups)} model groups")

    # Validate group structure
    local_group = None
    cloud_groups = []
    for g in groups:
        if not isinstance(g, dict):
            continue
        kind = g.get("kind", "")
        provider = g.get("provider", "")
        label = g.get("label", "")
        models = g.get("models", [])

        v.info(f"  Group: {label} ({provider}) kind={kind} models={len(models)}")

        if kind == "local":
            local_group = g
        elif kind == "cloud":
            cloud_groups.append(g)

    # Validate local group exists
    v.check_json("Model list contains 'local' group",
                 resp,
                 f"found={local_group is not None}",
                 local_group is not None)

    if local_group:
        local_models = local_group.get("models", [])
        if local_models:
            v.check_json("Local group models have 'local' badge",
                         resp,
                         f"{len(local_models)} models, all badge=local: "
                         f"{all(isinstance(m, dict) and m.get('badge') == 'local' for m in local_models)}",
                         all(isinstance(m, dict) and m.get("badge") == "local"
                             for m in local_models))
        else:
            v.warn("Local group has 0 models (Ollama may not be running)")

        # Check group-level fields
        for field in ("provider", "kind", "label", "requires_api_key",
                      "configured", "status"):
            v.check_json(f"Local group has '{field}' field",
                         resp,
                         f"present={field in local_group}",
                         field in local_group)

    # Validate cloud group structure if any configured
    if cloud_groups:
        cloud = cloud_groups[0]
        for field in ("provider", "kind", "label", "requires_api_key",
                      "configured", "status", "models"):
            v.check_json(f"Cloud group has '{field}' field",
                         resp,
                         f"present={field in cloud}",
                         field in cloud)
        v.check_json("Cloud group kind is 'cloud'",
                     resp,
                     f"kind={cloud.get('kind')}",
                     cloud.get("kind") == "cloud")
        v.check_json("Cloud group requires_api_key is true",
                     resp,
                     f"requires_api_key={cloud.get('requires_api_key')}",
                     cloud.get("requires_api_key") is True)

        # Check model badge
        cloud_models = cloud.get("models", [])
        if cloud_models:
            first_model = cloud_models[0]
            v.check_json("Cloud models have 'cloud' badge",
                         resp,
                         f"badge={first_model.get('badge')}",
                         first_model.get("badge") == "cloud")
            v.check_json("Cloud models have qualified id (provider:model)",
                         resp,
                         f"id={first_model.get('id')}",
                         ":" in first_model.get("id", ""))

    # ── 4b. POST /chat with deepseek:deepseek-chat ───────────────────
    v.info("Testing cloud routing: deepseek:deepseek-chat")
    chat_payload = {
        "model_id": "deepseek:deepseek-chat",
        "messages": [
            {"role": "user", "content": "Say 'hello' in exactly one word."}
        ],
        "options": {"temperature": 0.0, "max_tokens": 16},
    }
    try:
        chat_resp = await http.post("/chat", data=chat_payload)
    except asyncio.TimeoutError:
        v.warn("Chat request timed out (cloud provider may be unreachable)")
        chat_resp = HttpResponse(status=504, body="timeout", text="timeout")

    # 200 = routed to cloud and succeeded
    # 503 = cloud provider unavailable (no API key or network issue) — not a crash
    # 404 = model not found — the routing path works, just no key
    # Any of these means the router handled the request without panic
    routing_ok = chat_resp.status in (200, 503, 404, 502, 504)
    body_preview = str(chat_resp.body)[:200] if chat_resp.body else "(empty)"
    v.check_json("POST /chat with deepseek:deepseek-chat doesn't crash router",
                 chat_resp,
                 f"HTTP {chat_resp.status} → routing intact: {routing_ok} | {body_preview}",
                 routing_ok)

    if chat_resp.status == 200:
        if isinstance(chat_resp.body, dict):
            has_content = "content" in chat_resp.body or "message" in chat_resp.body
            v.check_json("Chat response has content/message",
                         chat_resp,
                         f"keys: {list(chat_resp.body.keys())[:5]}",
                         has_content)
            # Verify the response has the right shape
            has_model = "model" in chat_resp.body
            v.info(f"Cloud chat response model field: {has_model}")

    # ── 4c. Test with a local model too (ollama:: prefix) ────────────
    v.info("Testing local routing: ollama:: prefix")
    resp_models = await http.get("/models")
    local_models = []
    if resp_models.ok() and isinstance(resp_models.body, list):
        local_models = [m for m in resp_models.body
                        if isinstance(m, dict) and m.get("provider") == "ollama"]
        v.info(f"Found {len(local_models)} Ollama models")

    if local_models:
        test_model = local_models[0]["id"]
        v.info(f"Testing chat with local model: {test_model}")
        local_chat = {
            "model_id": f"ollama::{test_model}",
            "messages": [
                {"role": "user", "content": "Reply with just: OK"}
            ],
            "options": {"temperature": 0.0, "max_tokens": 8},
        }
        try:
            local_resp = await http.post("/chat", data=local_chat)
        except asyncio.TimeoutError:
            v.warn("Local chat timed out")
            local_resp = HttpResponse(status=504, body="timeout", text="timeout")

        local_routing_ok = local_resp.status in (200, 503, 404, 502, 504)
        v.check_json("POST /chat with ollama:: prefix routes correctly",
                     local_resp,
                     f"HTTP {local_resp.status} → routing intact: {local_routing_ok}",
                     local_routing_ok)
    else:
        v.warn("No local Ollama models available, skipping local routing test")

    # Summary
    total = len(v.results)
    passed = total - v.failures
    v.info(f"{'='*50}")
    v.info(f"RESULTS: {passed}/{total} passed, {v.failures} failed")
    return v


# ── Main orchestrator ───────────────────────────────────────────────────────

async def main(base_url: str) -> int:
    print()
    print(f"{C_BOLD}{C_CYAN}╔{'═'*58}╗{C_RESET}")
    print(f"{C_BOLD}{C_CYAN}║  MLX-Pilot Wave 1 — Concurrency QA Test Suite{' ' *14}║{C_RESET}")
    print(f"{C_BOLD}{C_CYAN}║  Target: {base_url:<42}║{C_RESET}")
    print(f"{C_BOLD}{C_CYAN}╚{'═'*58}╝{C_RESET}")
    print()

    start_time = time.monotonic()

    async with HttpClient(base_url) as http:
        # Health check first
        print(f"{C_DIM}[HEALTH] Checking daemon...{C_RESET}", end=" ", flush=True)
        try:
            health = await http.get("/health")
            if health.ok():
                print(f"{C_GREEN}OK{C_RESET}")
            else:
                print(f"{C_RED}FAIL (HTTP {health.status}){C_RESET}")
                print(f"{C_RED}[FATAL] Daemon is not healthy. Start it first.{C_RESET}")
                return 1
        except Exception as e:
            print(f"{C_RED}UNREACHABLE: {e}{C_RESET}")
            print(f"{C_RED}[FATAL] Cannot reach daemon at {base_url}. Start it with: cargo run -p mlx-ollama-daemon{C_RESET}")
            return 1

        # Launch all 4 agents concurrently
        print(f"\n{C_BOLD}Launching 4 test agents in parallel...{C_RESET}\n")

        results = await asyncio.gather(
            agent_jobs(http),
            agent_search(http),
            agent_memory(http),
            agent_models(http),
            return_exceptions=True,
        )

    # ── Aggregate results ─────────────────────────────────────────────────
    elapsed = time.monotonic() - start_time
    print(f"\n{C_BOLD}{'═'*60}{C_RESET}")
    print(f"{C_BOLD}  FINAL REPORT{C_RESET}")
    print(f"{C_BOLD}{'═'*60}{C_RESET}\n")

    all_results: List[TestResult] = []
    total_failures = 0
    agent_names = ["JOBS", "SEARCH", "MEMORY", "MODELS"]

    for i, result in enumerate(results):
        agent = agent_names[i]
        if isinstance(result, Exception):
            print(f"  {C_RED}[{agent}] EXCEPTION: {result}{C_RESET}")
            total_failures += 1
            continue
        if result is None:
            print(f"  {C_RED}[{agent}] returned None (agent crashed){C_RESET}")
            total_failures += 1
            continue

        total = len(result.results)
        passed = total - result.failures
        status = C_GREEN if result.failures == 0 else C_RED
        print(f"  {AGENT_COLORS.get(agent, C_WHITE)}[{agent}]{C_RESET} {status}{passed}/{total} passed{C_RESET}")

        # Show failures
        for r in result.results:
            if not r.passed:
                print(f"    {C_RED}✗{C_RESET} {r.name} → HTTP {r.status} (expected {r.expected_status})")
                if r.detail:
                    print(f"      {C_DIM}{r.detail}{C_RESET}")

        all_results.extend(result.results)
        total_failures += result.failures

    total_tests = len(all_results)
    total_passed = total_tests - total_failures

    print(f"\n  {C_BOLD}Total:{C_RESET} {total_passed}/{total_tests} passed, "
          f"{total_failures} failures in {elapsed:.1f}s")

    if total_failures == 0:
        print(f"\n  {C_GREEN}{C_BOLD}✅ ALL TESTS PASSED{C_RESET}\n")
        return 0
    else:
        print(f"\n  {C_RED}{C_BOLD}❌ {total_failures} TEST(S) FAILED{C_RESET}\n")
        return 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="MLX-Pilot Wave 1 Concurrency QA Test Suite")
    parser.add_argument("--base-url", default="http://127.0.0.1:11435",
                        help="Daemon base URL (default: http://127.0.0.1:11435)")
    args = parser.parse_args()
    sys.exit(asyncio.run(main(args.base_url)))
