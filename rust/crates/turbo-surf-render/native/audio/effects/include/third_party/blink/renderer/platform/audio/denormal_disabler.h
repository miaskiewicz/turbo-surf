#pragma once
#include <cmath>
namespace surf_audio_effects{struct DenormalDisabler{static float FlushDenormalFloatToZero(float x){return std::fpclassify(x)==FP_SUBNORMAL?0.f:x;}};}
