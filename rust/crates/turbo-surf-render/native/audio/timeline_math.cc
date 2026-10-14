// Native math adapter for the licensed Chromium fdlibm implementation.
#include "third_party/fdlibm/ieee754.h"
extern "C" double surf_audio_pow(double x, double y) { return fdlibm::pow(x, y); }
extern "C" double surf_audio_exp(double x) { return fdlibm::exp(x); }
