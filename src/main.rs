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

    let mut file = File::open(&file_path).expect("Unable to open the file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read the file");

    let package_file: PkgFile = toml::from_str(&contents).expect("Unable to parse the TOML file");
    println!("{:#?}", package_file);

    let build_dir = setup_build_environment(&package_file);

    // execute build script in build directory
    match package_file.build {
        Some(build) => {
            let output = Command::new("sh")
                .arg("-c")
                .arg(build.script)
                .current_dir(&build_dir)
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
}

fn setup_build_environment(pkgfile: &PkgFile) -> String {
    // get unix timestamp
    let timestamp = chrono::Utc::now().timestamp();

    // create build directory in /tmp
    let build_dir = format!(
        "/tmp/pkgbuilder/build_{}_{}_{}",
        pkgfile.package.name, pkgfile.package.version, timestamp
    );
    fs::create_dir_all(&build_dir).expect("Unable to create build directory");
    println!("Created build directory: {}", build_dir);

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

    return build_dir;
}
