// Adapter around the unchanged Chrome 155 BSD-licensed Blink DSP kernel.
#include "third_party/blink/renderer/platform/audio/dynamics_compressor.h"
#include "third_party/blink/renderer/platform/audio/audio_bus.h"
#include <algorithm>
#include <new>
struct SurfCompressor {
  blink::DynamicsCompressor kernel;
  blink::AudioBus input;
  blink::AudioBus output;
  SurfCompressor(float rate) : kernel(rate, 2), input(2, 128), output(2, 128) {}
};
extern "C" {
void* surf_audio_compressor_new(float rate, const float* parameters) {
  SurfCompressor* state;
  try { state = new (std::nothrow) SurfCompressor(rate); } catch (...) { return nullptr; }
  if (!state) return nullptr;
  const unsigned indices[] = {blink::DynamicsCompressor::kParamThreshold,
    blink::DynamicsCompressor::kParamKnee, blink::DynamicsCompressor::kParamRatio,
    blink::DynamicsCompressor::kParamAttack, blink::DynamicsCompressor::kParamRelease};
  for (unsigned i = 0; i < 5; ++i) state->kernel.SetParameterValue(indices[i], parameters[i]);
  return state;
}
void surf_audio_compressor_parameters(void* handle, const float* parameters) {
  auto& state = *static_cast<SurfCompressor*>(handle);
  const unsigned indices[] = {blink::DynamicsCompressor::kParamThreshold,
    blink::DynamicsCompressor::kParamKnee, blink::DynamicsCompressor::kParamRatio,
    blink::DynamicsCompressor::kParamAttack, blink::DynamicsCompressor::kParamRelease};
  for (unsigned i=0;i<5;++i) state.kernel.SetParameterValue(indices[i],parameters[i]);
}
void surf_audio_compressor_process(void* handle, const float* left, const float* right,
                                  float* out_left, float* out_right) {
  auto& state = *static_cast<SurfCompressor*>(handle);
  std::copy_n(left, 128, state.input.Channel(0)->MutableSpan().begin());
  std::copy_n(right, 128, state.input.Channel(1)->MutableSpan().begin());
  state.kernel.Process(&state.input, &state.output, 128);
  std::copy_n(state.output.Channel(0)->Span().begin(), 128, out_left);
  std::copy_n(state.output.Channel(1)->Span().begin(), 128, out_right);
}
float surf_audio_compressor_reduction(void* handle) {
  return static_cast<SurfCompressor*>(handle)->kernel.ParameterValue(blink::DynamicsCompressor::kParamReduction);
}
void surf_audio_compressor_free(void* handle) { delete static_cast<SurfCompressor*>(handle); }
}
