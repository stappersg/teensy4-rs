//! Allows us to auto-import a iMXRT1062 module
//! from the svd2rust output into the imxrt1062-pac
//! megacrate. It code-ifies some manual work.
//!
//! This could probably use some better error handling...

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

mod toml;

static CARGO_TOML_DEPENDENCIES: &str = r#"vcell = "0.1.2"
"#;

static CARGO_NO_TESTS_BENCH: &str = r#"
[lib]
bench = false
test = false
"#;

fn add_deps(crate_path: &Path) {
    let mut cargo_toml = fs::OpenOptions::new()
        .append(true)
        .open(crate_path.join("Cargo.toml"))
        .expect("Cannot open Cargo.toml");

    cargo_toml
        .write_all(CARGO_TOML_DEPENDENCIES.as_bytes())
        .expect("Failed to update Cargo.toml");

    cargo_toml
        .write_all(CARGO_NO_TESTS_BENCH.as_bytes())
        .expect("Failed to update Cargo.toml");
}

static LIB_PRELUDE: &str = r#"#![deny(warnings)]
#![allow(non_camel_case_types)]
#![allow(clippy::all)]
#![no_std]

include!("../../generic.rs");

"#;

fn write_lib<R: Read>(crate_path: &Path, mut src: R) {
    let mut crate_lib =
        fs::File::create(crate_path.join("src").join("lib.rs")).expect("Unable to crate lib.rs");
    crate_lib
        .write_all(LIB_PRELUDE.as_bytes())
        .expect("Unable to write lib.rs prelude");
    io::copy(&mut src, &mut crate_lib).unwrap();
}

fn copy_contents<I: Iterator<Item = io::Result<fs::DirEntry>>>(crate_path: &Path, dir: I) {
    for entry in dir {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            let dst_dir = crate_path.join(entry.path().file_name().unwrap());
            fs::create_dir(&dst_dir).unwrap();
            copy_contents(&dst_dir, fs::read_dir(entry.path()).unwrap());
        } else {
            fs::copy(
                entry.path(),
                crate_path.join(entry.path().file_name().unwrap()),
            )
            .unwrap();
        }
    }
}

fn suggest_reexports(crate_names: &[String]) {
    println!("imxrt1062-pac reexport additions...");
    for crate_name in crate_names {
        let crate_name = crate_name.replace("-", "_");
        // pub use imxrt1062_foo_bar as foo_bar
        let module = crate_name
            .split("_")
            .skip(1)
            .map(String::from)
            .collect::<Vec<String>>()
            .join("_");
        println!("pub use {} as {};", crate_name, module);
    }
}

/// Update the workspace Cargo.toml.
/// We assume that we're running this binary from
/// this directory, and that the workspace Cargo.toml
/// is one level up
fn update_workspace_toml(crate_names: &[String]) {
    static WORKSPACE_CARGO_TOML: &str = "../Cargo.toml";
    let mut workspace: toml::Workspace = {
        let file = fs::read(WORKSPACE_CARGO_TOML).expect("Cannot read workspace Cargo.toml");
        ::toml::de::from_slice(&file).unwrap()
    };
    for crate_name in crate_names {
        workspace.add_member(PathBuf::from(OUTPUT_PAC_NAME).join(crate_name));
    }
    let new_toml = ::toml::ser::to_string_pretty(&workspace).unwrap();
    fs::write(WORKSPACE_CARGO_TOML, new_toml).unwrap();
}

fn update_pac_dependencies(output_pac: &Path, crate_names: &[String]) {
    let output_pac_toml = output_pac.join("Cargo.toml");
    let mut krate: toml::Krate = {
        let file = fs::read(&output_pac_toml).unwrap();
        ::toml::de::from_slice(&file).unwrap()
    };
    for crate_name in crate_names {
        krate.add_dependency(crate_name, crate_name);
    }
    let new_toml = ::toml::ser::to_string_pretty(&krate).unwrap();
    fs::write(&output_pac_toml, new_toml).unwrap();
}

static OUTPUT_PAC_NAME: &str = "imxrt1062-pac";

fn main() {
    let output_pac: PathBuf = PathBuf::from("../").join(OUTPUT_PAC_NAME);
    let mut args = env::args().skip(1);
    let svd_crate_path = match args.next() {
        Some(path) => PathBuf::from(path),
        None => {
            println!("usage: path/to/svd2rust/output module_name ...");
            process::exit(1);
        }
    };

    let mut new_pac_crates: Vec<String> = Vec::new();
    for module_name in args {
        let module_name = &module_name;
        let peripheral_module_src = fs::File::open(
            svd_crate_path
                .join("src")
                .join(format!("{}.rs", module_name)),
        )
        .expect(&format!("Unable to find main module for {}", module_name));
        let peripheral_dir_src = fs::read_dir(svd_crate_path.join("src").join(module_name)).expect(
            &format!("Unable to find module directory for {}", module_name),
        );

        let crate_name = format!("imxrt1062-{}", module_name.replace("_", "-"));
        let peripheral_crate_path = output_pac.join(crate_name.clone());
        if peripheral_crate_path.exists() {
            println!(
                "{} peripheral crate seems to already exist! Skipping...",
                peripheral_crate_path.display()
            );
            continue;
        }
        process::Command::new("cargo")
            .args(&[
                "new",
                "--lib",
                &format!("{}", peripheral_crate_path.display()),
                "--vcs",
                "none",
            ])
            .output()
            .expect(&format!(
                "Cannot create peripheral crate for '{}'",
                module_name
            ));

        add_deps(&peripheral_crate_path);
        write_lib(&peripheral_crate_path, peripheral_module_src);
        copy_contents(&peripheral_crate_path.join("src"), peripheral_dir_src);

        println!("{} crate was created! Add the crate to the workspace, re-export it from the main PAC crate, and enable the relevant structs in the PAC crate", peripheral_crate_path.display());
        new_pac_crates.push(crate_name);
    }

    if !new_pac_crates.is_empty() {
        suggest_reexports(&new_pac_crates);
        update_workspace_toml(&new_pac_crates);
        update_pac_dependencies(&output_pac, &new_pac_crates);
    }
}
