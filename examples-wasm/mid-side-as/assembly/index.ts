// Mid/Side splitter — AssemblyScript WASM module for the Patches host.
//
// Stereo → mid/side encoding, and mid/side → stereo decoding, in one module.
//
// Inputs:  in_left, in_right, mid_in, side_in
// Outputs: mid_out, side_out, out_left, out_right
//
// Encoding:  mid  = (L + R) * 0.5
//            side = (L - R) * 0.5
// Decoding:  L = mid + side
//            R = mid - side

// ── CableValue ABI constants ───────────────────────────────────────────────
// repr(C) enum CableValue { Mono(f32), Poly([f32;16]) }
// Layout: 4-byte i32 discriminant + 64-byte payload = 68 bytes
// [CableValue; 2] per staging slot = 136 bytes
const CABLE_VALUE_SIZE: i32 = 68;
const CABLE_SLOT_SIZE: i32 = 136; // [CableValue; 2]
const DISCRIMINANT_MONO: i32 = 0;

// ── FfiInputPort wire layout (WasmInputPort) ───────────────────────────────
// tag: u8 (offset 0), _pad: [u8;3], cable_idx: u32 (offset 4),
// scale: f32 (offset 8), connected: u8 (offset 12), _pad: [u8;3]
// Total: 16 bytes
const INPUT_PORT_SIZE: i32 = 16;

// ── FfiOutputPort wire layout (WasmOutputPort) ─────────────────────────────
// tag: u8 (offset 0), _pad: [u8;3], cable_idx: u32 (offset 4),
// connected: u8 (offset 8), _pad: [u8;3]
// Total: 12 bytes
const OUTPUT_PORT_SIZE: i32 = 12;

// ── Module state ───────────────────────────────────────────────────────────

// Port cable indices (0-based staging indices, set by patches_set_ports)
let inLeftIdx: i32 = 0;
let inRightIdx: i32 = 1;
let midInIdx: i32 = 2;
let sideInIdx: i32 = 3;
let midOutIdx: i32 = 4;
let sideOutIdx: i32 = 5;
let outLeftIdx: i32 = 6;
let outRightIdx: i32 = 7;

// Scales (from host port remapping)
let inLeftScale: f32 = 1.0;
let inRightScale: f32 = 1.0;
let midInScale: f32 = 1.0;
let sideInScale: f32 = 1.0;

// ── Cable read/write helpers ───────────────────────────────────────────────

// @ts-ignore: decorator
@inline
function readMono(cablePtr: i32, slotIdx: i32, writeIndex: i32, scale: f32): f32 {
  const readIndex = 1 - writeIndex;
  const base = cablePtr + slotIdx * CABLE_SLOT_SIZE + readIndex * CABLE_VALUE_SIZE;
  // Skip discriminant (offset 0), read f32 at offset 4
  return load<f32>(base + 4) * scale;
}

// @ts-ignore: decorator
@inline
function writeMono(cablePtr: i32, slotIdx: i32, writeIndex: i32, value: f32): void {
  const base = cablePtr + slotIdx * CABLE_SLOT_SIZE + writeIndex * CABLE_VALUE_SIZE;
  store<i32>(base, DISCRIMINANT_MONO); // discriminant
  store<f32>(base + 4, value);         // payload
}

// ── JSON descriptor (static) ───────────────────────────────────────────────

const DESCRIPTOR_JSON: string = `{
  "module_name": "MidSide",
  "shape": { "channels": 1, "length": 0, "high_quality": false },
  "inputs": [
    { "name": "in_left",  "index": 0, "kind": "mono" },
    { "name": "in_right", "index": 1, "kind": "mono" },
    { "name": "mid_in",   "index": 2, "kind": "mono" },
    { "name": "side_in",  "index": 3, "kind": "mono" }
  ],
  "outputs": [
    { "name": "mid_out",  "index": 0, "kind": "mono" },
    { "name": "side_out", "index": 1, "kind": "mono" },
    { "name": "out_left", "index": 2, "kind": "mono" },
    { "name": "out_right","index": 3, "kind": "mono" }
  ],
  "parameters": []
}`;

// ── Exported functions ─────────────────────────────────────────────────────

// Returns pointer to [len: u32 LE, ...JSON bytes] in WASM memory.
export function patches_describe(channels: i32, length: i32, hq: i32): i32 {
  // Encode the descriptor string as UTF-8 bytes
  const buf = String.UTF8.encode(DESCRIPTOR_JSON, false);
  const jsonLen = buf.byteLength;
  const totalLen = 4 + jsonLen;

  const ptr = heap.alloc(totalLen) as i32;
  // Write length prefix (u32 LE)
  store<u32>(ptr, jsonLen as u32);
  // Copy JSON bytes
  memory.copy(ptr + 4, changetype<usize>(buf), jsonLen);
  return ptr;
}

export function patches_prepare(
  descPtr: i32,
  descLen: i32,
  sampleRate: f32,
  polyVoices: i32,
  periodicInterval: i32,
  instanceIdLo: i32,
  instanceIdHi: i32,
): void {
  // No state to initialise beyond defaults — this module is stateless.
}

export function patches_process(cablePtr: i32, cableCount: i32, writeIndex: i32): void {
  // Read stereo inputs
  const l = readMono(cablePtr, inLeftIdx, writeIndex, inLeftScale);
  const r = readMono(cablePtr, inRightIdx, writeIndex, inRightScale);

  // Encode: stereo → mid/side
  const mid: f32 = (l + r) * 0.5;
  const side: f32 = (l - r) * 0.5;

  writeMono(cablePtr, midOutIdx, writeIndex, mid);
  writeMono(cablePtr, sideOutIdx, writeIndex, side);

  // Read mid/side return inputs
  const midIn = readMono(cablePtr, midInIdx, writeIndex, midInScale);
  const sideIn = readMono(cablePtr, sideInIdx, writeIndex, sideInScale);

  // Decode: mid/side → stereo
  writeMono(cablePtr, outLeftIdx, writeIndex, midIn + sideIn);
  writeMono(cablePtr, outRightIdx, writeIndex, midIn - sideIn);
}

export function patches_set_ports(
  inputsPtr: i32,
  inputsLen: i32,
  outputsPtr: i32,
  outputsLen: i32,
): void {
  // Read input port structs
  for (let i: i32 = 0; i < inputsLen; i++) {
    const base = inputsPtr + i * INPUT_PORT_SIZE;
    const cableIdx = load<u32>(base + 4) as i32;
    const scale = load<f32>(base + 8);

    if (i == 0) { inLeftIdx = cableIdx; inLeftScale = scale; }
    else if (i == 1) { inRightIdx = cableIdx; inRightScale = scale; }
    else if (i == 2) { midInIdx = cableIdx; midInScale = scale; }
    else if (i == 3) { sideInIdx = cableIdx; sideInScale = scale; }
  }

  // Read output port structs
  for (let i: i32 = 0; i < outputsLen; i++) {
    const base = outputsPtr + i * OUTPUT_PORT_SIZE;
    const cableIdx = load<u32>(base + 4) as i32;

    if (i == 0) { midOutIdx = cableIdx; }
    else if (i == 1) { sideOutIdx = cableIdx; }
    else if (i == 2) { outLeftIdx = cableIdx; }
    else if (i == 3) { outRightIdx = cableIdx; }
  }
}

export function patches_update_validated_parameters(paramsPtr: i32, paramsLen: i32): void {
  // No parameters — nothing to do.
}

export function patches_update_parameters(paramsPtr: i32, paramsLen: i32): i32 {
  // No parameters — always succeeds.
  return 0;
}

export function patches_periodic_update(cablePtr: i32, cableCount: i32, writeIndex: i32): i32 {
  return 0; // Not supported
}

export function patches_supports_periodic(): i32 {
  return 1;
}

export function patches_alloc(size: i32): i32 {
  return heap.alloc(size as usize) as i32;
}

export function patches_free(ptr: i32, size: i32): void {
  if (ptr == 0) return;
  heap.free(ptr as usize);
}
