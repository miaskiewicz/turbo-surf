#pragma once
#include <cmath>
namespace blink{struct DenormalDisabler{static float FlushDenormalFloatToZero(float x){return std::fpclassify(x)==FP_SUBNORMAL?0.f:x;}};}
