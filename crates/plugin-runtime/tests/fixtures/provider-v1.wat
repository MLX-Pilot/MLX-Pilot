;; Deterministic, compute-only guest for the inference-provider ABI probe.
(module
  (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
  (import "extism:host/env" "store_u8" (func $store (param i64 i32)))
  (import "extism:host/env" "output_set" (func $output (param i64 i64)))
  (memory 1)
  (data (i32.const 0) "{\22result\22:[{\22id\22:\22tiny\22,\22name\22:\22Tiny fixture\22,\22provider\22:\22plugin.fixture\22,\22path\22:\22tiny\22,\22is_available\22:true}]}")
  (data (i32.const 512) "{\22result\22:{\22model_id\22:\22tiny\22,\22provider\22:\22plugin.fixture\22,\22message\22:{\22role\22:\22assistant\22,\22content\22:\22pong\22},\22usage\22:{\22prompt_tokens\22:1,\22completion_tokens\22:1,\22total_tokens\22:2},\22latency_ms\22:0,\22raw_output\22:null}}")

  (func $reply (param $start i32) (result i32)
    (local $len i32) (local $i i32) (local $dest i64)
    (block $found_end (loop $scan
      (br_if $found_end (i32.eqz (i32.load8_u (i32.add (local.get $start) (local.get $len)))))
      (local.set $len (i32.add (local.get $len) (i32.const 1)))
      (br $scan)))
    (local.set $dest (call $alloc (i64.extend_i32_u (local.get $len))))
    (block $done (loop $copy
      (br_if $done (i32.eq (local.get $i) (local.get $len)))
      (call $store (i64.add (local.get $dest) (i64.extend_i32_u (local.get $i)))
        (i32.load8_u (i32.add (local.get $start) (local.get $i))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $copy)))
    (call $output (local.get $dest) (i64.extend_i32_u (local.get $len)))
    (i32.const 0))

  (func (export "provider_list_models") (result i32) (call $reply (i32.const 0)))
  (func (export "provider_chat") (result i32) (call $reply (i32.const 512))))
