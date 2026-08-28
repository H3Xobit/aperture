#include "aperture_limiter.h"

#include <chrono>
#include <mutex>
#include <string>
#include <unordered_map>
#include <algorithm>
#include <cmath>

namespace {

using clock = std::chrono::steady_clock;

struct Bucket {
  double tokens;
  clock::time_point last;
};

class Limiter {
 public:
  Limiter(uint64_t capacity, double refill_per_sec)
      : capacity_(static_cast<double>(capacity)),
        refill_per_sec_(refill_per_sec) {
    if (capacity_ < 1.0) {
      capacity_ = 1.0;
    }
    if (refill_per_sec_ < 0.0) {
      refill_per_sec_ = 0.0;
    }
  }

  bool allow(const std::string& tenant, uint32_t cost) {
    std::lock_guard<std::mutex> lock(mu_);
    Bucket& b = refill_locked(tenant);
    const double need = static_cast<double>(cost);
    if (b.tokens + 1e-9 < need) {
      return false;
    }
    b.tokens -= need;
    return true;
  }

  double remaining(const std::string& tenant) {
    std::lock_guard<std::mutex> lock(mu_);
    return refill_locked(tenant).tokens;
  }

 private:
  Bucket& refill_locked(const std::string& tenant) {
    const auto now = clock::now();
    auto it = buckets_.find(tenant);
    if (it == buckets_.end()) {
      Bucket fresh{capacity_, now};
      it = buckets_.emplace(tenant, fresh).first;
      return it->second;
    }
    Bucket& b = it->second;
    const double elapsed =
        std::chrono::duration<double>(now - b.last).count();
    if (elapsed > 0.0 && refill_per_sec_ > 0.0) {
      b.tokens = std::min(capacity_, b.tokens + elapsed * refill_per_sec_);
    }
    b.last = now;
    return b;
  }

  std::mutex mu_;
  std::unordered_map<std::string, Bucket> buckets_;
  double capacity_;
  double refill_per_sec_;
};

}  // namespace

extern "C" {

ApertureLimiter* aperture_limiter_create(uint64_t capacity, double refill_per_sec) {
  try {
    auto* lim = new Limiter(capacity, refill_per_sec);
    return reinterpret_cast<ApertureLimiter*>(lim);
  } catch (...) {
    return nullptr;
  }
}

void aperture_limiter_destroy(ApertureLimiter* limiter) {
  if (!limiter) {
    return;
  }
  delete reinterpret_cast<Limiter*>(limiter);
}

int aperture_limiter_allow(ApertureLimiter* limiter, const char* tenant_id,
                           uint32_t cost) {
  if (!limiter || !tenant_id || cost == 0) {
    return -1;
  }
  try {
    auto* lim = reinterpret_cast<Limiter*>(limiter);
    return lim->allow(std::string(tenant_id), cost) ? 1 : 0;
  } catch (...) {
    return -1;
  }
}

double aperture_limiter_remaining(ApertureLimiter* limiter,
                                  const char* tenant_id) {
  if (!limiter || !tenant_id) {
    return -1.0;
  }
  try {
    auto* lim = reinterpret_cast<Limiter*>(limiter);
    return lim->remaining(std::string(tenant_id));
  } catch (...) {
    return -1.0;
  }
}

}  // extern "C"
