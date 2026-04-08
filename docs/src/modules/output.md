# Audio output

## `AudioOut` — Stereo audio output

Sends signals to the left and right channels of the system audio output.
This module must appear exactly once in a patch.

**Inputs**

| Port | Description |
|---|---|
| `in_left` | Left channel |
| `in_right` | Right channel |

No clamping is applied by `AudioOut` itself. Use attenuation (scaled cables or
a `StereoMixer`) to keep levels in range — summing many signals without
attenuation will overdrive the output.

**Example**

```patches
mix.out_left  -[0.1]-> out.in_left
mix.out_right -[0.1]-> out.in_right
```
