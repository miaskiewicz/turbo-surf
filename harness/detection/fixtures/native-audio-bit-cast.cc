#include <cstdint>
#include "base/bit_cast.h"

#if !defined(TURBO_SURF_TEST_EXPECT_LEGACY) && defined(__cpp_lib_bit_cast) && __cpp_lib_bit_cast >= 201806L
static_assert(base::bit_cast<std::uint64_t>(1.0) == UINT64_C(0x3ff0000000000000));
constexpr const char* kBranch = "std";
#else
constexpr const char* kBranch = "fallback";
#endif

#include <array>
#include <cstdio>
#include <cstdlib>

#if defined(__ARM_NEON) || defined(__ARM_NEON__)
#include <arm_neon.h>
#elif defined(__SSE2__)
#include <emmintrin.h>
#endif

static void require(bool condition) {
  if (!condition) std::abort();
}

int main() {
  const std::uint32_t floats[] = {
      0, UINT32_C(0x80000000), 1, UINT32_C(0x80000001),
      UINT32_C(0x7f800000), UINT32_C(0xff800000),
      UINT32_C(0x7fc00042), UINT32_C(0xffc01234),
      UINT32_C(0x3f800000), UINT32_C(0xbf800000),
      UINT32_C(0x7f7fffff), UINT32_C(0xff7fffff)};
  for (auto bits : floats)
    require(base::bit_cast<std::uint32_t>(base::bit_cast<float>(bits)) == bits);
  const std::uint64_t doubles[] = {
      0, UINT64_C(0x8000000000000000), 1, UINT64_C(0x8000000000000001),
      UINT64_C(0x7ff0000000000000), UINT64_C(0xfff0000000000000),
      UINT64_C(0x7ff8000000000042), UINT64_C(0xfff8000000001234),
      UINT64_C(0x3ff0000000000000), UINT64_C(0xbff0000000000000),
      UINT64_C(0x7fefffffffffffff), UINT64_C(0xffefffffffffffff)};
  for (auto bits : doubles)
    require(base::bit_cast<std::uint64_t>(base::bit_cast<double>(bits)) == bits);
  const std::array<std::uint32_t, 4> lanes = {floats[0], floats[1], floats[6], floats[2]};
  require(base::bit_cast<std::array<std::uint32_t, 4>>(
              base::bit_cast<std::array<float, 4>>(lanes)) == lanes);
#if defined(__ARM_NEON) || defined(__ARM_NEON__)
  const auto vector = vreinterpretq_f32_u32(vld1q_u32(lanes.data()));
  require(base::bit_cast<std::array<std::uint32_t, 4>>(
              base::bit_cast<std::array<float, 4>>(vector)) == lanes);
#elif defined(__SSE2__)
  const auto vector = _mm_castsi128_ps(_mm_loadu_si128(
      reinterpret_cast<const __m128i*>(lanes.data())));
  require(base::bit_cast<std::array<std::uint32_t, 4>>(
              base::bit_cast<std::array<float, 4>>(vector)) == lanes);
#endif
  std::puts(kBranch);
}
