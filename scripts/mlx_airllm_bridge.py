#!/usr/bin/env python3
"""
Rust-owned AIRLLM bridge for MLX-Pilot.

This keeps decision/policy in Rust and uses Python only for MLX runtime calls
that are not available natively in Rust yet.
"""

from __future__ import annotations

import argparse
import sys

import mlx.core as mx


def log(message: str) -> None:
    sys.stderr.write(f"[airllm] {message}\n")
    sys.stderr.flush()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="MLX AIRLLM bridge")
    parser.add_argument("--model", required=True, help="Model path or repo id")
    parser.add_argument("--prompt", required=True, help="Prompt text")
    parser.add_argument(
        "--device",
        default="auto",
        choices=("auto", "cpu"),
        help="Execution device. Use cpu for memory-constrained fallback.",
    )
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


def main() -> int:
    args = parse_args()
    log(f"start device={args.device} max_tokens={max(1, args.max_tokens)}")
    log(f"model={args.model}")

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
    text = generate(
        model,
        tokenizer,
        prompt=args.prompt,
        max_tokens=max(1, args.max_tokens),
        sampler=sampler,
        max_kv_size=max(128, args.max_kv_size),
        kv_bits=max(1, args.kv_bits),
        kv_group_size=max(1, args.kv_group_size),
        quantized_kv_start=max(0, args.quantized_kv_start),
        verbose=False,
    )
    log("generation finished")

    sys.stdout.write(text.strip())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # noqa: BLE001 - forward exact failure to Rust caller
        sys.stderr.write(f"{type(exc).__name__}: {exc}\n")
        raise
