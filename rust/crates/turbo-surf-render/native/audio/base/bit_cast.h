#pragma once
#if __has_include(<bit>)
#include <bit>
#endif

#if defined(__cpp_lib_bit_cast) && __cpp_lib_bit_cast >= 201806L
namespace base { using std::bit_cast; }
#else
#include <cstring>
#include <type_traits>

namespace base {
// Older manylinux libraries omit bit_cast; these audio callers use runtime values.
template <typename To, typename From>
To bit_cast(const From& source) noexcept {
  static_assert(sizeof(To) == sizeof(From), "bit_cast requires equal sizes");
  static_assert(std::is_trivially_copyable<To>::value,
                "bit_cast destination must be trivially copyable");
  static_assert(std::is_trivially_copyable<From>::value,
                "bit_cast source must be trivially copyable");
  To destination;
  std::memcpy(&destination, &source, sizeof(To));
  return destination;
}
}  // namespace base
#endif
