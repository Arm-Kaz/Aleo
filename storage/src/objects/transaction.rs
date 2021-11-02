// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkOS library.

// The snarkOS library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkOS library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkOS library. If not, see <https://www.gnu.org/licenses/>.

use anyhow::*;
use std::io::Result as IoResult;

use smallvec::SmallVec;
use snarkvm_dpc::{
    testnet1::{Testnet1Components, Transaction},
    AleoAmount,
    Network,
    TransactionScheme,
};
use snarkvm_utilities::{CanonicalDeserialize, CanonicalSerialize, FromBytes, ToBytes, Write};

use crate::{Digest, SerialRecord};

pub type TransactionId = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialTransaction {
    pub id: TransactionId,

    /// The network this transaction is included in
    pub network: Network,

    /// The root of the ledger commitment Merkle tree
    pub ledger_digest: Digest,

    /// The serial numbers of the records being spent
    pub old_serial_numbers: Vec<Digest>,

    pub new_commitments: Vec<Digest>,

    /// The commitment to the old record death and new record birth programs
    pub program_commitment: Digest,

    /// The root of the local data merkle tree
    pub local_data_root: Digest,

    /// A transaction value balance is the difference between input and output record balances.
    /// This value effectively becomes the transaction fee for the miner. Only coinbase transactions
    /// can have a negative value balance representing tokens being minted.
    pub value_balance: AleoAmount,

    /// Randomized signatures that allow for authorized delegation of transaction generation
    pub signatures: Vec<Digest>,

    /// Encrypted record and selector bits of the new records generated by the transaction
    pub new_records: Vec<Vec<u8>>,

    /// Zero-knowledge proof attesting to the valididty of the transaction
    pub transaction_proof: Vec<u8>,

    /// Public data associated with the transaction that must be unique among all transactions
    pub memorandum: Digest,

    /// The ID of the inner SNARK being used
    pub inner_circuit_id: Digest,
}

impl SerialTransaction {
    pub fn size(&self) -> usize {
        use std::mem::size_of;

        size_of::<SerialTransaction>()
            + size_of::<Digest>() * (self.old_serial_numbers.len() + self.new_commitments.len() + self.signatures.len())
            + size_of::<SerialRecord>() * self.new_records.len()
            + self.new_records.iter().map(|x| x.len()).sum::<usize>()
            + self.transaction_proof.len()
    }
}

impl ToBytes for SerialTransaction {
    fn write_le<W: Write>(&self, mut writer: W) -> IoResult<()> {
        for serial_number in &self.old_serial_numbers {
            writer.write_all(&serial_number[..])?;
        }
        for commitment in &self.new_commitments {
            writer.write_all(&commitment[..])?;
        }
        writer.write_all(&self.memorandum[..])?;
        writer.write_all(&self.ledger_digest[..])?;
        writer.write_all(&self.inner_circuit_id[..])?;
        writer.write_all(&self.transaction_proof[..])?;
        writer.write_all(&self.program_commitment[..])?;
        writer.write_all(&self.local_data_root[..])?;
        self.value_balance.write_le(&mut writer)?;
        self.network.write_le(&mut writer)?;
        for signature in &self.signatures {
            writer.write_all(&signature[..])?;
        }
        for record in &self.new_records {
            record.write_le(&mut writer)?;
        }
        Ok(())
    }
}

pub trait VMTransaction: Sized {
    fn deserialize(tx: &SerialTransaction) -> IoResult<Self>;

    fn serialize(&self) -> Result<SerialTransaction>;
}

fn serialize_digest<B: ToBytes>(bytes: &B) -> IoResult<Digest> {
    let mut out = SmallVec::new();
    bytes.write_le(&mut out)?;
    Ok(Digest(out))
}

fn serialize_bytes<B: ToBytes>(bytes: &B) -> IoResult<Vec<u8>> {
    let mut out = vec![];
    bytes.write_le(&mut out)?;
    Ok(out)
}

fn serialize_many_digests<B: ToBytes>(items: &[B]) -> IoResult<Vec<Digest>> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(serialize_digest(item)?);
    }
    Ok(out)
}

fn deserialize_bytes<B: FromBytes, D: AsRef<[u8]>>(bytes: D) -> IoResult<B> {
    B::read_le(&mut bytes.as_ref())
}

fn deserialize_many_bytes<B: FromBytes, D: AsRef<[u8]>>(bytes: impl IntoIterator<Item = D>) -> IoResult<Vec<B>> {
    bytes.into_iter().map(deserialize_bytes).collect::<IoResult<Vec<_>>>()
}

impl<T: Testnet1Components> VMTransaction for Transaction<T> {
    fn deserialize(tx: &SerialTransaction) -> IoResult<Self> {
        let mut old_serial_numbers = Vec::with_capacity(tx.old_serial_numbers.len());
        for serial in &tx.old_serial_numbers {
            let digest = CanonicalDeserialize::deserialize(&mut &serial[..])?;
            old_serial_numbers.push(digest);
        }

        Ok(Transaction {
            network: tx.network,
            ledger_digest: deserialize_bytes(&tx.ledger_digest)?,
            old_serial_numbers,
            new_commitments: deserialize_many_bytes(&tx.new_commitments)?,
            program_commitment: deserialize_bytes(&tx.program_commitment)?,
            local_data_root: deserialize_bytes(&tx.local_data_root)?,
            value_balance: tx.value_balance,
            signatures: deserialize_many_bytes(&tx.signatures)?,
            encrypted_records: deserialize_many_bytes(&tx.new_records)?,
            transaction_proof: deserialize_bytes(&tx.transaction_proof)?,
            memorandum: deserialize_bytes(&tx.memorandum)?,
            inner_circuit_id: deserialize_bytes(&tx.inner_circuit_id)?,
        })
    }

    fn serialize(&self) -> Result<SerialTransaction> {
        let mut old_serial_numbers = Vec::with_capacity(self.old_serial_numbers.len());
        for serial in &self.old_serial_numbers {
            let mut digest = Digest::default();
            CanonicalSerialize::serialize(serial, &mut digest.0)?;
            old_serial_numbers.push(digest);
        }
        Ok(SerialTransaction {
            id: self.transaction_id().unwrap(),
            network: self.network,
            ledger_digest: serialize_digest(&self.ledger_digest)?,
            old_serial_numbers,
            new_commitments: serialize_many_digests(&self.new_commitments)?,
            program_commitment: serialize_digest(&self.program_commitment)?,
            local_data_root: serialize_digest(&self.local_data_root)?,
            value_balance: self.value_balance,
            signatures: serialize_many_digests(&self.signatures)?,
            new_records: self
                .encrypted_records
                .iter()
                .map(|record| {
                    let mut out = vec![];
                    record.write_le(&mut out)?;
                    Ok(out)
                })
                .collect::<Result<_>>()?,
            transaction_proof: serialize_bytes(&self.transaction_proof)?,
            memorandum: self.memorandum.into(),
            inner_circuit_id: serialize_digest(&self.inner_circuit_id)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    use snarkvm_algorithms::SignatureScheme;
    use snarkvm_dpc::{
        testnet1::{instantiated::Components, Transaction},
        DPCComponents,
        TransactionScheme,
    };
    use snarkvm_utilities::to_bytes_le;

    type DPCTransaction = Transaction<Components>;

    #[test]
    fn transaction_round_trip() {
        // one fp256 + length and selector flags
        let mut record = vec![0u8; 1 + 1 + 32];
        record[0] = 1u8;
        let test_serial_signature =
            <<Components as DPCComponents>::AccountSignature as SignatureScheme>::setup(&mut thread_rng()).unwrap();
        let test_serial_private = test_serial_signature.generate_private_key(&mut thread_rng()).unwrap();
        let test_serial_public = test_serial_signature.generate_public_key(&test_serial_private).unwrap();
        let mut test_serial = vec![];
        CanonicalSerialize::serialize(&test_serial_public, &mut test_serial).unwrap();

        let mut base_transaction = SerialTransaction {
            id: [0u8; 32],
            network: snarkvm_dpc::Network::Testnet1,
            ledger_digest: [0u8; 32].into(),
            old_serial_numbers: vec![test_serial[..].into(), test_serial[..].into()],
            new_commitments: vec![[3u8; 32].into(), [5u8; 32].into()],

            new_records: vec![record.clone(), record],
            program_commitment: [7u8; 32].into(),
            local_data_root: [8u8; 32].into(),
            value_balance: AleoAmount(1000),
            signatures: vec![[0u8; 64].into()],
            transaction_proof: vec![1u8; 579],
            memorandum: [10u8; 32].into(),
            inner_circuit_id: [1u8; 48].into(),
        };

        let deserialized = DPCTransaction::deserialize(&base_transaction).unwrap();
        base_transaction.id = deserialized.transaction_id().unwrap();

        let reserialized = deserialized.serialize().unwrap();

        assert_eq!(base_transaction, reserialized);

        assert_eq!(
            to_bytes_le![base_transaction].unwrap(),
            to_bytes_le![deserialized].unwrap()
        );
    }
}