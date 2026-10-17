#pragma once
#include "base/containers/span.h"
namespace surf_audio_effects::vector_math{
inline void Vsub(base::span<const float>a,base::span<const float>b,base::span<float>out,size_t n){for(size_t i=0;i<n;i++)out[i]=a[i]-b[i];}
inline void Vsma(base::span<const float>a,float scale,base::span<float>out,size_t n){for(size_t i=0;i<n;i++)out[i]=out[i]+scale*a[i];}}
