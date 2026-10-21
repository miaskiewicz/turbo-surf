#pragma once
#include <cmath>
#include "third_party/fdlibm/ieee754.h"
namespace blink::audio_utilities{inline float DecibelsToLinear(float x){return powf(10,.05f*x);}inline float LinearToDecibels(float x){return 20*log10f(x);}inline double DiscreteTimeConstantForSampleRate(double t,double r){return 1-fdlibm::exp(-1/(r*t));}}
