/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::env;
use std::io;
use std::path::{Path, PathBuf};

extern crate clap;
#[macro_use]
extern crate log;
extern crate proc_macro2;
#[macro_use]
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate quote;
#[macro_use]
extern crate syn;
extern crate toml;

use clap::{App, Arg, ArgMatches};
use heck::ShoutySnakeCase;

mod bindgen;
mod logging;

use crate::bindgen::{Bindings, Builder, Cargo, Error};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(default)]
struct Config {
    /// A list of additional system includes to put at the beginning of the generated header
    pub sys_includes: Vec<String>,
    /// Package namespace / prefix
    pub namespace: Option<String>,
}

impl Config {
    #[allow(unused)]
    fn from_file<P: AsRef<Path>>(file_name: P) -> Result<Config, String> {
        let config_text = std::fs::read_to_string(file_name.as_ref()).map_err(|_| {
            format!(
                "Couldn't open config file: {}.",
                file_name.as_ref().display()
            )
        })?;

        match toml::from_str::<Config>(&config_text) {
            Ok(x) => Ok(x),
            Err(e) => Err(format!("Couldn't parse config file: {}.", e)),
        }
    }

    #[allow(unused)]
    fn from_root_or_default<P: AsRef<Path>>(root: P) -> Config {
        let c = root.as_ref().join("gbindgen.toml");

        if c.exists() {
            Config::from_file(c).unwrap()
        } else {
            Config::default()
        }
    }
}

fn load_bindings<'a>(input: &Path, matches: &ArgMatches<'a>) -> Result<Bindings, Error> {
    // We have to load a whole crate, so we use cargo to gather metadata
    let lib = Cargo::load(
        input,
        matches.value_of("lockfile"),
        matches.value_of("crate"),
        true,
        matches.is_present("clean"),
        matches.value_of("metadata").map(Path::new),
    )?;

    let binding_crate_dir = lib.find_crate_dir(&lib.binding_crate_ref());

    let config = if let Some(binding_crate_dir) = binding_crate_dir {
        Config::from_root_or_default(&binding_crate_dir)
    } else {
        // This shouldn't happen
        Config::from_root_or_default(input)
    };

    let mut bindgen_config = bindgen::Config::default();
    bindgen_config.tab_width = 4;
    bindgen_config.sys_includes = config.sys_includes;
    let version = lib
        .binding_crate_ref()
        .version
        .and_then(|v| semver::Version::parse(&v).ok()).expect("Failed to parse crate version");
    bindgen_config.after_includes = Some(format!(r#"
#define {ns}_MAJOR_VERSION {major}
#define {ns}_MINOR_VERSION {minor}
#define {ns}_MICRO_VERSION {micro}

#define {ns}_CHECK_VERSION(major,minor,micro) \
    ({ns}_MAJOR_VERSION > (major) ||                                   \
     ({ns}_MAJOR_VERSION == (major) && {ns}_MINOR_VERSION > (minor)) || \
     ({ns}_MAJOR_VERSION == (major) && {ns}_MINOR_VERSION == (minor) && \
      {ns}_MICRO_VERSION >= (micro)))
"#,
        ns = config.namespace.unwrap().to_shouty_snake_case(),
        major = version.major,
        minor = version.minor,
        micro = version.patch
    ));

    Builder::new()
        .with_config(bindgen_config)
        .with_gobject(true)
        .with_header(&format!(
            "/* GObject C binding from Rust {} project, generated with gbindgen: DO NOT EDIT. */",
            lib.binding_crate_name()
        ))
        .with_cargo(lib)
        .generate()
}

fn main() {
    let matches = App::new("gbindgen")
        .version(bindgen::VERSION)
        .about("Generate GObject C bindings for a glib/gtk-rs library")
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Enable verbose logging"),
        )
        .arg(
            Arg::with_name("verify")
                .long("verify")
                .help("Generate bindings and compare it to the existing bindings file and error if they are different"),
        )
        .arg(
            Arg::with_name("INPUT")
                .help(
                    "A crate directory or source file to generate bindings for. \
                    In general this is the folder where the Cargo.toml file of \
                    source Rust library resides.")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::with_name("out")
                .short("o")
                .long("output")
                .value_name("PATH")
                .help("The file to output the bindings to")
                .required(false),
        )
        .arg(
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .help("Report errors only (overrides verbosity options).")
                .required(false),
        )
        .get_matches();

    // Initialize logging
    if matches.is_present("quiet") {
        logging::ErrorLogger::init().unwrap();
    } else {
        match matches.occurrences_of("v") {
            0 => logging::WarnLogger::init().unwrap(),
            1 => logging::InfoLogger::init().unwrap(),
            _ => logging::TraceLogger::init().unwrap(),
        }
    }

    // Find the input directory
    let input = match matches.value_of("INPUT") {
        Some(input) => PathBuf::from(input),
        None => env::current_dir().unwrap(),
    };

    let bindings = match load_bindings(&input, &matches) {
        Ok(bindings) => bindings,
        Err(msg) => {
            error!("{}", msg);
            error!("Couldn't generate bindings for {}.", input.display());
            std::process::exit(1);
        }
    };

    // Write the bindings file
    match matches.value_of("out") {
        Some(file) => {
            let changed = bindings.write_to_file(file);

            if matches.is_present("verify") && changed {
                error!("Bindings changed: {}", file);
                std::process::exit(2);
            }
        }
        _ => {
            bindings.write(io::stdout());
        }
    }
}
