#!/usr/bin/env python3
"""AIRLLM bridge for MLX-Pilot.

Supports:
- original AirLLM AutoModel flow (layered loading strategy)
- legacy mlx_lm.generate flow
"""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import platform
import sys
from pathlib import Path


def log(message: str) -> None:
    sys.stderr.write(f"[airllm] {message}\n")
    sys.stderr.flush()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="MLX AIRLLM bridge")
    parser.add_argument("--model", required=True, help="Model path or repo id")
    parser.add_argument("--prompt", required=True, help="Prompt text")
    parser.add_argument(
        "--backend",
        default="auto",
        choices=("auto", "original", "legacy"),
        help="Execution backend. 'original' mirrors upstream AirLLM AutoModel flow.",
    )
    parser.add_argument(
        "--device",
        default="auto",
        choices=("auto", "cpu"),
        help="Execution device hint.",
    )
    parser.add_argument("--max-seq-len", type=int, default=2048, help="Tokenizer truncation max length")
    parser.add_argument("--max-tokens", type=int, default=256, help="Max new tokens")
    parser.add_argument("--temp", type=float, default=0.2, help="Sampling temperature")
    parser.add_argument("--top-p", type=float, default=1.0, help="Top-p sampling")
    parser.add_argument("--max-kv-size", type=int, default=1024, help="Max KV cache size")
    parser.add_argument("--kv-bits", type=int, default=4, help="KV quantization bits")
    parser.add_argument("--kv-group-size", type=int, default=64, help="KV group size")
    parser.add_argument(
        "--quantized-kv-start",
        type=int,
        default=0,
        help="Step to start KV quantization",
    )
    return parser.parse_args()


def run_legacy_backend(args: argparse.Namespace) -> str:
    import mlx.core as mx

    model_type = _model_type_from_config(args.model)
    log("backend=legacy (mlx_lm.generate)")
    if args.device == "cpu":
        mx.set_default_device(mx.cpu)
        log("default device set to cpu")

    from mlx_lm import generate, load
    from mlx_lm.sample_utils import make_sampler

    log("loading model/tokenizer")
    model, tokenizer = load(args.model, lazy=True)
    log("model/tokenizer loaded")
    sampler = make_sampler(args.temp, args.top_p, 0.0, 1, top_k=0)
    log("generating")
    base_kwargs = {
        "prompt": args.prompt,
        "max_tokens": max(1, args.max_tokens),
        "sampler": sampler,
        "verbose": False,
    }
    max_kv_size = max(128, args.max_kv_size)
    if args.device != "cpu" and model_type in {"qwen3_5_moe", "qwen3_5_moe_text"}:
        max_kv_size = min(max_kv_size, 256)
        log(f"clamping max_kv_size to {max_kv_size} for model_type='{model_type}' on non-cpu device")
    kv_kwargs = {
        "max_kv_size": max_kv_size,
        "kv_bits": max(1, args.kv_bits),
        "kv_group_size": max(1, args.kv_group_size),
        "quantized_kv_start": max(0, args.quantized_kv_start),
    }
    try:
        text = generate(model, tokenizer, **base_kwargs, **kv_kwargs)
    except Exception as exc:
        if "RotatingKVCache Quantization NYI" not in str(exc):
            raise
        log("kv quantization unsupported by this model; retrying without kv quantization args")
        text = generate(model, tokenizer, **base_kwargs)
    log("generation finished")
    return text.strip()


def _is_memory_pressure_error(exc: Exception) -> bool:
    text = f"{type(exc).__name__}: {exc}".lower()
    return any(
        marker in text
        for marker in (
            "insufficient memory",
            "out of memory",
            "outofmemory",
            "max_recommended_working_set_size",
            "kiogpucommandbuffercallbackerroroutofmemory",
        )
    )


def _model_type_from_config(model_ref: str) -> str:
    model_path = Path(model_ref)
    if not model_path.is_dir():
        return ""

    config_path = model_path / "config.json"
    if not config_path.exists():
        return ""

    try:
        with config_path.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except Exception:
        return ""

    if isinstance(payload, dict):
        model_type = str(payload.get("model_type") or "").strip()
        if model_type:
            return model_type.lower()
        text_config = payload.get("text_config")
        if isinstance(text_config, dict):
            model_type = str(text_config.get("model_type") or "").strip()
            if model_type:
                return model_type.lower()
    return ""


def _should_skip_original_backend(model_ref: str) -> tuple[bool, str]:
    model_type = _model_type_from_config(model_ref)
    if not model_type:
        return False, ""

    # Current upstream AirLLM path is unstable for these model families in our integration.
    unsupported = {"qwen3", "qwen3_5_moe", "qwen3_5_moe_text"}
    if model_type in unsupported:
        return True, model_type
    return False, model_type


def _decode_sequences(model, generation_output) -> str:
    sequences = None
    if hasattr(generation_output, "sequences"):
        sequences = generation_output.sequences
    elif isinstance(generation_output, dict):
        sequences = generation_output.get("sequences")

    if sequences is None:
        return str(generation_output).strip()

    if hasattr(sequences, "__getitem__"):
        first = sequences[0]
    else:
        return str(generation_output).strip()

    return model.tokenizer.decode(first, skip_special_tokens=True).strip()


def run_original_backend(args: argparse.Namespace) -> str:
    from airllm import AutoModel

    log("backend=original (airllm AutoModel)")
    model_kwargs: dict[str, str] = {}
    if args.device == "cpu":
        model_kwargs["device"] = "cpu"
    elif platform.system().lower() != "darwin":
        try:
            import torch

            if not torch.cuda.is_available():
                # AutoModel defaults to cuda:0; on hosts without CUDA force CPU explicitly.
                model_kwargs["device"] = "cpu"
                log("cuda unavailable; forcing original backend to cpu")
        except Exception:
            # Keep upstream default behavior if torch probing fails.
            pass

    model = AutoModel.from_pretrained(args.model, **model_kwargs)
    max_seq_len = max(64, int(args.max_seq_len))
    max_new_tokens = max(1, int(args.max_tokens))
    prompt_batch = [args.prompt]

    if platform.system().lower() == "darwin":
        import mlx.core as mx

        tokens = model.tokenizer(
            prompt_batch,
            return_tensors="np",
            return_attention_mask=False,
            truncation=True,
            max_length=max_seq_len,
            padding=False,
        )
        log("tokenized with return_tensors=np for macOS AirLLM")
        output = model.generate(
            mx.array(tokens["input_ids"]),
            max_new_tokens=max_new_tokens,
            use_cache=True,
            return_dict_in_generate=True,
        )
        text = str(output).strip()
        if text.startswith(args.prompt):
            text = text[len(args.prompt) :].strip()
        return text

    tokens = model.tokenizer(
        prompt_batch,
        return_tensors="pt",
        return_attention_mask=False,
        truncation=True,
        max_length=max_seq_len,
        padding=False,
    )

    input_ids = tokens["input_ids"]
    if args.device != "cpu" and hasattr(input_ids, "cuda"):
        try:
            input_ids = input_ids.cuda()
        except Exception:
            log("cuda unavailable; keeping input_ids on current device")

    output = model.generate(
        input_ids,
        max_new_tokens=max_new_tokens,
        use_cache=True,
        return_dict_in_generate=True,
    )
    text = _decode_sequences(model, output)

    if text.startswith(args.prompt):
        text = text[len(args.prompt) :].strip()

    return text.strip()


def run_backend(args: argparse.Namespace) -> str:
    def invoke_silenced(fn, fn_args: argparse.Namespace) -> str:
        # AirLLM/mlx_lm may print setup/progress noise to stdout.
        # Keep stdout reserved for the final answer payload only.
        with contextlib.redirect_stdout(io.StringIO()):
            return fn(fn_args)

    backend = (args.backend or "auto").strip().lower()
    if backend not in {"auto", "original", "legacy"}:
        backend = "auto"

    if backend in {"auto", "original"}:
        skip_original, model_type = _should_skip_original_backend(args.model)
        if backend == "auto" and skip_original:
            log(
                f"skipping original backend for model_type='{model_type}' due known incompatibility; using legacy"
            )
        else:
            try:
                return invoke_silenced(run_original_backend, args)
            except Exception as exc:
                if backend == "original":
                    raise
                log(f"original backend failed ({type(exc).__name__}: {exc}); falling back to legacy")

    legacy_args = argparse.Namespace(**vars(args))
    try:
        return invoke_silenced(run_legacy_backend, legacy_args)
    except Exception as exc:
        if legacy_args.device != "cpu" and _is_memory_pressure_error(exc):
            log("legacy backend hit memory pressure; retrying with device=cpu")
            legacy_args.device = "cpu"
            return invoke_silenced(run_legacy_backend, legacy_args)
        raise


def main() -> int:
    args = parse_args()
    log(
        "start backend={} device={} max_tokens={} max_seq_len={}".format(
            args.backend,
            args.device,
            max(1, args.max_tokens),
            max(64, args.max_seq_len),
        )
    )
    log(f"model={args.model}")

    text = run_backend(args)
    sys.stdout.write(text.strip())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # noqa: BLE001 - forward exact failure to Rust caller
        sys.stderr.write(f"{type(exc).__name__}: {exc}\n")
        raise
