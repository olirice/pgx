// Copyright 2020 ZomboDB, LLC <zombodb@gmail.com>. All rights reserved. Use of this source code is
// governed by the MIT license that can be found in the LICENSE file.

use crate::{
    command::{
        get::get_property, install::install_extension, start::start_postgres, stop::stop_postgres,
    },
    CommandExecute,
};
use eyre::{eyre, WrapErr};
use owo_colors::OwoColorize;
use pgx_utils::{
    createdb,
    pg_config::{PgConfig, Pgx},
};
use cargo_toml::Manifest;
use std::{path::Path, os::unix::process::CommandExt, process::Command};
/// Compile/install extension to a pgx-managed Postgres instance and start psql
#[derive(clap::Args, Debug)]
#[clap(author)]
pub(crate) struct Run {
    /// Do you want to run against Postgres `pg10`, `pg11`, `pg12`, `pg13`, `pg14`?
    #[clap(env = "PG_VERSION")]
    pg_version: Option<String>,
    /// The database to connect to (and create if the first time).  Defaults to a database with the same name as the current extension name
    dbname: Option<String>,
    /// Package to build (see `cargo help pkgid`)
    #[clap(long, short)]
    package: Option<String>,
    /// Path to Cargo.toml
    #[clap(long)]
    manifest_path: Option<String>,
    /// Compile for release mode (default is debug)
    #[clap(env = "PROFILE", long, short)]
    release: bool,
    #[clap(flatten)]
    features: clap_cargo::Features,
    #[clap(from_global, parse(from_occurrences))]
    verbose: usize,
}

impl CommandExecute for Run {
    #[tracing::instrument(level = "error", skip(self))]
    fn execute(mut self) -> eyre::Result<()> {
        let metadata = crate::metadata::metadata(&self.features, self.manifest_path.as_ref())
            .wrap_err("couldn't get cargo metadata")?;
        crate::metadata::validate(&metadata)?;
        let package_manifest_path = crate::manifest::manifest_path(&metadata, self.package.as_ref())
            .wrap_err("Couldn't get manifest path")?;
        let package_manifest = Manifest::from_path(&package_manifest_path)
            .wrap_err("Couldn't parse manifest")?;

        let pgx = Pgx::from_config()?;

        let (pg_config, pg_version) = match self.pg_version {
            Some(pg_version) => {
                match pgx.get(&pg_version) {
                    Ok(pg_config) => (pg_config, pg_version),
                    Err(err) => {
                        if self.dbname.is_some() {
                            return Err(err);
                        }
                        // It's actually the dbname! We should infer from the manifest.
                        self.dbname = Some(pg_version);
                        let default_pg_version = crate::manifest::default_pg_version(&package_manifest)
                            .ok_or(eyre!("No provided `pg$VERSION` flag."))?;
                        (pgx.get(&default_pg_version)?, default_pg_version)
                    }
                }
            }
            None => {
                // We should infer from the manifest.
                let default_pg_version = crate::manifest::default_pg_version(&package_manifest)
                    .ok_or(eyre!("No provided `pg$VERSION` flag."))?;
                (pgx.get(&default_pg_version)?, default_pg_version)
            }
        };

        let features = crate::manifest::features_for_version(self.features, &package_manifest, &pg_version);

        let dbname = match self.dbname {
            Some(dbname) => dbname,
            None => get_property(&package_manifest_path, "extname")?.ok_or(eyre!("could not determine extension name"))?,
        };

        run_psql(
            pg_config,
            self.manifest_path.as_ref(),
            self.package.as_ref(),
            package_manifest_path,
            &dbname,
            self.release,
            &features,
        )
    }
}

#[tracing::instrument(level = "error", skip_all, fields(
    pg_version = %pg_config.version()?,
    dbname,
    release = is_release,
))]
pub(crate) fn run_psql(
    pg_config: &PgConfig,
    user_manifest_path: Option<impl AsRef<Path>>,
    user_package: Option< &String>,
    package_manifest_path: impl AsRef<Path>,
    dbname: &str,
    is_release: bool,
    features: &clap_cargo::Features,
) -> eyre::Result<()> {
    // stop postgres
    stop_postgres(pg_config)?;

    // install the extension
    install_extension(
        user_manifest_path, user_package, package_manifest_path, pg_config, is_release, false, None, features,
    )?;

    // restart postgres
    start_postgres(pg_config)?;

    // create the named database
    if !createdb(pg_config, dbname, false, true)? {
        println!(
            "{} existing database {}",
            "    Re-using".bold().cyan(),
            dbname
        );
    }

    // run psql
    exec_psql(pg_config, dbname)
}

pub(crate) fn exec_psql(pg_config: &PgConfig, dbname: &str) -> eyre::Result<()> {
    let mut command = Command::new(pg_config.psql_path()?);
    command
        .env_remove("PGDATABASE")
        .env_remove("PGHOST")
        .env_remove("PGPORT")
        .env_remove("PGUSER")
        .arg("-h")
        .arg(pg_config.host())
        .arg("-p")
        .arg(pg_config.port()?.to_string())
        .arg(dbname);

    // we'll never return from here as we've now become psql
    panic!("{}", command.exec());
}
