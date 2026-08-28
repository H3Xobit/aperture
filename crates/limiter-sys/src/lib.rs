use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int};
use std::sync::Arc;

#[repr(C)]
struct ApertureLimiter {
    _private: [u8; 0],
}

extern "C" {
    fn aperture_limiter_create(capacity: u64, refill_per_sec: c_double) -> *mut ApertureLimiter;
    fn aperture_limiter_destroy(limiter: *mut ApertureLimiter);
    fn aperture_limiter_allow(
        limiter: *mut ApertureLimiter,
        tenant_id: *const c_char,
        cost: u32,
    ) -> c_int;
    fn aperture_limiter_remaining(
        limiter: *mut ApertureLimiter,
        tenant_id: *const c_char,
    ) -> c_double;
}

pub struct Limiter {
    raw: *mut ApertureLimiter,
}

unsafe impl Send for Limiter {}
unsafe impl Sync for Limiter {}

impl Limiter {
    pub fn new(capacity: u64, refill_per_sec: f64) -> Result<Arc<Self>, String> {
        let raw = unsafe { aperture_limiter_create(capacity, refill_per_sec) };
        if raw.is_null() {
            return Err("aperture_limiter_create returned null".into());
        }
        Ok(Arc::new(Self { raw }))
    }

    pub fn allow(&self, tenant_id: &str, cost: u32) -> Result<bool, String> {
        let c = CString::new(tenant_id).map_err(|_| "tenant id contains NUL")?;
        let rc = unsafe { aperture_limiter_allow(self.raw, c.as_ptr(), cost) };
        match rc {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err("limiter error".into()),
        }
    }

    pub fn remaining(&self, tenant_id: &str) -> Result<f64, String> {
        let c = CString::new(tenant_id).map_err(|_| "tenant id contains NUL")?;
        let v = unsafe { aperture_limiter_remaining(self.raw, c.as_ptr()) };
        if v < 0.0 {
            return Err("limiter error".into());
        }
        Ok(v)
    }
}

impl Drop for Limiter {
    fn drop(&mut self) {
        unsafe { aperture_limiter_destroy(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenants_do_not_share_buckets() {
        let lim = Limiter::new(2, 0.0).unwrap();
        assert!(lim.allow("acme", 1).unwrap());
        assert!(lim.allow("acme", 1).unwrap());
        assert!(!lim.allow("acme", 1).unwrap());
        assert!(lim.allow("northwind", 1).unwrap());
    }
}
