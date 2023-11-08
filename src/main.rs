use serde::Deserialize;
use std::{env, fs, fs::File, io::Read, process::Command};
use toml;

#[derive(Debug, Deserialize)]
struct PkgFile {
    package: PkgFilePackage,
    source: Option<Vec<PkgFileSource>>,
    build: Option<PkgFileBuild>,
}

#[derive(Debug, Deserialize)]
struct PkgFilePackage {
    name: String,
    version: String,
    description: String,
    license: String,
}

#[derive(Debug, Deserialize)]
struct PkgFileSource {
    source: String,
    // default is root of the build directory
    destination: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PkgFileBuild {
    script: String,
}

fn main() {
    let file_path = env::args().nth(1).unwrap_or_else(|| {
        env::var("PKGBUILDER_PKGFILE_PATH").unwrap_or_else(|_| panic!("No file path provided"))
    });

    let output_path = env::args().nth(2).unwrap_or_else(|| {
        env::var("PKGBUILDER_OUTPUT_PATH").unwrap_or_else(|_| panic!("No output path provided"))
    });

    let mut file = File::open(&file_path).expect("Unable to open the file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read the file");

    let package_file: PkgFile = toml::from_str(&contents).expect("Unable to parse the TOML file");
    println!("{:#?}", package_file);

    let (build_dir, out_dir) = setup_build_environment(&package_file);

    // execute build script in build directory
    match package_file.build {
        Some(build) => {
            let output = Command::new("sh")
                .arg("-c")
                .arg(build.script)
                .current_dir(&build_dir)
                .env("OUT", &out_dir)
                .output()
                .expect("Failed to execute command");

            if !output.status.success() {
                eprintln!(
                    "Build script failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        None => println!("No build script to execute"),
    }

    // copy package file to out directory as package.toml
    let package_file_path = format!("{}/package.toml", &out_dir);
    fs::copy(&file_path, &package_file_path).expect("Unable to copy package file");

    // tar out directory
    let tar_file_path = format!("{}/{}.tar.gz", &out_dir, &package_file.package.name);
    let output = Command::new("tar")
        .arg("-czvf")
        .arg(&tar_file_path)
        .arg("./")
        .current_dir(&out_dir)
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        eprintln!(
            "Compression failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // copy tar file to output path
    fs::copy(&tar_file_path, &output_path).expect("Unable to copy tar file");

    // remove build directory
    fs::remove_dir_all(&build_dir).expect("Unable to remove build directory");
    println!("Removed build directory: {}", build_dir);

    // remove out directory
    fs::remove_dir_all(&out_dir).expect("Unable to remove out directory");
    println!("Removed out directory: {}", out_dir);

    println!("Package built successfully");
}

fn setup_build_environment(pkgfile: &PkgFile) -> (String, String) {
    // get unix timestamp
    let timestamp = chrono::Utc::now().timestamp();

    // create build directory in /tmp
    let build_dir = format!(
        "/tmp/pkgbuilder/build_{}_{}_{}",
        pkgfile.package.name, pkgfile.package.version, timestamp
    );
    fs::create_dir_all(&build_dir).expect("Unable to create build directory");
    println!("Created build directory: {}", build_dir);

    // create out directory in /tmp
    let out_dir = format!(
        "/tmp/pkgbuilder/build_{}_{}_{}_out",
        pkgfile.package.name, pkgfile.package.version, timestamp
    );
    fs::create_dir_all(&out_dir).expect("Unable to create out directory");
    println!("Created out directory: {}", out_dir);

    match pkgfile.source {
        Some(ref sources) => {
            for source in sources {
                let source_url = &source.source;
                let destination = match source.destination {
                    Some(ref destination) => format!("{}{}", build_dir.clone(), destination),
                    None => build_dir.clone(),
                };

                println!("Cloning {} into {}", source_url, &destination);

                // Yes, we can use the git2 crate, but that increases build time, bundle size and complexity

                let output = Command::new("git")
                    .arg("clone")
                    .arg(source_url)
                    .arg(&destination)
                    .output()
                    .expect("Failed to execute command");

                if !output.status.success() {
                    eprintln!(
                        "Git clone failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
        None => println!("No sources to clone"),
    }

    println!("Build environment setup successfully");

    return (build_dir, out_dir);
}
