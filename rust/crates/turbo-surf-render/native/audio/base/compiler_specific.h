#pragma once
#if defined(_MSC_VER)
#define ALWAYS_INLINE __forceinline
#else
#define ALWAYS_INLINE inline __attribute__((always_inline))
#endif
#define UNSAFE_BUFFERS(...) __VA_ARGS__
