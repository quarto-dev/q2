use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Represents the version of Lua to build.
#[derive(Debug, PartialEq, Eq)]
pub enum Version {
    Lua51,
    Lua52,
    Lua53,
    Lua54,
    Lua55,
}
pub use self::Version::*;

/// Represents the configuration for building Lua artifacts.
pub struct Build {
    out_dir: Option<PathBuf>,
    target: Option<String>,
    host: Option<String>,
    opt_level: Option<String>,
    debug: Option<bool>,
}

/// Represents the artifacts produced by the build process.
#[derive(Clone, Debug)]
pub struct Artifacts {
    include_dir: PathBuf,
    lib_dir: PathBuf,
    libs: Vec<String>,
}

impl Default for Build {
    fn default() -> Build {
        Build {
            out_dir: env::var_os("OUT_DIR").map(PathBuf::from),
            target: env::var("TARGET").ok(),
            host: None,
            opt_level: None,
            debug: None,
        }
    }
}

impl Build {
    /// Creates a new `Build` instance with default settings.
    pub fn new() -> Build {
        Build::default()
    }

    /// Sets the output directory for the build artifacts.
    ///
    /// This is required if called outside of a build script.
    pub fn out_dir<P: AsRef<Path>>(&mut self, path: P) -> &mut Build {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Sets the target architecture for the build.
    ///
    /// This is required if called outside of a build script.
    pub fn target(&mut self, target: &str) -> &mut Build {
        self.target = Some(target.to_string());
        self
    }

    /// Sets the host architecture for the build.
    ///
    /// This is optional and will default to the environment variable `HOST` if not set.
    /// If called outside of a build script, it will default to the target architecture.
    pub fn host(&mut self, host: &str) -> &mut Build {
        self.host = Some(host.to_string());
        self
    }

    /// Sets the optimization level for the build.
    ///
    /// This is optional and will default to the environment variable `OPT_LEVEL` if not set.
    /// If called outside of a build script, it will default to `0` in debug mode and `2` otherwise.
    pub fn opt_level(&mut self, opt_level: &str) -> &mut Build {
        self.opt_level = Some(opt_level.to_string());
        self
    }

    /// Sets whether to build in debug mode.
    ///
    /// This is optional and will default to the value of `cfg!(debug_assertions)`.
    /// If set to `true`, it also enables Lua API checks.
    pub fn debug(&mut self, debug: bool) -> &mut Build {
        self.debug = Some(debug);
        self
    }

    /// Builds the Lua artifacts for the specified version.
    pub fn build(&self, version: Version) -> Artifacts {
        match self.try_build(version) {
            Ok(artifacts) => artifacts,
            Err(err) => panic!("{err}"),
        }
    }

    /// Attempts to build the Lua artifacts for the specified version.
    ///
    /// Returns an error if the build fails.
    pub fn try_build(&self, version: Version) -> Result<Artifacts, Box<dyn Error>> {
        let target = self.target.as_ref().ok_or("TARGET is not set")?;
        let out_dir = self.out_dir.as_ref().ok_or("OUT_DIR is not set")?;
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut source_dir = manifest_dir.join(version.source_dir());
        let lib_dir = out_dir.join("lib");
        let include_dir = out_dir.join("include");

        if !include_dir.exists() {
            fs::create_dir_all(&include_dir)
                .context(|| format!("Cannot create '{}'", include_dir.display()))?;
        }

        let mut config = cc::Build::new();
        config.warnings(false).cargo_metadata(false).target(target);

        match &self.host {
            Some(host) => {
                config.host(host);
            }
            // Host will be taken from the environment variable
            None if env::var("HOST").is_ok() => {}
            None => {
                // If called outside of build script, set default host
                config.host(target);
            }
        }

        let mut libs = vec![version.lib_name().to_string()];
        match target {
            _ if target.contains("linux") => {
                config.define("LUA_USE_LINUX", None);
            }
            _ if target.ends_with("bsd") => {
                config.define("LUA_USE_LINUX", None);
            }
            _ if target.contains("apple-darwin") => {
                match version {
                    Lua51 => config.define("LUA_USE_LINUX", None),
                    _ => config.define("LUA_USE_MACOSX", None),
                };
            }
            _ if target.contains("apple-ios") => {
                match version {
                    Lua54 | Lua55 => config.define("LUA_USE_IOS", None),
                    _ => config.define("LUA_USE_POSIX", None),
                };
            }
            _ if target.contains("windows") => {
                // Defined in Lua >= 5.3
                config.define("LUA_USE_WINDOWS", None);
            }
            _ if target.ends_with("emscripten") => {
                config
                    .define("LUA_USE_POSIX", None)
                    .cpp(true)
                    .flag("-fexceptions")
                    .flag("-fwasm-exceptions"); // Enable exceptions to be caught

                let cpp_source_dir = out_dir.join("cpp_source");
                if cpp_source_dir.exists() {
                    fs::remove_dir_all(&cpp_source_dir)
                        .context(|| format!("Cannot remove '{}'", cpp_source_dir.display()))?;
                }
                fs::create_dir_all(&cpp_source_dir)
                    .context(|| format!("Cannot create '{}'", cpp_source_dir.display()))?;

                for file in fs::read_dir(&source_dir)
                    .context(|| format!("Cannot read '{}'", source_dir.display()))?
                {
                    let file = file?;
                    let filename = file.file_name();
                    let filename = &*filename.to_string_lossy();
                    let src_file = source_dir.join(file.file_name());
                    let dst_file = cpp_source_dir.join(file.file_name());

                    let mut content = fs::read(&src_file)
                        .context(|| format!("Cannot read '{}'", src_file.display()))?;
                    if ["lauxlib.h", "lua.h", "lualib.h"].contains(&filename) {
                        content.splice(0..0, b"extern \"C\" {\n".to_vec());
                        content.extend(b"\n}".to_vec())
                    }
                    fs::write(&dst_file, content)
                        .context(|| format!("Cannot write to '{}'", dst_file.display()))?;
                }
                source_dir = cpp_source_dir
            }
            _ if target.contains("wasi") => {
                // WASI is posix-like, but further patches are needed to the Lua
                // source to get it compiling.
                config.define("LUA_USE_POSIX", None);

                // Bring in just enough signal-handling support to get Lua at
                // least compiling, but WASI in general does not support
                // signals.
                config.define("_WASI_EMULATED_SIGNAL", None);
                libs.push("wasi-emulated-signal".to_string());

                // https://github.com/WebAssembly/wasi-sdk/blob/main/SetjmpLongjmp.md
                // for information about getting setjmp/longjmp working.
                config.flag("-mllvm").flag("-wasm-enable-eh");
                config.flag("-mllvm").flag("-wasm-use-legacy-eh=false");
                config.flag("-mllvm").flag("-wasm-enable-sjlj");
                libs.push("setjmp".to_string());
            }
            _ if target == "wasm32-unknown-unknown" => {
                // Override Lua's setjmp/longjmp error handling with Rust's
                // panic/catch_unwind mechanism via a force-included header.
                let wasm_header = source_dir.join("luaconf_wasm.h");
                config.flag(&format!("-include{}", wasm_header.display()));

                // Enable WASM exception handling for unwind-through-C-frames
                config.flag("-fwasm-exceptions");
            }
            _ => Err(format!("don't know how to build Lua for {target}"))?,
        }

        if let Lua54 = version {
            config.define("LUA_COMPAT_5_3", None);
        }

        #[cfg(feature = "ucid")]
        if let Lua54 | Lua55 = version {
            config.define("LUA_UCID", None);
        }

        let debug = self.debug.unwrap_or(cfg!(debug_assertions));
        if debug {
            config.define("LUA_USE_APICHECK", None);
            config.debug(true);
        }

        match &self.opt_level {
            Some(opt_level) => {
                config.opt_level_str(opt_level);
            }
            // Opt level will be taken from the environment variable
            None if env::var("OPT_LEVEL").is_ok() => {}
            None => {
                // If called outside of build script, set default opt level
                config.opt_level(if debug { 0 } else { 2 });
            }
        }

        // Files to exclude from compilation for wasm32-unknown-unknown
        // (io, os, and dynamic loading libraries require system calls)
        let exclude_files: &[&str] = if target == "wasm32-unknown-unknown" {
            &["liolib.c", "loslib.c", "loadlib.c"]
        } else {
            &[]
        };

        config
            .include(&source_dir)
            .warnings(false) // Suppress all warnings
            .flag_if_supported("-fno-common") // Compile common globals like normal definitions
            .add_files_by_ext_excluding(&source_dir, "c", exclude_files)?
            .out_dir(&lib_dir)
            .try_compile(version.lib_name())?;

        for f in &["lauxlib.h", "lua.h", "luaconf.h", "lualib.h"] {
            let from = source_dir.join(f);
            let to = include_dir.join(f);
            fs::copy(&from, &to)
                .context(|| format!("Cannot copy '{}' to '{}'", from.display(), to.display()))?;
        }

        Ok(Artifacts {
            include_dir,
            lib_dir,
            libs,
        })
    }
}

impl Version {
    fn source_dir(&self) -> &'static str {
        match self {
            Lua51 => "lua-5.1.5",
            Lua52 => "lua-5.2.4",
            Lua53 => "lua-5.3.6",
            Lua54 => "lua-5.4.8",
            Lua55 => "lua-5.5.0",
        }
    }

    fn lib_name(&self) -> &'static str {
        match self {
            Lua51 => "lua5.1",
            Lua52 => "lua5.2",
            Lua53 => "lua5.3",
            Lua54 => "lua5.4",
            Lua55 => "lua5.5",
        }
    }
}

impl Artifacts {
    /// Returns the directory containing the Lua headers.
    pub fn include_dir(&self) -> &Path {
        &self.include_dir
    }

    /// Returns the directory containing the Lua libraries.
    pub fn lib_dir(&self) -> &Path {
        &self.lib_dir
    }

    /// Returns the names of the Lua libraries built.
    pub fn libs(&self) -> &[String] {
        &self.libs
    }

    /// Prints the necessary Cargo metadata for linking the Lua libraries.
    ///
    /// This method is typically called in a build script to inform Cargo
    /// about the location of the Lua libraries and how to link them.
    pub fn print_cargo_metadata(&self) {
        println!("cargo:rustc-link-search=native={}", self.lib_dir.display());
        for lib in self.libs.iter() {
            println!("cargo:rustc-link-lib=static:-bundle={lib}");
        }
    }
}

trait ErrorContext<T> {
    fn context(self, f: impl FnOnce() -> String) -> Result<T, Box<dyn Error>>;
}

impl<T, E: Error> ErrorContext<T> for Result<T, E> {
    fn context(self, f: impl FnOnce() -> String) -> Result<T, Box<dyn Error>> {
        self.map_err(|e| format!("{}: {e}", f()).into())
    }
}

trait AddFilesByExt {
    fn add_files_by_ext_excluding(
        &mut self,
        dir: &Path,
        ext: &str,
        exclude: &[&str],
    ) -> Result<&mut Self, Box<dyn Error>>;
}

impl AddFilesByExt for cc::Build {
    fn add_files_by_ext_excluding(
        &mut self,
        dir: &Path,
        ext: &str,
        exclude: &[&str],
    ) -> Result<&mut Self, Box<dyn Error>> {
        for entry in fs::read_dir(dir)
            .context(|| format!("Cannot read '{}'", dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension() == Some(ext.as_ref()))
        {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();
            if !exclude.contains(&&*filename_str) {
                self.file(entry.path());
            }
        }
        Ok(self)
    }
}
