// Internal utilities
pub(crate) mod param_cache;
mod small_cstr;
pub(crate) use param_cache::ParamIndexCache;
pub(crate) use small_cstr::SmallCString;

// Doesn't use any modern features or vtab stuff, but is only used by them.
#[cfg(any(
    feature = "loadable_extension",
    feature = "loadable_extension_embedded",
    feature = "modern_sqlite",
    feature = "vtab",
))]
mod sqlite_string;
#[cfg(any(
    feature = "loadable_extension",
    feature = "loadable_extension_embedded",
    feature = "modern_sqlite",
    feature = "vtab",
))]
pub(crate) use sqlite_string::{SqliteMallocString};

#[cfg(any(
    feature = "loadable_extension",
    feature = "loadable_extension_embedded",
    feature = "modern_sqlite",
    feature = "vtab",
))]
pub(crate) fn alloc(s: &str) -> *mut std::os::raw::c_char {
    SqliteMallocString::from_str(s).into_raw()
}
