// Copyright 2017-2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

//! This is part of the Substrate runtime.

#![warn(missing_docs)]

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "std"), feature(lang_items))]
#![cfg_attr(not(feature = "std"), feature(alloc_error_handler))]
#![cfg_attr(not(feature = "std"), feature(core_intrinsics))]

#![cfg_attr(feature = "std", doc = "Substrate runtime standard library as compiled when linked with Rust's standard library.")]
#![cfg_attr(not(feature = "std"), doc = "Substrate's runtime standard library as compiled without Rust's standard library.")]

use rstd::vec::Vec;

use primitives::{
	crypto::KeyTypeId, ed25519, sr25519, H256,
	offchain::{
		Timestamp, HttpRequestId, HttpRequestStatus, HttpError, StorageKind, OpaqueNetworkState,
	},
	child_storage_key::ChildStorageKey,
};

use trie::{TrieConfiguration, trie_types::Layout};

use runtime_interface::runtime_interface;

use codec::{Encode, Decode};

/// Error verifying ECDSA signature
#[derive(Encode, Decode)]
pub enum EcdsaVerifyError {
	/// Incorrect value of R or S
	BadRS,
	/// Incorrect value of V
	BadV,
	/// Invalid signature
	BadSignature,
}

/// Interface for accessing the storage from within the runtime.
#[runtime_interface]
pub trait Storage {
	/// Returns the data for `key` in the storage or `None` if the key can not be found.
	fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
		self.storage(key).map(|s| s.to_vec())
	}

	/// Returns the data for `key` in the child storage or `None` if the key can not be found.
	fn child_get(&self, child_storage_key: &[u8], key: &[u8]) -> Option<Vec<u8>> {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		ext.child_storage(storage_key, key).map(|s| s.to_vec())
	}

	/// Get `key` from storage, placing the value into `value_out` and return the number of
	/// bytes that the entry in storage has beyond the offset or `None` if the storage entry
	/// doesn't exist at all.
	/// If `value_out` length is smaller than the returned length, only `value_out` length bytes
	/// are copied into `value_out`.
	fn read(&self, key: &[u8], value_out: &mut [u8], value_offset: u32) -> Option<u32> {
		self.storage(key).map(|value| {
			let value_offset = value_offset as usize;
			let data = &value[value_offset.min(value.len())..];
			let written = std::cmp::min(data.len(), value_out.len());
			value_out[..written].copy_from_slice(&data[..written]);
			value.len() as u32
		})
	}

	/// Get `key` from child storage, placing the value into `value_out` and return the number
	/// of bytes that the entry in storage has beyond the offset or `None` if the storage entry
	/// doesn't exist at all.
	/// If `value_out` length is smaller than the returned length, only `value_out` length bytes
	/// are copied into `value_out`.
	fn child_read(
		&self,
		child_storage_key: &[u8],
		key: &[u8],
		value_out: &mut [u8],
		value_offset: u32,
	) -> Option<u32> {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.child_storage(storage_key, key)
			.map(|value| {
				let value_offset = value_offset as usize;
				let data = &value[value_offset.min(value.len())..];
				let written = std::cmp::min(data.len(), value_out.len());
				value_out[..written].copy_from_slice(&data[..written]);
				value.len() as u32
			})
	}

	/// Set `key` to `value` in the storage.
	fn set(&mut self, key: &[u8], value: &[u8]) {
		self.set_storage(key.to_vec(), value.to_vec());
	}

	/// Set `key` to `value` in the child storage denoted by `child_storage_key`.
	fn child_set(&mut self, child_storage_key: &[u8], key: &[u8], value: &[u8]) {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.set_child_storage(storage_key, key.to_vec(), value.to_vec());
	}

	/// Clear the storage of the given `key` and its value.
	fn clear(&mut self, key: &[u8]) {
		self.clear_storage(key)
	}

	/// Clear the storage of a key.
	fn clear_child(&mut self, child_storage_key: &[u8], key: &[u8]) {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.clear_child_storage(storage_key, key);
	}

	/// Clear an entire child storage.
	fn kill_child_storage(&mut self, child_storage_key: &[u8]) {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.kill_child_storage(storage_key);
	}

	/// Check whether the given `key` exists in storage.
	fn exists(&self, key: &[u8]) -> bool {
		self.exists_storage(key)
	}

	/// Check whether the given `key` exists in storage.
	fn child_exists(&self, child_storage_key: &[u8], key: &[u8]) -> bool {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.exists_child_storage(storage_key, key)
	}

	/// Clear the storage of each key-value pair where the key starts with the given `prefix`.
	fn clear_prefix(&mut self, prefix: &[u8]) {
		self.clear_prefix(prefix)
	}

	/// Clear the child storage of each key-value pair where the key starts with the given `prefix`.
	fn child_clear_prefix(&mut self, child_storage_key: &[u8], prefix: &[u8]) {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.clear_child_prefix(storage_key, prefix);
	}

	/// "Commit" all existing operations and compute the resulting storage root.
	fn root(&mut self) -> [u8; 32] {
		self.storage_root()
	}

	/// "Commit" all existing operations and compute the resulting child storage root.
	fn child_root(&mut self, child_storage_key: &[u8]) -> Vec<u8> {
		let storage_key = child_storage_key_or_panic(child_storage_key);
		self.child_storage_root(storage_key)
	}

	/// "Commit" all existing operations and get the resulting storage change root.
	fn changes_root(&mut self, parent_hash: [u8; 32]) -> Option<[u8; 32]> {
		self.storage_changes_root(parent_hash.into()).map(|h| h.map(|h| h.into())).ok()
	}

	/// A trie root formed from the iterated items.
	fn blake2_256_trie_root(input: Vec<(Vec<u8>, Vec<u8>)>) -> H256 {
		Layout::<Blake2Hasher>::trie_root(input)
	}

	/// A trie root formed from the enumerated items.
	fn blake2_256_ordered_trie_root(input: Vec<Vec<u8>>) -> H256 {
		Layout::<Blake2Hasher>::ordered_trie_root(input)
	}
}

/// Interface that provides miscellaneous functions for communicating between the runtime and the node.
#[runtime_interface]
pub trait Misc {
	/// The current relay chain identifier.
	fn chain_id(&self) -> u64 {
		self.chain_id()
	}

	/// Print a number.
	fn print_num(val: u64) {
		println!("{}", val);
	}

	/// Print any valid `utf8` buffer.
	fn print_utf8(utf8: &[u8]) {
		if let Ok(data) = std::str::from_utf8(utf8) {
			println!("{}", data)
		}
	}

	/// Print any `u8` slice as hex.
	fn print_hex(data: &[u8]) {
		println!("{}", HexDisplay::from(&data));
	}
}

/// Interfaces for working with crypto related types from within the runtime.
pub trait Crypto {
	/// Returns all `ed25519` public keys for the given key id from the keystore.
	fn ed25519_public_keys(&self, id: KeyTypeId) -> Vec<ed25519::Public> {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.read()
			.ed25519_public_keys(id)
	}

	/// Generate an `ed22519` key for the given key type and store it in the keystore.
	///
	/// Returns the public key.
	fn ed25519_generate(&self, id: KeyTypeId, seed: Option<&str>) -> ed25519::Public {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.write()
			.ed25519_generate_new(id, seed)
			.expect("`ed25519_generate` failed")
	}

	/// Sign the given `msg` with the `ed25519` key that corresponds to the given public key and
	/// key type in the keystore.
	///
	/// Returns the signature.
	fn ed25519_sign(
		&self,
		id: KeyTypeId,
		pub_key: &ed25519::Public,
		msg: &[u8],
	) -> Option<ed25519::Signature> {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.read()
			.ed25519_key_pair(id, &pub_key)
			.map(|k| k.sign(msg))
	}

	/// Verify an `ed25519` signature.
	///
	/// Returns `true` when the verification in successful.
	fn ed25519_verify(
		&self,
		sig: &ed25519::Signature,
		msg: &[u8],
		pub_key: &ed25519::Public,
	) -> bool {
		ed25519::Pair::verify(sig, msg, pub_key)
	}

	/// Returns all `sr25519` public keys for the given key id from the keystore.
	fn sr25519_public_keys(&self, id: KeyTypeId) -> Vec<sr25519::Public> {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.read()
			.sr25519_public_keys(id)
	}

	/// Generate an `sr22519` key for the given key type and store it in the keystore.
	///
	/// Returns the public key.
	fn sr25519_generate(&self, id: KeyTypeId, seed: Option<&str>) -> sr25519::Public {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.write()
			.sr25519_generate_new(id, seed)
			.expect("`sr25519_generate` failed")
	}

	/// Sign the given `msg` with the `sr25519` key that corresponds to the given public key and
	/// key type in the keystore.
	///
	/// Returns the signature.
	fn sr25519_sign(
		self,
		id: KeyTypeId,
		pub_key: &sr25519::Public,
		msg: &[u8],
	) -> Option<sr25519::Signature> {
		self.keystore()
			.expect("No `keystore` associated for the current context!")
			.read()
			.sr25519_key_pair(id, &pub_key)
			.map(|k| k.sign(msg))
	}

	/// Verify an `sr25519` signature.
	///
	/// Returns `true` when the verification in successful.
	fn sr25519_verify(sig: &sr25519::Signature, msg: &[u8], pubkey: &sr25519::Public) -> bool {
		sr25519::Pair::verify(sig, msg, pubkey)
	}

	/// Verify and recover a SECP256k1 ECDSA signature.
	/// - `sig` is passed in RSV format. V should be either 0/1 or 27/28.
	/// Returns `Err` if the signature is bad, otherwise the 64-byte pubkey
	/// (doesn't include the 0x04 prefix).
	fn secp256k1_ecdsa_recover(
		sig: &[u8; 65],
		msg: &[u8; 32],
	) -> Result<[u8; 64], EcdsaVerifyError> {
		let rs = secp256k1::Signature::parse_slice(&sig[0..64])
			.map_err(|_| EcdsaVerifyError::BadRS)?;
		let v = secp256k1::RecoveryId::parse(if sig[64] > 26 { sig[64] - 27 } else { sig[64] } as u8)
			.map_err(|_| EcdsaVerifyError::BadV)?;
		let pubkey = secp256k1::recover(&secp256k1::Message::parse(msg), &rs, &v)
			.map_err(|_| EcdsaVerifyError::BadSignature)?;
		let mut res = [0u8; 64];
		res.copy_from_slice(&pubkey.serialize()[1..65]);
		Ok(res)
	}
}

/// Interface that provides functions for hashing with different algorithms.
#[runtime_interface]
pub trait Hashing {
	/// Conduct a 256-bit Keccak hash.
	fn keccak_256(data: &[u8]) -> [u8; 32] {
		tiny_keccak::keccak256(data)
	}

	/// Conduct a 128-bit Blake2 hash.
	fn blake2_128(data: &[u8]) -> [u8; 16] {
		blake2_128(data)
	}

	/// Conduct a 256-bit Blake2 hash.
	fn blake2_256(data: &[u8]) -> [u8; 32] {
		blake2_256(data)
	}

	/// Conduct four XX hashes to give a 256-bit result.
	fn twox_256(data: &[u8]) -> [u8; 32] {
		twox_256(data)
	}

	/// Conduct two XX hashes to give a 128-bit result.
	fn twox_128(data: &[u8]) -> [u8; 16] {
		twox_128(data)
	}

	/// Conduct two XX hashes to give a 64-bit result.
	fn twox_64(data: &[u8]) -> [u8; 8] {
		twox_64(data)
	}
}

/// Interface that provides functions to access the offchain functionality.
#[runtime_interface]
pub trait Offchain {
	/// Returns if the local node is a potential validator.
	///
	/// Even if this function returns `true`, it does not mean that any keys are configured
	/// and that the validator is registered in the chain.
	fn is_validator(&self) -> bool {
		self.offchain()
			.expect("is_validator can be called only in the offchain worker context")
			.is_validator()
	}

	/// Submit an encoded transaction to the pool.
	///
	/// The transaction will end up in the pool.
	fn submit_transaction(&self, data: Vec<u8>) -> Result<(), ()> {
		self.offchain()
			.expect("submit_transaction can be called only in the offchain worker context")
			.submit_transaction(data)
	}

	/// Returns information about the local node's network state.
	fn network_state(&self) -> Result<OpaqueNetworkState, ()> {
		self.offchain()
			.expect("network_state can be called only in the offchain worker context")
			.network_state()
	}

	/// Returns current UNIX timestamp (in millis)
	fn timestamp(&self) -> Timestamp {
		self.offchain()
			.expect("timestamp can be called only in the offchain worker context")
			.timestamp()
	}

	/// Pause the execution until `deadline` is reached.
	fn sleep_until(&self, deadline: Timestamp) {
		self.offchain()
			.expect("sleep_until can be called only in the offchain worker context")
			.sleep_until(deadline)
	}

	/// Returns a random seed.
	///
	/// This is a trully random non deterministic seed generated by host environment.
	/// Obviously fine in the off-chain worker context.
	fn random_seed(&self) -> [u8; 32] {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.random_seed()
	}

	/// Sets a value in the local storage.
	///
	/// Note this storage is not part of the consensus, it's only accessible by
	/// offchain worker tasks running on the same machine. It IS persisted between runs.
	fn local_storage_set(&self, kind: StorageKind, key: &[u8], value: &[u8]) {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.local_storage_set(kind, key, value)
	}

	/// Sets a value in the local storage if it matches current value.
	///
	/// Since multiple offchain workers may be running concurrently, to prevent
	/// data races use CAS to coordinate between them.
	///
	/// Returns `true` if the value has been set, `false` otherwise.
	///
	/// Note this storage is not part of the consensus, it's only accessible by
	/// offchain worker tasks running on the same machine. It IS persisted between runs.
	fn local_storage_compare_and_set(
		&self,
		kind: StorageKind,
		key: &[u8],
		old_value: Option<&[u8]>,
		new_value: &[u8],
	) -> bool {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.local_storage_compare_and_set(kind, key, old_value, new_value)
	}

	/// Gets a value from the local storage.
	///
	/// If the value does not exist in the storage `None` will be returned.
	/// Note this storage is not part of the consensus, it's only accessible by
	/// offchain worker tasks running on the same machine. It IS persisted between runs.
	fn local_storage_get(&self, kind: StorageKind, key: &[u8]) -> Option<Vec<u8>> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.local_storage_get(kind, key)
	}

	/// Initiates a http request given HTTP verb and the URL.
	///
	/// Meta is a future-reserved field containing additional, parity-scale-codec encoded parameters.
	/// Returns the id of newly started request.
	fn http_request_start(
		&self,
		method: &str,
		uri: &str,
		meta: &[u8],
	) -> Result<HttpRequestId, ()> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_request_start(method, uri, meta)
	}

	/// Append header to the request.
	fn http_request_add_header(
		&self,
		request_id: HttpRequestId,
		name: &str,
		value: &str,
	) -> Result<(), ()> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_request_add_header(request_id, name, value)
	}

	/// Write a chunk of request body.
	///
	/// Writing an empty chunks finalises the request.
	/// Passing `None` as deadline blocks forever.
	///
	/// Returns an error in case deadline is reached or the chunk couldn't be written.
	fn http_request_write_body(
		&self,
		request_id: HttpRequestId,
		chunk: &[u8],
		deadline: Option<Timestamp>,
	) -> Result<(), HttpError> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_request_write_body(request_id, chunk, deadline)
	}

	/// Block and wait for the responses for given requests.
	///
	/// Returns a vector of request statuses (the len is the same as ids).
	/// Note that if deadline is not provided the method will block indefinitely,
	/// otherwise unready responses will produce `DeadlineReached` status.
	///
	/// Passing `None` as deadline blocks forever.
	fn http_response_wait(
		&self,
		ids: &[HttpRequestId],
		deadline: Option<Timestamp>,
	) -> Vec<HttpRequestStatus> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_response_wait(ids, deadline)
	}

	/// Read all response headers.
	///
	/// Returns a vector of pairs `(HeaderKey, HeaderValue)`.
	/// NOTE response headers have to be read before response body.
	fn http_response_headers(&self, request_id: HttpRequestId) -> Vec<(Vec<u8>, Vec<u8>)> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_response_headers(request_id)
	}

	/// Read a chunk of body response to given buffer.
	///
	/// Returns the number of bytes written or an error in case a deadline
	/// is reached or server closed the connection.
	/// If `0` is returned it means that the response has been fully consumed
	/// and the `request_id` is now invalid.
	/// NOTE this implies that response headers must be read before draining the body.
	/// Passing `None` as a deadline blocks forever.
	fn http_response_read_body(
		&self,
		request_id: HttpRequestId,
		buffer: &mut [u8],
		deadline: Option<Timestamp>,
	) -> Result<u32, HttpError> {
		self.offchain()
			.expect("random_seed can be called only in the offchain worker context")
			.http_response_read_body(request_id, buffer, deadline)
			.map(|r| r as u32)
	}
}


mod imp {
	use super::*;

	#[cfg(feature = "std")]
	include!("../with_std.rs");

	#[cfg(not(feature = "std"))]
	include!("../without_std.rs");
}

#[cfg(feature = "std")]
pub use self::imp::{StorageOverlay, ChildrenStorageOverlay, with_storage};
#[cfg(not(feature = "std"))]
pub use self::imp::ext::*;

/// Type alias for Externalities implementation used in tests.
#[cfg(feature = "std")]
pub type TestExternalities = self::imp::TestExternalities<primitives::Blake2Hasher, u64>;
