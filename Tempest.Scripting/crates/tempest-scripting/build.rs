//! Compiles the native UI layer: Dear ImGui + cimgui (every target) and the
//! Win32/DX9/DX11 backends with the overlay glue (Windows targets). See
//! native/cimgui/VENDORED.txt for the vendored revision.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let windows = target.contains("windows");
    let gnu = target.contains("gnu") || target.contains("apple") || target.contains("linux");
    let native = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("native");
    println!("cargo:rerun-if-changed={}", native.display());

    let mut b = cc::Build::new();
    b.cpp(true)
        .include(&native)
        .include(native.join("cimgui"))
        .include(native.join("cimgui/imgui"))
        .define("IMGUI_USER_CONFIG", "\"tempest_imconfig.h\"")
        .define("CIMGUI_NO_EXPORT", None)
        .define("IMGUI_IMPL_WIN32_DISABLE_GAMEPAD", None)
        .warnings(false)
        .file(native.join("cimgui/cimgui.cpp"))
        .file(native.join("overlay.cpp"));
    for f in ["imgui.cpp", "imgui_draw.cpp", "imgui_tables.cpp", "imgui_widgets.cpp", "imgui_demo.cpp"] {
        b.file(native.join("cimgui/imgui").join(f));
    }
    if windows {
        for f in ["imgui_impl_win32.cpp", "imgui_impl_dx9.cpp", "imgui_impl_dx11.cpp"] {
            b.file(native.join("cimgui/imgui/backends").join(f));
        }
    }
    if gnu {
        // ImGui needs neither exceptions nor RTTI; dropping them keeps the
        // static C++ runtime footprint small.
        b.flag("-fno-exceptions").flag("-fno-rtti").flag("-fno-threadsafe-statics").flag("-std=c++17");
    } else {
        b.flag("/EHs-c-").flag("/GR-").flag("/std:c++17");
    }
    if target.ends_with("-pc-windows-gnu") {
        // Link libstdc++ statically: the game folder has no libstdc++-6.dll.
        // cc would add a plain `-lstdc++`, which the gcc driver resolves to the
        // import library; point at the archive instead.
        b.cpp_link_stdlib(None);
        let compiler = b.get_compiler();
        let out = Command::new(compiler.path()).arg("-print-file-name=libstdc++.a").output().expect("query libstdc++.a");
        let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if let Some(dir) = path.parent().filter(|_| path.is_file()) {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=stdc++");
        } else {
            println!("cargo:warning=libstdc++.a not found via {}; linking libstdc++ dynamically", compiler.path().display());
            println!("cargo:rustc-link-lib=stdc++");
        }
    }
    b.compile("tempest_overlay");

    if windows {
        for lib in ["d3dcompiler_47", "dwmapi", "gdi32", "user32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
