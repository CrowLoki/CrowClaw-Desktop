#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, ptr};
    // No CRYPTPROTECT_LOCAL_MACHINE: never grant every Windows user access.
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 1;
    #[repr(C)]
    struct Blob {
        length: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    pub fn transform(bytes: &[u8], entropy: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
        if bytes.is_empty() || bytes.len() > 2 * 1024 * 1024 || entropy.len() > 8192 {
            return Err("Protected credential input exceeds its bounds".into());
        }
        let input = Blob {
            length: bytes.len() as u32,
            data: bytes.as_ptr() as *mut u8,
        };
        let entropy = Blob {
            length: entropy.len() as u32,
            data: entropy.as_ptr() as *mut u8,
        };
        let mut output = Blob {
            length: 0,
            data: ptr::null_mut(),
        };
        // Both input slices remain alive for this synchronous call. Blob uses
        // the documented DATA_BLOB ABI; Windows allocates the output separately.
        let result = unsafe {
            if decrypt {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &entropy,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if result == 0 {
            return Err(format!(
                "Windows {} of this registration's credentials failed (code {})",
                if decrypt { "decryption" } else { "protection" },
                std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
            ));
        }
        if output.data.is_null() || output.length == 0 || output.length as usize > 2 * 1024 * 1024 {
            if !output.data.is_null() {
                unsafe {
                    LocalFree(output.data as *mut c_void);
                }
            }
            return Err("Windows returned an invalid protected record".into());
        }
        // Copy before releasing the DPAPI allocation with its required allocator.
        let value =
            unsafe { std::slice::from_raw_parts(output.data, output.length as usize) }.to_vec();
        unsafe {
            LocalFree(output.data as *mut c_void);
        }
        Ok(value)
    }
}
pub fn protect(bytes: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        windows::transform(bytes, entropy, false)
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, entropy);
        Err("This desktop build requires its platform credential protector".into())
    }
}
pub fn unprotect(bytes: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        windows::transform(bytes, entropy, true)
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, entropy);
        Err("This desktop build requires its platform credential protector".into())
    }
}
