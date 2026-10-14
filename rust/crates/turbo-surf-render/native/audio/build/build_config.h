#pragma once
#if defined(__aarch64__) || defined(_M_ARM64)
#define ARCH_CPU_ARM64 1
#elif defined(__x86_64__) || defined(_M_X64)
#define ARCH_CPU_X86_64 1
#endif
#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#define ARCH_CPU_BIG_ENDIAN 1
#else
#define ARCH_CPU_LITTLE_ENDIAN 1
#endif
