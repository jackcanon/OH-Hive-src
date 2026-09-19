//! Stable C entry point used by the launcher before loading generated Swift bindings.
//! The bundle builder supplies the source stamp to Cargo and the app's Info.plist.

/// Return a process-lifetime, NUL-terminated build stamp. No allocation or worker startup.
#[no_mangle]
pub extern "C" fn ohhive_core_source_commit() -> *const std::ffi::c_char {
    concat!(env!("OHHIVE_CORE_SOURCE_COMMIT"), "\0")
        .as_ptr()
        .cast()
}

#[cfg(test)]
mod tests {
    #[test]
    fn exported_stamp_matches_compiled_stamp() {
        let stamp = unsafe { std::ffi::CStr::from_ptr(super::ohhive_core_source_commit()) };
        assert_eq!(stamp.to_str().unwrap(), env!("OHHIVE_CORE_SOURCE_COMMIT"));
    }
}
