# Native Chrome155 audio effects

The upstream directory preserves the full BSD-licensed Biquad, Delay, ARM NEON
delay and StereoPanner sources from Chrome155.0.8059.40. PROVENANCE.json records
SHA-256 for each original; the sibling compiled copies differ only by an isolated
namespace and the standalone platform-header include. The compiled ARM NEON
delay copy also includes <cmath> directly for std::isnan, without relying on
platform-specific transitive headers. Compatibility headers
provide containers and checked call boundaries without a browser runtime.

The adapter preserves macOS Accelerate double-precision Biquad processing,
original eight filter coefficient formulas, ARM NEON a-rate delay read-position
precision, and fdlibm equal-power panning. WaveShaper interpolation follows the
separate float vector operations in the preserved upstream wave_shaper_handler.cc;
fused or double interpolation changes the PCM. Real Biquad TailFrame and generic
bus-activity timing control tail suppression, independently from zero-valued
samples. Source activity is supplied by the graph renderer.

Independent Chrome155 controls prove exact PCM for lowpass, highpass, WaveShaper,
Delay and mono-to-stereo panning. These controls establish the covered kernels;
they do not prove feedback graphs, WaveShaper oversampling, full automation graph
connectivity or every filter/API behavior. Unsupported oversampling remains an
explicit renderer error. Delay memory is bounded to one million frames per channel.

Build the five C++ translation units with effects/include before native/audio,
C++20, NDEBUG and -ffp-contract=off; reuse native/audio's existing fdlibm object.
Link Accelerate on macOS. All originals retain their complete copyright and
redistribution notices. Root native/audio/LICENSE.chromium applies to Chromium
Authors files, including StereoPanner and ARM NEON Delay. Other originals carry
their complete BSD notices in the files.
