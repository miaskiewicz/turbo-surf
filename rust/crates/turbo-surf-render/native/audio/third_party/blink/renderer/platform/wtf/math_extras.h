#pragma once
#include <algorithm>
namespace blink{constexpr float kPiOverTwoFloat=1.5707963267948966f;template<class T>T ClampTo(T x,T lo,T hi){return std::clamp(x,lo,hi);}}
