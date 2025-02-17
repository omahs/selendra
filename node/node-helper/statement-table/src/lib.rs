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

//! The statement table.
//!
//! This stores messages other authorities issue about candidates.
//!
//! These messages are used to create a proposal submitted to a BFT consensus process.
//!
//! Proposals are formed of sets of candidates which have the requisite number of
//! validity and availability votes.
//!
//! Each parachain is associated with two sets of authorities: those which can
//! propose and attest to validity of candidates, and those who can only attest
//! to availability.

pub mod generic;

pub use generic::{Context, Table};

/// Concrete instantiations suitable for v2 primitives.
pub mod v2 {
	use crate::generic;
	use primitives::v2::{
		CandidateHash, CommittedCandidateReceipt, CompactStatement as PrimitiveStatement, Id,
		ValidatorIndex, ValidatorSignature,
	};

	/// Statements about candidates on the network.
	pub type Statement = generic::Statement<CommittedCandidateReceipt, CandidateHash>;

	/// Signed statements about candidates.
	pub type SignedStatement = generic::SignedStatement<
		CommittedCandidateReceipt,
		CandidateHash,
		ValidatorIndex,
		ValidatorSignature,
	>;

	/// Kinds of misbehavior, along with proof.
	pub type Misbehavior = generic::Misbehavior<
		CommittedCandidateReceipt,
		CandidateHash,
		ValidatorIndex,
		ValidatorSignature,
	>;

	/// A summary of import of a statement.
	pub type Summary = generic::Summary<CandidateHash, Id>;

	impl<'a> From<&'a Statement> for PrimitiveStatement {
		fn from(s: &'a Statement) -> PrimitiveStatement {
			match *s {
				generic::Statement::Valid(s) => PrimitiveStatement::Valid(s),
				generic::Statement::Seconded(ref s) => PrimitiveStatement::Seconded(s.hash()),
			}
		}
	}
}
