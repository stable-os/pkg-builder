use serde::Deserialize;
use std::{
    env, fs,
    fs::File,
    io::{self, Read},
    process::{Command, Stdio},
};
use toml;

#[derive(Debug, Deserialize)]
struct PkgFile {
    package: PkgFilePackage,
    subpackage: Option<Vec<PkgFileSubPackage>>,
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
struct PkgFileSubPackage {
    name: String,
    description: String,
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PkgFileSource {
    source: String,
    git_ref: Option<String>,
    git_commit: Option<String>,
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

    let (build_dir, out_dir, package_dir) = setup_build_environment(&package_file);

    // execute build script in build directory
    match package_file.build {
        Some(build) => {
            let mut child = Command::new("bash")
                .arg("-c")
                .arg(format!("source /root/.bashrc\n\n{}", build.script))
                .current_dir(&build_dir)
                .env("OUT", &out_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to execute command");

            let mut stdout = child.stdout.take().expect("Failed to capture stdout");
            let mut stderr = child.stderr.take().expect("Failed to capture stderr");

            std::thread::spawn(move || {
                io::copy(&mut stdout, &mut io::stdout()).expect("Failed to copy stdout");
            });

            std::thread::spawn(move || {
                io::copy(&mut stderr, &mut io::stderr()).expect("Failed to copy stderr");
            });

            let output = child.wait().expect("Failed to wait on child");

            if !output.success() {
                eprintln!("Build script failed");
                panic!("Build script failed");
            }
        }
        None => println!("No build script to execute"),
    }

    println!("Build script executed successfully, packaging...");

    // create final output directory
    fs::create_dir_all(&output_path).expect("Unable to create output directory");

    if let Some(subpackages) = package_file.subpackage {
        for subpackage in subpackages {
            println!("Handling subpackage: {:#?}", subpackage);

            // create a seperate direcotry for subpackage
            let subpackage_dir = format!("{}/{}", &package_dir, subpackage.name);
            fs::create_dir_all(&subpackage_dir).expect("Unable to create subpackage directory");

            // move files to subpackage directory
            // files in a subpackage shouldn't be in the main package
            for file_selector in subpackage.files {
                // the file_selector is a relative glob pattern
                // so it must be expanded to get the actual file paths
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(format!(
                        "shopt -s nullglob; shopt -s dotglob; echo {}{}",
                        out_dir, file_selector
                    ))
                    .current_dir(&build_dir)
                    .output()
                    .expect("Failed to execute command");

                if !output.status.success() {
                    eprintln!(
                        "Failed to expand file selector: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    continue;
                }

                let files = String::from_utf8_lossy(&output.stdout);
                let files = files.split_whitespace().collect::<Vec<&str>>();

                for file in files {
                    // remove the out directory from the file path
                    let file = file.replace(&out_dir, "");

                    // create the directory structure in the subpackage directory
                    let file_dir = file.rsplitn(2, '/').last().unwrap();
                    let file_dir = format!("{}/{}", &subpackage_dir, file_dir);
                    fs::create_dir_all(&file_dir).expect("Unable to create file directory");

                    println!("Moving file: {}", file);
                    Command::new("mv")
                        .arg(format!("{}{}", out_dir, file))
                        .arg(format!("{}{}", &subpackage_dir, file))
                        .output()
                        .expect("Failed to move files to subpackage directory");
                }
            }

            println!("Moved files to subpackage directory: {}", subpackage_dir);

            // Copy package file to subpackage directory
            fs::copy(&file_path, &format!("{}/package.toml", subpackage_dir))
                .expect("Unable to copy package file to subpackage directory");

            // Create a tarball of the subpackage directory
            let tarball_name = format!("{}/{}.tar.gz", &output_path, subpackage.name);
            let output = Command::new("tar")
                .arg("-czf")
                .arg(&tarball_name)
                .arg("./")
                .current_dir(&subpackage_dir)
                .output()
                .expect("Failed to create tarball");

            if !output.status.success() {
                eprintln!(
                    "Failed to create tarball: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                continue;
            }

            println!("Created tarball for subpackage: {}", tarball_name);

            // Remove subpackage directory
            fs::remove_dir_all(&subpackage_dir).expect("Unable to remove subpackage directory");

            // Copy tarball to final output directory
            fs::copy(&tarball_name, &format!("{}/{}", &output_path, tarball_name))
                .expect("Unable to copy tarball to output directory");
        }
    }

    // Move the remaining files from the out directory to the package directory
    // in a subfolder named after the package name
    Command::new("mv")
        .arg(&out_dir)
        .arg(&format!("{}/{}", package_dir, package_file.package.name))
        .output()
        .expect("Failed to move files from out directory to package directory");

    // remove build directory
    fs::remove_dir_all(&build_dir).expect("Unable to remove build directory");
    println!("Removed build directory: {}", build_dir);

    // remove package directory
    fs::remove_dir_all(&package_dir).expect("Unable to remove package directory");
    println!("Removed package directory: {}", package_dir);

    // Out directory got moved into package directory, does not have to be deleted

    println!("Package built successfully");
}

fn setup_build_environment(pkgfile: &PkgFile) -> (String, String, String) {
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

    // create package directory in /tmp
    let package_dir = format!(
        "/tmp/pkgbuilder/build_{}_{}_{}_package",
        pkgfile.package.name, pkgfile.package.version, timestamp
    );
    fs::create_dir_all(&package_dir).expect("Unable to create package directory");
    println!("Created package directory: {}", package_dir);

    match pkgfile.source {
        Some(ref sources) => {
            for source in sources {
                let source_url = &source.source;
                let destination = match source.destination {
                    Some(ref destination) => format!("{}{}", build_dir.clone(), destination),
                    None => build_dir.clone(),
                };

                if source_url.ends_with(".git") {
                    println!("Cloning {} into {}", source_url, &destination);

                    let output = Command::new("git")
                        .arg("clone")
                        // don't copy all the history
                        .arg("--depth")
                        .arg("1")
                        // if a git_ref is specified, add the --branch flag
                        .args(match source.git_ref {
                            Some(ref git_ref) => vec!["--branch", git_ref],
                            None => vec![],
                        })
                        .arg(source_url)
                        .arg(destination.clone())
                        .output()
                        .expect("Failed to execute command");

                    if !output.status.success() {
                        eprintln!(
                            "Git clone failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }

                    // run git reset --hard if a git_commit is specified
                    if let Some(ref git_commit) = source.git_commit {
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(git_commit)
                            .current_dir(&destination)
                            .output()
                            .expect("Failed to execute command");

                        if !output.status.success() {
                            eprintln!(
                                "Git reset failed: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                    }
                }

                if source_url.ends_with(".tar.gz")
                    || source_url.ends_with(".tgz")
                    || source_url.ends_with(".tar.bz2")
                    || source_url.ends_with(".tar.xz")
                {
                    println!("Downloading {} into {}", source_url, &destination);

                    let output = Command::new("curl")
                        .arg("-L")
                        .arg(source_url)
                        .arg("-o")
                        .arg(format!("{}.tmpdownload", &destination))
                        .output()
                        .expect("Failed to execute command");

                    if !output.status.success() {
                        eprintln!(
                            "Download failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }

                    println!("Extracting {} into {}", source_url, &destination);

                    let output = Command::new("tar")
                        .arg("-xvf")
                        .arg(format!("{}.tmpdownload", &destination))
                        .arg("-C")
                        .arg(&destination)
                        .output()
                        .expect("Failed to execute command");

                    if !output.status.success() {
                        eprintln!(
                            "Extraction failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }

                if source_url.ends_with(".zip") {
                    println!("Downloading {} into {}", source_url, &destination);

                    let output = Command::new("curl")
                        .arg("-L")
                        .arg(source_url)
                        .arg("-o")
                        .arg(format!("{}.tmpdownload", &destination))
                        .output()
                        .expect("Failed to execute command");

                    if !output.status.success() {
                        eprintln!(
                            "Download failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }

                    println!("Extracting {} into {}", source_url, &destination);

                    let output = Command::new("unzip")
                        .arg("-o")
                        .arg(format!("{}.tmpdownload", &destination))
                        .arg("-d")
                        .arg(&destination)
                        .output()
                        .expect("Failed to execute command");

                    if !output.status.success() {
                        eprintln!(
                            "Extraction failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
            }
        }
        None => println!("No sources to clone"),
    }

    println!("Build environment setup successfully");

    return (build_dir, out_dir, package_dir);
}
