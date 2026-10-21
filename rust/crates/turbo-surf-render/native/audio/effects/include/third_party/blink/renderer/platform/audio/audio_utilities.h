#pragma once
#include <cmath>
namespace surf_audio_effects::audio_utilities{
enum SampleFrameRounding{kRoundToNearest,kRoundDown,kRoundUp};
inline size_t TimeToSampleFrame(double t,double rate,SampleFrameRounding r){double frame=std::round(t*rate*1024)/1024;return static_cast<size_t>(r==kRoundUp?std::ceil(frame):r==kRoundDown?std::floor(frame):std::round(frame));}}
