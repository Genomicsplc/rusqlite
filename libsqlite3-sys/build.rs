// Copyright (c) 2014 John Gallagher <johnkgallagher@gmail.com>
// Copyright (c) 2019 Genomics plc
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use std::env;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("bindgen.rs");
    if cfg!(feature = "sqlcipher") {
        if cfg!(feature = "bundled") {
            println!(
                "cargo:warning={}",
                "Builds with bundled SQLCipher are not supported. Searching for SQLCipher to link against. \
                 This can lead to issues if your version of SQLCipher is not up to date!");
        }
        build_linked::main(&out_dir, &out_path)
    } else {
        // This can't be `cfg!` without always requiring our `mod build_bundled` (and thus `cc`)
        #[cfg(feature = "bundled")]
        {
            if cfg!(feature = "loadable_extension") {
                panic!("Building a loadable extension bundled is not supported");
            }
            build_bundled::main(&out_dir, &out_path)
        }
        #[cfg(not(feature = "bundled"))]
        {
            if cfg!(feature = "loadable_extension") {
                build_loadable_extension::main(&out_dir, &out_path)
            } else {
                build_linked::main(&out_dir, &out_path)
            }
        }
    }
}

#[cfg(feature = "bundled")]
mod build_bundled {
    use super::header_file;
    use cc;
    use std::path::Path;

    pub fn main(out_dir: &str, out_path: &Path) {
        if cfg!(feature = "sqlcipher") {
            // This is just a sanity check, the top level `main` should ensure this.
            panic!("Builds with bundled SQLCipher are not supported");
        }

        #[cfg(feature = "buildtime_bindgen")]
        {
            use super::{bindings, HeaderLocation};
            let header = HeaderLocation::FromPath(format!("sqlite3/{}", header_file()).to_owned());
            bindings::write_to_out_dir(header, out_path);
        }
        #[cfg(not(feature = "buildtime_bindgen"))]
        {
            use std::fs;
            fs::copy("sqlite3/bindgen_bundled_version.rs", out_path)
                .expect("Could not copy bindings to output directory");
        }

        let mut cfg = cc::Build::new();
        cfg.file("sqlite3/sqlite3.c")
            .flag("-DSQLITE_CORE")
            .flag("-DSQLITE_DEFAULT_FOREIGN_KEYS=1")
            .flag("-DSQLITE_ENABLE_API_ARMOR")
            .flag("-DSQLITE_ENABLE_COLUMN_METADATA")
            .flag("-DSQLITE_ENABLE_DBSTAT_VTAB")
            .flag("-DSQLITE_ENABLE_FTS3")
            .flag("-DSQLITE_ENABLE_FTS3_PARENTHESIS")
            .flag("-DSQLITE_ENABLE_FTS5")
            .flag("-DSQLITE_ENABLE_JSON1")
            .flag("-DSQLITE_ENABLE_LOAD_EXTENSION=1")
            .flag("-DSQLITE_ENABLE_MEMORY_MANAGEMENT")
            .flag("-DSQLITE_ENABLE_RTREE")
            .flag("-DSQLITE_ENABLE_STAT2")
            .flag("-DSQLITE_ENABLE_STAT4")
            .flag("-DSQLITE_HAVE_ISNAN")
            .flag("-DSQLITE_SOUNDEX")
            .flag("-DSQLITE_THREADSAFE=1")
            .flag("-DSQLITE_USE_URI")
            .flag("-DHAVE_USLEEP=1");
        if cfg!(feature = "unlock_notify") {
            cfg.flag("-DSQLITE_ENABLE_UNLOCK_NOTIFY");
        }
        if cfg!(feature = "preupdate_hook") {
            cfg.flag("-DSQLITE_ENABLE_PREUPDATE_HOOK");
        }
        if cfg!(feature = "session") {
            cfg.flag("-DSQLITE_ENABLE_SESSION");
        }
        cfg.compile("libsqlite3.a");

        println!("cargo:lib_dir={}", out_dir);
    }
}

fn env_prefix() -> &'static str {
    if cfg!(feature = "sqlcipher") {
        "SQLCIPHER"
    } else {
        "SQLITE3"
    }
}

fn header_file() -> &'static str {
    if cfg!(feature = "loadable_extension") {
        "sqlite3ext.h"
    } else {
        "sqlite3.h"
    }
}

fn wrapper_file() -> &'static str {
    if cfg!(feature = "loadable_extension") {
        "wrapper-ext.h"
    } else {
        "wrapper.h"
    }
}

pub enum HeaderLocation {
    FromEnvironment,
    Wrapper,
    FromPath(String),
}

impl From<HeaderLocation> for String {
    fn from(header: HeaderLocation) -> String {
        match header {
            HeaderLocation::FromEnvironment => {
                let prefix = env_prefix();
                let mut header = env::var(format!("{}_INCLUDE_DIR", prefix)).expect(&format!(
                    "{}_INCLUDE_DIR must be set if {}_LIB_DIR is set",
                    prefix, prefix
                ));
                header.push_str("/");
                header.push_str(header_file());
                header
            }
            HeaderLocation::Wrapper => wrapper_file().into(),
            HeaderLocation::FromPath(path) => path,
        }
    }
}

mod build_linked {
    use pkg_config;

    #[cfg(all(feature = "vcpkg", target_env = "msvc"))]
    extern crate vcpkg;

    use super::{bindings, env_prefix, header_file, HeaderLocation};
    use std::env;
    use std::path::Path;

    pub fn main(_out_dir: &str, out_path: &Path) {
        let header = find_sqlite();
        if cfg!(feature = "bundled") && !cfg!(feature = "buildtime_bindgen") {
            // We can only get here if `bundled` and `sqlcipher` were both
            // specified (and `builtime_bindgen` was not). In order to keep
            // `rusqlite` relatively clean we hide the fact that `bundled` can
            // be ignored in some cases, and just use the bundled bindings, even
            // though the library we found might not match their version.
            // Ideally we'd perform a version check here, but doing so is rather
            // tricky, since we might not have access to executables (and
            // moreover, we might be cross compiling).
            std::fs::copy("sqlite3/bindgen_bundled_version.rs", out_path)
                .expect("Could not copy bindings to output directory");
        } else {
            bindings::write_to_out_dir(header, out_path);
        }
    }

    fn find_link_mode() -> &'static str {
        // If the user specifies SQLITE_STATIC (or SQLCIPHER_STATIC), do static
        // linking, unless it's explicitly set to 0.
        match &env::var(format!("{}_STATIC", env_prefix())) {
            Ok(v) if v != "0" => "static",
            _ => "dylib",
        }
    }
    // Prints the necessary cargo link commands and returns the path to the header.
    fn find_sqlite() -> HeaderLocation {
        let link_lib = link_lib();

        println!("cargo:rerun-if-env-changed={}_INCLUDE_DIR", env_prefix());
        println!("cargo:rerun-if-env-changed={}_LIB_DIR", env_prefix());
        println!("cargo:rerun-if-env-changed={}_STATIC", env_prefix());
        if cfg!(target_os = "windows") {
            println!("cargo:rerun-if-env-changed=PATH");
        }
        if cfg!(all(feature = "vcpkg", target_env = "msvc")) {
            println!("cargo:rerun-if-env-changed=VCPKGRS_DYNAMIC");
        }
        // Allow users to specify where to find SQLite.
        if let Ok(dir) = env::var(format!("{}_LIB_DIR", env_prefix())) {
            // Try to use pkg-config to determine link commands
            let pkgconfig_path = Path::new(&dir).join("pkgconfig");
            env::set_var("PKG_CONFIG_PATH", pkgconfig_path);
            if let Err(_) = pkg_config::Config::new().probe(link_lib) {
                // Otherwise just emit the bare minimum link commands.
                println!("cargo:rustc-link-lib={}={}", find_link_mode(), link_lib);
                println!("cargo:rustc-link-search={}", dir);
            }
            return HeaderLocation::FromEnvironment;
        }

        if let Some(header) = try_vcpkg() {
            return header;
        }

        // See if pkg-config can do everything for us.
        match pkg_config::Config::new()
            .print_system_libs(false)
            .probe(link_lib)
        {
            Ok(mut lib) => {
                if let Some(mut header) = lib.include_paths.pop() {
                    header.push(header_file());
                    HeaderLocation::FromPath(header.to_string_lossy().into())
                } else {
                    HeaderLocation::Wrapper
                }
            }
            Err(_) => {
                // No env var set and pkg-config couldn't help; just output the link-lib
                // request and hope that the library exists on the system paths. We used to
                // output /usr/lib explicitly, but that can introduce other linking problems;
                // see https://github.com/jgallagher/rusqlite/issues/207.
                println!("cargo:rustc-link-lib={}={}", find_link_mode(), link_lib);
                HeaderLocation::Wrapper
            }
        }
    }

    #[cfg(all(feature = "vcpkg", target_env = "msvc"))]
    fn try_vcpkg() -> Option<HeaderLocation> {
        // See if vcpkg can find it.
        if let Ok(mut lib) = vcpkg::Config::new().probe(link_lib()) {
            if let Some(mut header) = lib.include_paths.pop() {
                header.push(header_file());
                return Some(HeaderLocation::FromPath(header.to_string_lossy().into()));
            }
        }
        None
    }

    #[cfg(not(all(feature = "vcpkg", target_env = "msvc")))]
    fn try_vcpkg() -> Option<HeaderLocation> {
        None
    }

    fn link_lib() -> &'static str {
        if cfg!(feature = "sqlcipher") {
            "sqlcipher"
        } else {
            "sqlite3"
        }
    }
}

mod build_loadable_extension {
    use pkg_config;

    use super::{bindings, env_prefix, header_file, HeaderLocation};
    use std::env;
    use std::path::Path;

    pub fn main(_out_dir: &str, out_path: &Path) {
        let header = find_sqlite();
        bindings::write_to_out_dir(header, out_path);
    }

    // Prints the necessary cargo link commands and returns the path to the header.
    fn find_sqlite() -> HeaderLocation {
        let link_lib = "sqlite3";

        println!("cargo:rerun-if-env-changed={}_INCLUDE_DIR", env_prefix());
        println!("cargo:rerun-if-env-changed={}_LIB_DIR", env_prefix());
        println!("cargo:rerun-if-env-changed={}_STATIC", env_prefix());
        if cfg!(target_os = "windows") {
            println!("cargo:rerun-if-env-changed=PATH");
        }
        if cfg!(all(feature = "vcpkg", target_env = "msvc")) {
            println!("cargo:rerun-if-env-changed=VCPKGRS_DYNAMIC");
        }
        // Allow users to specify where to find SQLite.
        if let Ok(dir) = env::var(format!("{}_LIB_DIR", env_prefix())) {
            // Try to use pkg-config to determine link commands
            let pkgconfig_path = Path::new(&dir).join("pkgconfig");
            env::set_var("PKG_CONFIG_PATH", pkgconfig_path);
            return HeaderLocation::FromEnvironment;
        }

        if let Some(header) = try_vcpkg() {
            return header;
        }

        // See if pkg-config can do everything for us.
        match pkg_config::Config::new()
            .print_system_libs(false)
            .probe(link_lib)
        {
            Ok(mut lib) => {
                if let Some(mut header) = lib.include_paths.pop() {
                    header.push(header_file());
                    HeaderLocation::FromPath(header.to_string_lossy().into())
                } else {
                    HeaderLocation::Wrapper
                }
            }
            Err(_) => HeaderLocation::Wrapper,
        }
    }

    #[cfg(all(feature = "vcpkg", target_env = "msvc"))]
    fn try_vcpkg() -> Option<HeaderLocation> {
        // See if vcpkg can find it.
        if let Ok(mut lib) = vcpkg::Config::new().probe(link_lib()) {
            if let Some(mut header) = lib.include_paths.pop() {
                header.push(header_file());
                return Some(HeaderLocation::FromPath(header.to_string_lossy().into()));
            }
        }
        None
    }

    #[cfg(not(all(feature = "vcpkg", target_env = "msvc")))]
    fn try_vcpkg() -> Option<HeaderLocation> {
        None
    }

}

#[cfg(not(feature = "buildtime_bindgen"))]
mod bindings {
    use super::HeaderLocation;

    use std::fs;
    use std::path::Path;

    static PREBUILT_BINDGEN_PATHS: &'static [&'static str] = &[
        "bindgen-bindings/bindgen_3.6.8",
        #[cfg(feature = "min_sqlite_version_3_6_23")]
        "bindgen-bindings/bindgen_3.6.23",
        #[cfg(feature = "min_sqlite_version_3_7_7")]
        "bindgen-bindings/bindgen_3.7.7",
        #[cfg(feature = "min_sqlite_version_3_7_16")]
        "bindgen-bindings/bindgen_3.7.16",
        #[cfg(feature = "min_sqlite_version_3_13_0")]
        "bindgen-bindings/bindgen_3.13.0",
        #[cfg(feature = "min_sqlite_version_3_20_0")]
        "bindgen-bindings/bindgen_3.20.0",
        #[cfg(feature = "min_sqlite_version_3_26_0")]
        "bindgen-bindings/bindgen_3.26.0",
    ];

    pub fn write_to_out_dir(_header: HeaderLocation, out_path: &Path) {
        let in_path = format!(
            "{}{}.rs",
            PREBUILT_BINDGEN_PATHS[PREBUILT_BINDGEN_PATHS.len() - 1],
            prebuilt_bindgen_ext()
        );
        fs::copy(in_path.to_owned(), out_path).expect(&format!(
            "Could not copy bindings to output directory from {}",
            in_path
        ));
    }

    fn prebuilt_bindgen_ext() -> &'static str {
        if cfg!(feature = "loadable_extension") {
            "-ext"
        } else {
            ""
        }
    }

}

#[cfg(feature = "buildtime_bindgen")]
mod bindings {
    use bindgen;

    use self::bindgen::callbacks::{IntKind, ParseCallbacks};
    use super::HeaderLocation;

    use std::fs::OpenOptions;
    use std::io::copy;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    #[derive(Debug)]
    struct SqliteTypeChooser;

    impl ParseCallbacks for SqliteTypeChooser {
        fn int_macro(&self, _name: &str, value: i64) -> Option<IntKind> {
            if value >= i32::min_value() as i64 && value <= i32::max_value() as i64 {
                Some(IntKind::I32)
            } else {
                None
            }
        }
    }

    pub fn write_to_out_dir(header: HeaderLocation, out_path: &Path) {
        let header: String = header.into();
        let mut output = Vec::new();
        let mut bindings = bindgen::builder()
            .header(header.clone())
            .parse_callbacks(Box::new(SqliteTypeChooser))
            .rustfmt_bindings(false); // we'll run rustfmt after (possibly) adding wrappers

        if cfg!(feature = "unlock_notify") {
            bindings = bindings.clang_arg("-DSQLITE_ENABLE_UNLOCK_NOTIFY");
        }
        if cfg!(feature = "preupdate_hook") {
            bindings = bindings.clang_arg("-DSQLITE_ENABLE_PREUPDATE_HOOK");
        }
        if cfg!(feature = "session") {
            bindings = bindings.clang_arg("-DSQLITE_ENABLE_SESSION");
        }

        // rust-bindgen does not handle CPP macros that alias functions, so
        // when using sqlite3ext.h to support loadable extensions, the macros
        // that attempt to redefine sqlite3 API routines to be redirected through
        // the global sqlite3_api instance of the sqlite3_api_routines structure
        // do not result in any code production.
        //
        // Before defining wrappers to take their place, we need to blacklist
        // all sqlite3 API functions since none of their symbols will be
        // available directly when being loaded as an extension.
        #[cfg(feature = "loadable_extension")]
        {
            // some api functions do not have an implementation in sqlite3_api_routines
            // (for example: sqlite3_config, sqlite3_initialize, sqlite3_interrupt, ...).
            // while this isn't a problem for shared libraries (unless we actually try to
            // call them, it is better to blacklist them all so that the build will fail
            // if an attempt is made to call an extern function that we know won't exist
            // and to avoid undefined symbol issues when linking the loadable extension
            // rust code with other (e.g. non-rust) code
            bindings = bindings.blacklist_function(".*");
        }

        bindings
            .generate()
            .expect(&format!("could not run bindgen on header {}", header))
            .write(Box::new(&mut output))
            .expect("could not write output of bindgen");
        let mut output = String::from_utf8(output).expect("bindgen output was not UTF-8?!");

        // Get the list of API functions supported by sqlite3_api_routines,
        // set the corresponding sqlite3 api routine to be blacklisted in the
        // final bindgen run, and add wrappers for each of the API functions to
        // dispatch the API call through a sqlite3_api global, which is also
        // declared in the bindings (either as a built-in or an extern symbol
        // in the case of loadable_extension_embedded (i.e. when the rust code
        // will be a part of an extension but not implement the extension
        // entrypoint itself).
        #[cfg(feature = "loadable_extension")]
        {
            let api_routines_struct_name = "sqlite3_api_routines".to_owned();

            let api_routines_struct = match get_struct_by_name(&output, &api_routines_struct_name) {
                Some(s) => s,
                None => {
                    panic!(
                        "Failed to find struct {} in early bindgen output",
                        api_routines_struct_name
                    );
                }
            };

            output.push_str(
                r#"

// a non-embedded loadable extension is a standalone rust loadable extension, 
// so we need our own sqlite3_api global
#[cfg(not(feature = "loadable_extension_embedded"))]
#[no_mangle]
pub static mut sqlite3_api: *mut sqlite3_api_routines = 0 as *mut sqlite3_api_routines;

// an embedded loadable extension is one in which the rust code will be linked in to 
// external code that implements the loadable extension and exports the sqlite3_api 
// interface as a symbol
#[cfg(feature = "loadable_extension_embedded")]
extern {
    #[no_mangle]
    pub static mut sqlite3_api: *mut sqlite3_api_routines;
}

// Wrappers to support loadable extensions (generated from build.rs - not by rust-bindgen)
"#,
            );

            // create wrapper for each field in api routines struct
            for field in &api_routines_struct.fields {
                let ident = match &field.ident {
                    Some(ident) => ident,
                    None => {
                        panic!("Unexpected anonymous field in sqlite");
                    }
                };
                let field_type = &field.ty;

                // construct global sqlite api function identifier from field identifier
                let api_fn_name = format!("sqlite3_{}", ident);

                // generate wrapper function and push it to output string
                let wrapper = generate_wrapper(ident, field_type, &api_fn_name);
                output.push_str(&wrapper);
            }

            output.push_str("\n");
        }

        // rusqlite's functions feature ors in the SQLITE_DETERMINISTIC flag when it
        // can. This flag was added in SQLite 3.8.3, but oring it in in prior
        // versions of SQLite is harmless. We don't want to not build just
        // because this flag is missing (e.g., if we're linking against
        // SQLite 3.7.x), so append the flag manually if it isn't present in bindgen's
        // output.
        if !output.contains("pub const SQLITE_DETERMINISTIC") {
            output.push_str("\npub const SQLITE_DETERMINISTIC: i32 = 2048;\n");
        }

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(out_path.clone())
            .expect(&format!("Could not write to {:?}", out_path));

        // pipe generated bindings through rustfmt
        let rustfmt = which::which("rustfmt")
            .expect("rustfmt not on PATH")
            .to_owned();
        let mut cmd = Command::new(rustfmt);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut rustfmt_child = cmd.spawn().expect("failed to execute rustfmt");
        let mut rustfmt_child_stdin = rustfmt_child.stdin.take().unwrap();
        let mut rustfmt_child_stdout = rustfmt_child.stdout.take().unwrap();

        // spawn a thread to write output string to rustfmt stdin
        let stdin_handle = ::std::thread::spawn(move || {
            let _ = rustfmt_child_stdin.write_all(output.as_bytes());
            output
        });

        // read stdout of rustfmt and write it to bindings file at out_path
        copy(&mut rustfmt_child_stdout, &mut file)
            .expect(&format!("Could not write to {:?}", out_path));

        let status = rustfmt_child
            .wait()
            .expect("failed to wait for rustfmt to complete");
        stdin_handle
            .join()
            .expect("The impossible: writer to rustfmt stdin cannot panic");

        match status.code() {
            Some(0) => {}
            Some(2) => {
                panic!("rustfmt parsing error");
            }
            Some(3) => {
                panic!("rustfmt could not format some lines.");
            }
            _ => {
                panic!("Internal rustfmt error");
            }
        }
    }

    #[cfg(feature = "loadable_extension")]
    fn get_struct_by_name(bindgen_sources: &str, name: &str) -> Option<syn::ItemStruct> {
        let file = syn::parse_file(&bindgen_sources).expect("unable to parse early bindgen output");

        for item in &file.items {
            if let syn::Item::Struct(s) = item {
                if s.ident == name {
                    return Some(s.to_owned());
                }
            }
        }
        return None;
    }

    #[cfg(feature = "loadable_extension")]
    fn bare_fn_from_type_path(t: &syn::Type) -> syn::TypeBareFn {
        let path = match t {
            syn::Type::Path(tp) => &tp.path,
            _ => {
                panic!("type was not a type path");
            }
        };

        let mut path_args: Option<syn::PathArguments> = None;
        for segment in &path.segments {
            if segment.arguments.is_empty() {
                continue;
            }
            path_args = Some(segment.arguments.to_owned());
            break;
        }
        match path_args {
            Some(syn::PathArguments::AngleBracketed(p)) => {
                for gen_arg in p.args {
                    match gen_arg {
                        syn::GenericArgument::Type(syn::Type::BareFn(bf)) => {
                            return bf;
                        }
                        _ => {
                            panic!("parsed type was not a bare function as expected");
                        }
                    };
                }
            }
            _ => {
                panic!("parsed path args were not angle bracketed as expected");
            }
        };
        panic!("unexpected failure to parse bare function");
    }

    #[cfg(feature = "loadable_extension")]
    fn generate_wrapper(
        field_ident: &syn::Ident,
        syn_type: &syn::Type,
        api_fn_name: &str,
    ) -> String {
        use quote::quote;
        use syn::Token;

        let field_name = field_ident.to_string();
        let api_fn_ident = syn::Ident::new(&api_fn_name, field_ident.span());

        // add wrapper macro invocation to be appended to the generated bindings
        let bare_fn = bare_fn_from_type_path(syn_type);
        let api_fn_output = &bare_fn.output;

        // prepare inputs
        let mut api_fn_inputs = bare_fn.inputs.clone();

        // handle variadic api functions
        if bare_fn.variadic.is_some() {
            // until rust c_variadic support exists, we can't
            // transparently wrap variadic api functions.
            // generate specific set of args in place of
            // variadic for each function we care about.
            let var_arg_types: Vec<Option<syn::Type>> = match api_fn_name.as_ref() {
                "sqlite3_db_config" => {
                    let mut_int_type: syn::TypeReference = syn::parse2(quote!(&mut i32))
                        .expect("failed to parse mutable integer reference");
                    vec![None, Some(syn::Type::Reference(mut_int_type))]
                }
                _ => vec![None],
            };

            for (index, var_arg_type) in var_arg_types.iter().enumerate() {
                let mut input = api_fn_inputs[api_fn_inputs.len() - 1].clone();
                let input_ident =
                    syn::Ident::new(&format!("vararg{}", index + 1), field_ident.span());
                let colon = Token![:](field_ident.span());
                input.name = Some((syn::BareFnArgName::Named(input_ident), colon));
                match var_arg_type.to_owned() {
                    Some(t) => {
                        input.ty = t;
                    }
                    None => {}
                };
                api_fn_inputs.push(input);
            }
        }

        // get identifiers for each of the inputs to use in the api call
        let api_fn_input_idents: Vec<syn::Ident> = (&api_fn_inputs)
            .into_iter()
            .map(|input| match &input.name {
                Some((syn::BareFnArgName::Named(ident), _)) => ident.to_owned(),
                _ => {
                    panic!("Input has no name {:#?}", input);
                }
            })
            .collect();

        // generate wrapper and return it as a string
        let wrapper_tokens = quote! {
            pub unsafe fn #api_fn_ident(#api_fn_inputs) #api_fn_output {
                if sqlite3_api.is_null() {
                    panic!("sqlite3_api is null");
                }
                ((*sqlite3_api).#field_ident
                    .expect(stringify!("sqlite3_api contains null pointer for ", #field_name, " function")))(
                        #(#api_fn_input_idents),*
                )
            }
        };
        return format!("{}\n\n", wrapper_tokens.to_string());
    }
}
