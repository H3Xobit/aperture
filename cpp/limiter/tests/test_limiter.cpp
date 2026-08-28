#include "aperture_limiter.h"

#include <cassert>
#include <chrono>
#include <cstdio>
#include <thread>

int main() {
  ApertureLimiter* lim = aperture_limiter_create(5, 100.0);
  assert(lim != nullptr);

  const char* acme = "11111111-1111-1111-1111-111111111111";
  const char* north = "22222222-2222-2222-2222-222222222222";

  for (int i = 0; i < 5; ++i) {
    assert(aperture_limiter_allow(lim, acme, 1) == 1);
  }
  assert(aperture_limiter_allow(lim, acme, 1) == 0);
  assert(aperture_limiter_allow(lim, north, 1) == 1);

  std::this_thread::sleep_for(std::chrono::milliseconds(80));
  assert(aperture_limiter_allow(lim, acme, 1) == 1);

  assert(aperture_limiter_allow(lim, nullptr, 1) == -1);
  assert(aperture_limiter_allow(nullptr, acme, 1) == -1);
  assert(aperture_limiter_remaining(lim, north) >= 0.0);

  aperture_limiter_destroy(lim);
  std::puts("ok");
  return 0;
}
