// Copyright (C) 2021-2022 Selendra.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Migration code for the parachain's DB.

#![cfg(feature = "full-node")]

use super::{columns, other_io_error, DatabaseKind, LOG_TARGET};
use std::{
	fs, io,
	path::{Path, PathBuf},
	str::FromStr,
};

type Version = u32;

/// Version file name.
const VERSION_FILE_NAME: &'static str = "parachain_db_version";

/// Current db version.
const CURRENT_VERSION: Version = 1;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("I/O error when reading/writing the version")]
	Io(#[from] io::Error),
	#[error("The version file format is incorrect")]
	CorruptedVersionFile,
	#[error("Future version (expected {current:?}, found {got:?})")]
	FutureVersion { current: Version, got: Version },
}

impl From<Error> for io::Error {
	fn from(me: Error) -> io::Error {
		match me {
			Error::Io(e) => e,
			_ => super::other_io_error(me.to_string()),
		}
	}
}

/// Try upgrading parachain's database to the current version.
pub(crate) fn try_upgrade_db(db_path: &Path, db_kind: DatabaseKind) -> Result<(), Error> {
	let is_empty = db_path.read_dir().map_or(true, |mut d| d.next().is_none());
	if !is_empty {
		match get_db_version(db_path)? {
			// 0 -> 1 migration
			Some(0) => migrate_from_version_0_to_1(db_path, db_kind)?,
			// Already at current version, do nothing.
			Some(CURRENT_VERSION) => (),
			// This is an arbitrary future version, we don't handle it.
			Some(v) => return Err(Error::FutureVersion { current: CURRENT_VERSION, got: v }),
			// No version file. For `RocksDB` we dont need to do anything.
			None if db_kind == DatabaseKind::RocksDB => (),
			// No version file. `ParityDB` did not previously have a version defined.
			// We handle this as a `0 -> 1` migration.
			None if db_kind == DatabaseKind::ParityDB =>
				migrate_from_version_0_to_1(db_path, db_kind)?,
			None => unreachable!(),
		}
	}

	update_version(db_path)
}

/// Reads current database version from the file at given path.
/// If the file does not exist returns `None`, otherwise the version stored in the file.
fn get_db_version(path: &Path) -> Result<Option<Version>, Error> {
	match fs::read_to_string(version_file_path(path)) {
		Err(ref err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(err) => Err(err.into()),
		Ok(content) => u32::from_str(&content)
			.map(|v| Some(v))
			.map_err(|_| Error::CorruptedVersionFile),
	}
}

/// Writes current database version to the file.
/// Creates a new file if the version file does not exist yet.
fn update_version(path: &Path) -> Result<(), Error> {
	fs::create_dir_all(path)?;
	fs::write(version_file_path(path), CURRENT_VERSION.to_string()).map_err(Into::into)
}

/// Returns the version file path.
fn version_file_path(path: &Path) -> PathBuf {
	let mut file_path = path.to_owned();
	file_path.push(VERSION_FILE_NAME);
	file_path
}

fn migrate_from_version_0_to_1(path: &Path, db_kind: DatabaseKind) -> Result<(), Error> {
	gum::info!(target: LOG_TARGET, "Migrating parachains db from version 0 to version 1 ...");

	match db_kind {
		DatabaseKind::ParityDB => paritydb_migrate_from_version_0_to_1(path),
		DatabaseKind::RocksDB => rocksdb_migrate_from_version_0_to_1(path),
	}
	.and_then(|result| {
		gum::info!(target: LOG_TARGET, "Migration complete! ");
		Ok(result)
	})
}

/// Migration from version 0 to version 1:
/// * the number of columns has changed from 3 to 5;
fn rocksdb_migrate_from_version_0_to_1(path: &Path) -> Result<(), Error> {
	use kvdb_rocksdb::{Database, DatabaseConfig};

	let db_path = path
		.to_str()
		.ok_or_else(|| super::other_io_error("Invalid database path".into()))?;
	let db_cfg = DatabaseConfig::with_columns(super::columns::v0::NUM_COLUMNS);
	let db = Database::open(&db_cfg, db_path)?;

	db.add_column()?;
	db.add_column()?;

	Ok(())
}

// This currently clears columns which had their configs altered between versions.
// The columns to be changed are constrained by the `allowed_columns` vector.
fn paritydb_fix_columns(
	path: &Path,
	options: parity_db::Options,
	allowed_columns: Vec<u32>,
) -> io::Result<()> {
	// Figure out which columns to delete. This will be determined by inspecting
	// the metadata file.
	if let Some(metadata) = parity_db::Options::load_metadata(&path)
		.map_err(|e| other_io_error(format!("Error reading metadata {:?}", e)))?
	{
		let columns_to_clear = metadata
			.columns
			.into_iter()
			.enumerate()
			.filter(|(idx, _)| allowed_columns.contains(&(*idx as u32)))
			.filter_map(|(idx, opts)| {
				let changed = opts != options.columns[idx];
				if changed {
					gum::debug!(
						target: LOG_TARGET,
						"Column {} will be cleared. Old options: {:?}, New options: {:?}",
						idx,
						opts,
						options.columns[idx]
					);
					Some(idx)
				} else {
					None
				}
			})
			.collect::<Vec<_>>();

		if columns_to_clear.len() > 0 {
			gum::debug!(
				target: LOG_TARGET,
				"Database column changes detected, need to cleanup {} columns.",
				columns_to_clear.len()
			);
		}

		for column in columns_to_clear {
			gum::debug!(target: LOG_TARGET, "Clearing column {}", column,);
			parity_db::clear_column(path, column.try_into().expect("Invalid column ID"))
				.map_err(|e| other_io_error(format!("Error clearing column {:?}", e)))?;
		}

		// Write the updated column options.
		options
			.write_metadata(path, &metadata.salt)
			.map_err(|e| other_io_error(format!("Error writing metadata {:?}", e)))?;
	}

	Ok(())
}

/// Database configuration for version 1.
pub(crate) fn paritydb_version_1_config(path: &Path) -> parity_db::Options {
	let mut options =
		parity_db::Options::with_columns(&path, super::columns::v1::NUM_COLUMNS as u8);
	for i in columns::v1::ORDERED_COL {
		options.columns[*i as usize].btree_index = true;
	}

	options
}

/// Database configuration for version 0. This is useful just for testing.
#[cfg(test)]
pub(crate) fn paritydb_version_0_config(path: &Path) -> parity_db::Options {
	let mut options =
		parity_db::Options::with_columns(&path, super::columns::v1::NUM_COLUMNS as u8);
	options.columns[super::columns::v1::COL_AVAILABILITY_META as usize].btree_index = true;
	options.columns[super::columns::v1::COL_CHAIN_SELECTION_DATA as usize].btree_index = true;

	options
}

/// Migration from version 0 to version 1.
/// Cases covered:
/// - upgrading from v0.9.23 or earlier -> the `dispute coordinator column` was changed
/// - upgrading from v0.9.24+ -> this is a no op assuming the DB has been manually fixed as per
/// release notes
fn paritydb_migrate_from_version_0_to_1(path: &Path) -> Result<(), Error> {
	// Delete the `dispute coordinator` column if needed (if column configuration is changed).
	paritydb_fix_columns(
		path,
		paritydb_version_1_config(path),
		vec![super::columns::v1::COL_DISPUTE_COORDINATOR_DATA],
	)?;

	Ok(())
}

#[cfg(test)]
mod tests {
	#[test]
	fn test_paritydb_migrate_0_1() {
		use super::{columns::v1::*, *};
		use parity_db::Db;

		let db_dir = tempfile::tempdir().unwrap();
		let path = db_dir.path();
		{
			let db = Db::open_or_create(&paritydb_version_0_config(&path)).unwrap();

			db.commit(vec![
				(COL_DISPUTE_COORDINATOR_DATA as u8, b"1234".to_vec(), Some(b"somevalue".to_vec())),
				(COL_AVAILABILITY_META as u8, b"5678".to_vec(), Some(b"somevalue".to_vec())),
			])
			.unwrap();
		}

		try_upgrade_db(&path, DatabaseKind::ParityDB).unwrap();

		let db = Db::open(&paritydb_version_1_config(&path)).unwrap();
		assert_eq!(
			db.get(super::columns::v1::COL_DISPUTE_COORDINATOR_DATA as u8, b"1234").unwrap(),
			None
		);
		assert_eq!(
			db.get(super::columns::v1::COL_AVAILABILITY_META as u8, b"5678").unwrap(),
			Some("somevalue".as_bytes().to_vec())
		);
	}
}
