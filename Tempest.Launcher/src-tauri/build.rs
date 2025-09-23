use std::env;
use std::fs;
use std::process::Command;

fn main() {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    if profile != "release" {
        // In dev builds we still want a placeholder sidecar so that Tauri's
        // sidecar resolution (and any runtime path assumptions) don't fail.
        println!("cargo:warning=Dev profile detected - ensuring dummy sidecar binary exists");

        let current_dir = env::current_dir().expect("Failed to get current directory");
        let binaries_dir = current_dir.join("binaries");
        if let Err(e) = fs::create_dir_all(&binaries_dir) {
            panic!("Failed to create binaries directory: {e}");
        }

        // Reuse the same target triple logic as release builds so the name matches.
        let target_triple = env::var("TARGET").unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                "x86_64-pc-windows-msvc".to_string()
            } else if cfg!(target_os = "macos") {
                "x86_64-apple-darwin".to_string()
            } else {
                "x86_64-unknown-linux-gnu".to_string()
            }
        });

        let exe_extension = if target_triple.contains("windows") { ".exe" } else { "" };
        let sidecar_name = format!("tempest-cli-{}{}", target_triple, exe_extension);
        let sidecar_path = binaries_dir.join(&sidecar_name);

        if !sidecar_path.exists() {
            // Create a zero-length file. Make it executable on Unix so any spawn attempts won't immediately fail due to permission.
            if let Err(e) = fs::File::create(&sidecar_path) {
                panic!("Failed to create dummy sidecar file: {e}");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&sidecar_path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&sidecar_path, perms);
                }
            }
            println!("cargo:warning=Created dummy sidecar binary: {}", sidecar_name);
        } else {
            println!("cargo:warning=Dummy sidecar already present: {}", sidecar_name);
        }

        return tauri_build::build();
    }
    
    println!("cargo:warning=Building .NET project for Tauri sidecar");
    
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let dotnet_project_path = current_dir.join("../../Tempest.CLI");
    
    if !dotnet_project_path.exists() {
        panic!("Tempest.CLI project directory not found");
    }
    
    let binaries_dir = current_dir.join("binaries");
    fs::create_dir_all(&binaries_dir).expect("Failed to create binaries directory");
    
    let target_triple = env::var("TARGET").unwrap_or_else(|_| {
        // Fallback to common targets
        if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc".to_string()
        } else if cfg!(target_os = "macos") {
            "x86_64-apple-darwin".to_string()
        } else {
            "x86_64-unknown-linux-gnu".to_string()
        }
    });
    
    // Tauri sidecar naming convention: binary-name-target-triple[.exe]
    let exe_extension = if target_triple.contains("windows") { ".exe" } else { "" };
    let sidecar_name = format!("tempest-cli-{}{}", target_triple, exe_extension);
    let sidecar_path = binaries_dir.join(&sidecar_name);
    
    // Publish .NET project to a temporary directory
    let temp_output = current_dir.join("temp-dotnet-build");
    
    let output = Command::new("dotnet")
        .arg("publish")
        .arg(&dotnet_project_path)
        .arg("--configuration")
        .arg("Release")
        .arg("--output")
        .arg(&temp_output)
        .arg("-v")
        .arg("q")
        .output()
        .expect("Failed to execute dotnet publish");
    
    if !output.status.success() {
        panic!("Failed to build .NET project: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    // Copy the .NET executable to the sidecar location
    let dotnet_exe_name = if target_triple.contains("windows") { "Tempest.CLI.exe" } else { "Tempest.CLI" };
    let dotnet_exe_path = temp_output.join(dotnet_exe_name);
    
    if dotnet_exe_path.exists() {
        fs::copy(&dotnet_exe_path, &sidecar_path)
            .expect("Failed to copy .NET executable to sidecar location");
        
        // Set executable permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&sidecar_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&sidecar_path, perms).unwrap();
        }
        
        println!("cargo:warning=Sidecar binary created: {}", sidecar_name);
    } else {
        panic!("Built .NET executable not found at: {}", dotnet_exe_path.display());
    }
    
    // Clean up temporary build directory
    let _ = fs::remove_dir_all(temp_output);
    
    println!("cargo:warning=.NET CLI built successfully");

    tauri_build::build()
}
