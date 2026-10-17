# Native offline audio evidence and limits

The previous Botto `audio-silent` result did not measure silent rendered audio.
The frozen pre-audio binary did not install `OfflineAudioContext`; the collector
caught `TypeError: AC is not a constructor` and mapped that failure to zero
energy. The old fixed synthetic waveform was disconnected from the graph and
has been removed.

The renderer now executes a real, bounded offline signal graph. Its oscillator
ports Chrome 155's PeriodicWave/RustFFT and ARM oscillator arithmetic; its
compressor uses the unchanged licensed Blink/fdlibm kernel. The DSP receives
actual graph connections, signal buffers and private parameter values. It never
substitutes stored PCM or an expected fingerprint.

## Reusable evidence

`audio-probe.js` separates oscillator-only, buffer/compressor-only and composed
signals. `audio-ownership-probe.js` tests parameter shadows, overridden sample
iterators, transferred channel buffers, copy bounds and trusted event order.
Both are normal detection-harness probes and can be run against a fresh Chrome
control and the rebuilt native CLI. `fixtures/audio-chrome155.json` stores the
independent Chrome 155 results without the large PCM arrays. Full PCM remains
in private artifacts; each public graph records its SHA256.

| Authored graph | Chrome quantized hash | Native ARM64 full PCM |
| --- | --- | --- |
| Triangle, 10,000 Hz | `9f58dfe3` | Exact SHA256 |
| Sine, 440 Hz | `fdca1b60` | Exact SHA256 |
| Square, 220 Hz → gain 0.25 | `2b8170b3` | Exact SHA256 |
| Triangle → compressor (-50, 40, 12, 0, 0.25) | `63b6b305` | Exact SHA256 |
| Sine, 880 Hz → compressor (-24, 20, 4, 0.01, 0.1) | `17d003f4` | Exact SHA256 |
| Authored PCM sine → fingerprint compressor | `a8d73295` | Matching probe hash |

The full-PCM assertions are macOS ARM64-specific because the independent control is
Chrome 155 on macOS ARM64 and uses that oscillator SIMD arithmetic. Other
architectures still run genuine graph, ownership and resource-limit contracts;
these results do not establish bit parity with their Chrome DSP kernels.

The public adapter uses real native function callbacks, opaque host objects and
private records. It preserves stable mutable channel views, method-realm result
ownership, associated context identity and native receiver checks. An own
`AudioParam.value` shadow and a channel's `Symbol.iterator` override do not alter
rendering. Transferring a channel buffer detaches its existing view; the measured
Chrome graph subsequently renders silence and succeeds. Completion follows the
measured running-statechange → ended (when due) → complete (closed) → closed-
statechange sequence, with trusted events.

Meaningful offline tests are `audio_contracts.rs`, the `native_audio` library
tests, and `native_dom_contracts.rs`. The authored graph assertions compare
independent Chrome hashes, not implementation-produced snapshots. Source
provenance and license notices are in `native/audio/PROVENANCE.json` and the
package's `THIRD_PARTY_NOTICES.md`.

## Supported graph and explicit limits

The current backend supports mono/stereo DAGs containing basic sine, square,
sawtooth or triangle oscillators, scheduled buffer sources, gain, the real
compressor, all eight Biquad filter types, Delay, WaveShaper without oversampling,
StereoPanner and a destination. Sources have real scheduled starts/stops;
buffer sources support offsets, durations, positive/negative playback rates,
detune, fractional looping and input-rate conversion. AudioParam events perform
real set-value, linear/exponential ramps, target convergence, value curves,
scheduled-event cancellation and hold. Compressor and buffer parameters retain
Chrome's fixed k-rate semantics. The graph tracks source activity separately
from zero-valued samples, so effect tails use genuine silent-bus lifecycle.

`audio-advanced-probe.js` and `audio-advanced-contract-probe.js` are reusable
Chrome/native controls. `fixtures/audio-advanced-chrome155.json` contains 32
independent authored graphs, their full Float32 PCM SHA256 digests, and one
shared authored input buffer. The native standalone DSP laboratory matches all
32 controls exactly on macOS ARM64, including scheduled starts, five automation
types, cancellation/hold, rate/loop/resampling, five effects and effect tails. Separate controls cover default and low-frequency
oscillators, reverse phase, delayed a-rate evaluation and frequency/detune
automation-rate arithmetic.
The isolated genuine V8 15.5.35.20 candidate passed all 13 audio integration contracts,
including these collectors, native brands, borrowed-realm ownership, trusted
completion events, detached channels and hostile index hooks. That isolated
renderer/MCP checkpoint passed 474 tests with GPU, CoreText, transport impersonation
and trust anchors enabled; strict all-targets Clippy also passes. These checks
certify this bounded implementation and its authored controls, not every Web
Audio operation.

It enforces 262,144 channel samples, 64 nodes, 8,388,608 node-frame work units,
256 events and 65,536 curve points per parameter, bounded delay allocation, and
a two-second native graph execution ceiling. Feedback cycles, AudioParam signal
connections, more than two rendered channels, custom periodic waves, WaveShaper
oversampling, nonfinite PCM and suspension/resume remain explicit errors or
unavailable methods. Native realtime device playback is unavailable and its
constructor reports `NotSupportedError`. No unsupported method silently
pretends to run DSP.

These are deliberate implementation limits, not a claim of full Web Audio
parity. More parameter combinations, event insertion/cancellation edge cases,
multichannel rules, realtime device lifecycle and browser resource policies
still need separate implementation and independent Chrome contracts. The audio
hashes alone do not prove Google acceptance; the full fresh traced Google run
must use the validated rebuilt binary.

## Deferred TODO

The remaining audio work is tracked in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).
Realtime playback, feedback graphs, AudioParam signal connections, wider channel
layouts, custom waves, oversampling and suspend/resume are deferred from the
current PR #26 completion scope at the user's request. Retain the existing DSP
and regression tests. Reprioritize a missing audio path if captured detector code
or traces demonstrate its relevance to a target; current audio controls do not
establish that Google's rejection is caused by audio.
