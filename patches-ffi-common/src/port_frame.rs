//! `PortFrame` — fixed-layout port bindings for ADR 0045 §5.
//!
//! Mirrors the parameter data plane: host packs the descriptor-sized frame
//! on the control thread, audio thread reads a borrowed [`PortView`] on the
//! plugin side. Wire format is a `#[repr(C)]` header followed by typed
//! arrays of [`crate::FfiInputPort`] / [`crate::FfiOutputPort`] structs.
//!
//! The frame owns a single pre-allocated `Vec<u8>` sized at `prepare` time
//! from the module's `PortLayout`. Packing overwrites in place; no audio-
//! thread allocation on either side.

use std::mem::{align_of, size_of};

use patches_core::{InputPort, OutputPort};

use crate::types::{FfiInputPort, FfiOutputPort};

/// `#[repr(C)]` port-frame header. Trailing bytes carry
/// `[FfiInputPort; input_count]` followed by `[FfiOutputPort; output_count]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortFrameHeader {
    /// Module pool index for this frame.
    pub idx: u32,
    pub input_count: u32,
    pub output_count: u32,
}

/// Fixed descriptor-derived layout for a module's [`PortFrame`]. Host and
/// plugin compute this independently from the descriptor's port counts at
/// `prepare`; shape never changes over an instance's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortLayout {
    pub input_count: u32,
    pub output_count: u32,
    pub input_offset: usize,
    pub output_offset: usize,
    pub total_size: usize,
}

impl PortLayout {
    /// Derive a layout for a module whose descriptor declares `input_count`
    /// inputs and `output_count` outputs. Panics on overflow rather than
    /// silently wrapping.
    pub fn new(input_count: u32, output_count: u32) -> Self {
        let header_size = size_of::<PortFrameHeader>();
        let in_align = align_of::<FfiInputPort>();
        let out_align = align_of::<FfiOutputPort>();

        let in_off = align_up_checked(header_size, in_align);
        let in_bytes = checked_array_bytes::<FfiInputPort>(input_count);
        let after_inputs = in_off
            .checked_add(in_bytes)
            .expect("PortLayout: input array size overflow");
        let out_off = align_up_checked(after_inputs, out_align);
        let out_bytes = checked_array_bytes::<FfiOutputPort>(output_count);
        let total = out_off
            .checked_add(out_bytes)
            .expect("PortLayout: output array size overflow");

        Self {
            input_count,
            output_count,
            input_offset: in_off,
            output_offset: out_off,
            total_size: total,
        }
    }
}

fn align_up_checked(offset: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    offset
        .checked_add(align - 1)
        .expect("PortLayout: alignment overflow")
        & !(align - 1)
}

fn checked_array_bytes<T>(count: u32) -> usize {
    (count as usize)
        .checked_mul(size_of::<T>())
        .expect("PortLayout: array size overflow")
}

/// Owned, fixed-size packed port frame.
#[derive(Debug)]
pub struct PortFrame {
    bytes: Vec<u8>,
    layout: PortLayout,
}

impl PortFrame {
    /// Allocate a zero-filled frame shaped for `layout`.
    pub fn with_layout(layout: PortLayout) -> Self {
        let bytes = vec![0u8; layout.total_size];
        Self { bytes, layout }
    }

    pub fn layout(&self) -> &PortLayout {
        &self.layout
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Borrow a typed view. Header fields are read via `PortView::header`.
    pub fn view(&self) -> PortView<'_> {
        PortView { layout: &self.layout, bytes: &self.bytes }
    }
}

/// Errors raised when host-side input is shape-incompatible with the
/// frame's layout. These are control-thread / planner bugs; the audio
/// thread never sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackPortsError {
    InputCountMismatch { expected: u32, actual: usize },
    OutputCountMismatch { expected: u32, actual: usize },
}

impl std::fmt::Display for PackPortsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputCountMismatch { expected, actual } => {
                write!(f, "port-frame input count mismatch: layout={expected}, got {actual}")
            }
            Self::OutputCountMismatch { expected, actual } => {
                write!(f, "port-frame output count mismatch: layout={expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for PackPortsError {}

/// Control-thread encoder: write `idx`, input port structs, and output port
/// structs into `frame`. The frame is reshaped at `prepare`; this call
/// never allocates.
pub fn pack_ports_into(
    idx: u32,
    inputs: &[InputPort],
    outputs: &[OutputPort],
    frame: &mut PortFrame,
) -> Result<(), PackPortsError> {
    let layout = frame.layout;
    if inputs.len() != layout.input_count as usize {
        return Err(PackPortsError::InputCountMismatch {
            expected: layout.input_count,
            actual: inputs.len(),
        });
    }
    if outputs.len() != layout.output_count as usize {
        return Err(PackPortsError::OutputCountMismatch {
            expected: layout.output_count,
            actual: outputs.len(),
        });
    }

    let header = PortFrameHeader {
        idx,
        input_count: layout.input_count,
        output_count: layout.output_count,
    };
    let bytes = frame.bytes_mut();
    // SAFETY: header is `#[repr(C)]` POD; bytes slice is at least
    // `size_of::<PortFrameHeader>()` because layout.total_size >= input_offset
    // >= header_size.
    unsafe {
        std::ptr::write_unaligned(bytes.as_mut_ptr() as *mut PortFrameHeader, header);
    }

    let in_off = layout.input_offset;
    for (i, ip) in inputs.iter().enumerate() {
        let ffi = FfiInputPort::from(ip);
        let off = in_off + i * size_of::<FfiInputPort>();
        // SAFETY: off + size fits in total_size by PortLayout::new.
        unsafe {
            std::ptr::write_unaligned(bytes.as_mut_ptr().add(off) as *mut FfiInputPort, ffi);
        }
    }

    let out_off = layout.output_offset;
    for (i, op) in outputs.iter().enumerate() {
        let ffi = FfiOutputPort::from(op);
        let off = out_off + i * size_of::<FfiOutputPort>();
        // SAFETY: off + size fits in total_size by PortLayout::new.
        unsafe {
            std::ptr::write_unaligned(bytes.as_mut_ptr().add(off) as *mut FfiOutputPort, ffi);
        }
    }

    Ok(())
}

/// Borrowed view over a packed port frame. Audio-thread reader.
#[derive(Clone, Copy)]
pub struct PortView<'a> {
    layout: &'a PortLayout,
    bytes: &'a [u8],
}

impl<'a> PortView<'a> {
    /// Wrap an externally-supplied frame buffer (e.g. one received across
    /// the FFI boundary) with its layout.
    pub fn new(layout: &'a PortLayout, bytes: &'a [u8]) -> Self {
        debug_assert!(bytes.len() >= layout.total_size);
        Self { layout, bytes }
    }

    pub fn layout(&self) -> &PortLayout {
        self.layout
    }

    pub fn header(&self) -> PortFrameHeader {
        // SAFETY: bytes is at least header-sized (PortLayout::new enforces).
        unsafe { std::ptr::read_unaligned(self.bytes.as_ptr() as *const PortFrameHeader) }
    }

    pub fn input_count(&self) -> usize {
        self.layout.input_count as usize
    }

    pub fn output_count(&self) -> usize {
        self.layout.output_count as usize
    }

    pub fn input(&self, i: usize) -> FfiInputPort {
        assert!(i < self.input_count(), "PortView::input out of bounds");
        let off = self.layout.input_offset + i * size_of::<FfiInputPort>();
        // SAFETY: layout guarantees off + size <= total_size <= bytes.len().
        unsafe { std::ptr::read_unaligned(self.bytes.as_ptr().add(off) as *const FfiInputPort) }
    }

    pub fn output(&self, i: usize) -> FfiOutputPort {
        assert!(i < self.output_count(), "PortView::output out of bounds");
        let off = self.layout.output_offset + i * size_of::<FfiOutputPort>();
        // SAFETY: layout guarantees off + size <= total_size <= bytes.len().
        unsafe { std::ptr::read_unaligned(self.bytes.as_ptr().add(off) as *const FfiOutputPort) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{MonoInput, MonoOutput, PolyInput, PolyOutput};

    fn mono_in(idx: usize, scale: f32, connected: bool) -> InputPort {
        InputPort::Mono(MonoInput { cable_idx: idx, scale, connected })
    }
    fn poly_in(idx: usize, scale: f32, connected: bool) -> InputPort {
        InputPort::Poly(PolyInput { cable_idx: idx, scale, connected })
    }
    fn mono_out(idx: usize, connected: bool) -> OutputPort {
        OutputPort::Mono(MonoOutput { cable_idx: idx, connected })
    }
    fn poly_out(idx: usize, connected: bool) -> OutputPort {
        OutputPort::Poly(PolyOutput { cable_idx: idx, connected })
    }

    #[test]
    fn layout_shape_common_sizes() {
        let l = PortLayout::new(0, 0);
        assert_eq!(l.input_count, 0);
        assert_eq!(l.output_count, 0);
        assert!(l.total_size >= size_of::<PortFrameHeader>());

        let l2 = PortLayout::new(3, 2);
        assert!(l2.input_offset >= size_of::<PortFrameHeader>());
        assert!(l2.output_offset >= l2.input_offset + 3 * size_of::<FfiInputPort>());
        assert_eq!(l2.total_size, l2.output_offset + 2 * size_of::<FfiOutputPort>());
    }

    #[test]
    fn round_trip_mixed_shapes() {
        let shapes: &[(u32, u32)] = &[(0, 0), (1, 0), (0, 1), (1, 1), (3, 2), (8, 8)];
        for &(nin, nout) in shapes {
            let layout = PortLayout::new(nin, nout);
            let mut frame = PortFrame::with_layout(layout);

            let inputs: Vec<InputPort> = (0..nin)
                .map(|i| {
                    if i % 2 == 0 {
                        mono_in(i as usize * 3, 0.25 * (i as f32 + 1.0), i % 3 != 0)
                    } else {
                        poly_in(i as usize * 5, 0.5 * (i as f32 + 1.0), i % 2 == 0)
                    }
                })
                .collect();
            let outputs: Vec<OutputPort> = (0..nout)
                .map(|i| {
                    if i % 2 == 0 {
                        mono_out(i as usize * 7, i % 2 == 0)
                    } else {
                        poly_out(i as usize * 11, i % 3 == 0)
                    }
                })
                .collect();

            pack_ports_into(42, &inputs, &outputs, &mut frame).unwrap();

            let view = frame.view();
            let h = view.header();
            assert_eq!(h.idx, 42);
            assert_eq!(h.input_count, nin);
            assert_eq!(h.output_count, nout);
            assert_eq!(view.input_count(), nin as usize);
            assert_eq!(view.output_count(), nout as usize);

            for (i, ip) in inputs.iter().enumerate() {
                let expected = FfiInputPort::from(ip);
                let got = view.input(i);
                assert_eq!(got.tag, expected.tag);
                assert_eq!(got.cable_idx, expected.cable_idx);
                assert_eq!(got.scale, expected.scale);
                assert_eq!(got.connected, expected.connected);
            }
            for (i, op) in outputs.iter().enumerate() {
                let expected = FfiOutputPort::from(op);
                let got = view.output(i);
                assert_eq!(got.tag, expected.tag);
                assert_eq!(got.cable_idx, expected.cable_idx);
                assert_eq!(got.connected, expected.connected);
            }
        }
    }

    #[test]
    fn pack_rejects_count_mismatch() {
        let layout = PortLayout::new(2, 1);
        let mut frame = PortFrame::with_layout(layout);
        let err = pack_ports_into(0, &[mono_in(0, 1.0, true)], &[mono_out(0, true)], &mut frame)
            .unwrap_err();
        assert_eq!(err, PackPortsError::InputCountMismatch { expected: 2, actual: 1 });

        let err = pack_ports_into(
            0,
            &[mono_in(0, 1.0, true), mono_in(1, 1.0, true)],
            &[],
            &mut frame,
        )
        .unwrap_err();
        assert_eq!(err, PackPortsError::OutputCountMismatch { expected: 1, actual: 0 });
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    #[should_panic(expected = "overflow")]
    fn layout_overflow_panics_not_ub_32bit() {
        // On 32-bit, u32::MAX * sizeof(FfiInputPort) blows past usize::MAX.
        let _ = PortLayout::new(u32::MAX, 0);
    }
}
