#pragma once
#include <cstdint>
#include <tuple>
#include <cstdlib>
#define BUILDFLAG(x) x
#if defined(__aarch64__)
#define CPU_ARM_NEON 1
#endif
#ifdef __APPLE__
#define IS_MAC 1
#else
#define IS_MAC 0
#endif
#define DCHECK(...) ((void)0)
#define DCHECK_EQ(...) ((void)0)
#define DCHECK_GE(...) ((void)0)
#define DCHECK_GT(...) ((void)0)
#define DCHECK_LE(...) ((void)0)
#define DCHECK_LT(...) ((void)0)
#define CHECK(x) do { if (!(x)) std::abort(); } while (false)
#define CHECK_EQ(a,b) CHECK((a)==(b))
#define CHECK_GE(a,b) CHECK((a)>=(b))
#define CHECK_LT(a,b) CHECK((a)<(b))
