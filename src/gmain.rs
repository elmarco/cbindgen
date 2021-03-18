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

mod bindgen;
mod logging;

use crate::bindgen::{Bindings, Builder, Cargo, Error};

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

    Builder::new()
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
