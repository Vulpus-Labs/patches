# Drone (Radigue-inspired)

`examples/radigue_drone.patches` — a slowly evolving sine drone with diode ring
modulation, inspired by the tape-music practice of Eliane Radigue.

## Signal flow

Two pairs of sine oscillators (A and B) are tuned to adjacent sub-bass
frequencies. Each pair feeds a diode ring modulator (`ring_a`, `ring_b`). The
two ring mod outputs feed a final ring modulator (`ring_final`), generating sum
and difference tones.

Cross-feedback cables let each ring mod output lightly FM-modulate the drifting
oscillator of the other pair, creating slow non-linear mutual evolution without
hard synchronisation.

```
osc_a1 ──► ring_a ──► ring_final ──► mix[0]
osc_a2 ──►        └─[FM]──► osc_b2

osc_b1 ──► ring_b ──►               mix[0]
osc_b2 ──►        └─[FM]──► osc_a2

osc_a1 ─────────────────────────► mix[1]  (hard left)
osc_a2 ─────────────────────────► mix[2]  (left-centre)
osc_b1 ─────────────────────────► mix[3]  (right-centre)
osc_b2 ─────────────────────────► mix[4]  (hard right)
```

## Running it

```bash
cargo run -p patches-player -- examples/radigue_drone.patches
```

No MIDI device required. Try editing the `drift` values or the ring mod `drive`
parameters while it plays.

## Full patch

```patches
patch {
    module osc_a1 : Osc { frequency: 80Hz,  drift: 0.9, fm_type: linear }
    module osc_a2 : Osc { frequency: 159Hz, drift: 0.9, fm_type: logarithmic }
    module osc_b1 : Osc { frequency: 120Hz, drift: 0.9, fm_type: linear }
    module osc_b2 : Osc { frequency: 241Hz, drift: 0.9, fm_type: logarithmic }

    module ring_a     : RingMod { drive: 4.0 }
    module ring_b     : RingMod { drive: 4.0 }
    module ring_final : RingMod { drive: 4.0 }

    module mix : StereoMixer(channels: 5) {
        level[0]: 0.8,  pan[0]:  0.0,
        level[1]: 0.4,  pan[1]: -0.8,
        level[2]: 0.3,  pan[2]: -0.4,
        level[3]: 0.3,  pan[3]:  0.4,
        level[4]: 0.4,  pan[4]:  0.8
    }
    module out : AudioOut

    osc_a1.sine -> ring_a.signal
    osc_a2.sine -> ring_a.carrier
    osc_b1.sine -> ring_b.signal
    osc_b2.sine -> ring_b.carrier

    ring_a.out -> ring_final.signal
    ring_b.out -> ring_final.carrier

    ring_a.out    -[0.4]-> osc_b2.fm
    ring_b.out    -[0.2]-> osc_a2.fm
    ring_final.out        -> osc_a1.fm
    ring_final.out        -> osc_b1.fm

    ring_final.out -> mix.in[0]
    osc_a1.sine    -> mix.in[1]
    osc_a2.sine    -> mix.in[2]
    osc_b1.sine    -> mix.in[3]
    osc_b2.sine    -> mix.in[4]

    mix.out_left  -[0.1]-> out.in_left
    mix.out_right -[0.1]-> out.in_right
}
```
