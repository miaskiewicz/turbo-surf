#pragma once
#include <algorithm>
#include <limits>
namespace surf_audio_effects {
constexpr double kPiDouble=3.14159265358979323846264338327950288;
constexpr double kPiOverTwoDouble=kPiDouble/2;
template<class V,class B>B ClampTo(V x,B lo,B hi=std::numeric_limits<B>::max()){return std::clamp(static_cast<B>(x),lo,hi);}
}
