;; A real Extism guest: forward the complete input to the capability broker.
;; quota repeats that request four times in one invocation; spin exhausts fuel.
(module
  (import "extism:host/env" "input_length" (func $input_length (result i64)))
  (import "extism:host/env" "input_load_u8" (func $input_load_u8 (param i64) (result i32)))
  (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
  (import "extism:host/env" "store_u8" (func $store_u8 (param i64 i32)))
  (import "extism:host/env" "length" (func $length (param i64) (result i64)))
  (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
  (import "extism:host/user" "mlx_host_call" (func $host (param i64) (result i64)))
  (func $forward (param $count i32) (result i32)
    (local $ptr i64) (local $len i64) (local $i i64) (local $response i64)
    (local.set $len (call $input_length))
    (local.set $ptr (call $alloc (local.get $len)))
    (block $done (loop $copy
      (br_if $done (i64.ge_u (local.get $i) (local.get $len)))
      (call $store_u8 (i64.add (local.get $ptr) (local.get $i)) (call $input_load_u8 (local.get $i)))
      (local.set $i (i64.add (local.get $i) (i64.const 1)))
      (br $copy)))
    (loop $requests
      (local.set $response (call $host (local.get $ptr)))
      (local.set $count (i32.sub (local.get $count) (i32.const 1)))
      (br_if $requests (local.get $count)))
    (call $output_set (local.get $response) (call $length (local.get $response)))
    (i32.const 0))
  (func (export "broker") (result i32) (call $forward (i32.const 1)))
  (func (export "quota") (result i32) (call $forward (i32.const 4)))
  (func (export "spin") (result i32) (loop $forever (br $forever)) (i32.const 0)))
