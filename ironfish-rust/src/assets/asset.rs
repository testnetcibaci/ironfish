/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use crate::{errors::IronfishError, keys::PUBLIC_ADDRESS_SIZE, util::str_to_array, PublicAddress};
use byteorder::{ReadBytesExt, WriteBytesExt};
use ironfish_zkp::constants::{ASSET_ID_LENGTH, ASSET_ID_PERSONALIZATION, GH_FIRST_BLOCK};
use jubjub::{ExtendedPoint, SubgroupPoint};
use std::io;

use super::asset_identifier::AssetIdentifier;

pub const NAME_LENGTH: usize = 32;
pub const METADATA_LENGTH: usize = 96;
pub const ASSET_LENGTH: usize = NAME_LENGTH + PUBLIC_ADDRESS_SIZE + METADATA_LENGTH + 1;
pub const ID_LENGTH: usize = ASSET_ID_LENGTH;

/// Describes all the fields necessary for creating and transacting with an
/// asset on the Iron Fish network
#[derive(Clone, Copy, Debug)]
pub struct Asset {
    /// Name of the asset
    pub(crate) name: [u8; NAME_LENGTH],

    /// Metadata fields for the asset (ex. chain, network, token identifier)
    pub(crate) metadata: [u8; METADATA_LENGTH],

    /// The owner who created the asset. Has permissions to mint
    pub(crate) owner: PublicAddress,

    /// The random byte used to ensure we get a valid asset identifier
    pub(crate) nonce: u8,

    /// The byte representation of a blake2s hash of the asset info
    pub(crate) id: AssetIdentifier,
}

impl Asset {
    /// Create a new AssetType from a public address, name, chain, and network
    pub fn new(owner: PublicAddress, name: &str, metadata: &str) -> Result<Asset, IronfishError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(IronfishError::InvalidData);
        }

        let name_bytes = str_to_array(trimmed_name);
        let metadata_bytes = str_to_array(metadata);

        let mut nonce = 0u8;
        loop {
            if let Ok(asset) = Asset::new_with_nonce(owner, name_bytes, metadata_bytes, nonce) {
                return Ok(asset);
            }
            nonce = nonce.checked_add(1).ok_or(IronfishError::RandomnessError)?;
        }
    }

    pub fn new_with_nonce(
        owner: PublicAddress,
        name: [u8; NAME_LENGTH],
        metadata: [u8; METADATA_LENGTH],
        nonce: u8,
    ) -> Result<Asset, IronfishError> {
        // Create the potential asset identifier from the asset info
        let asset_id_hash = blake2s_simd::Params::new()
            .hash_length(ASSET_ID_LENGTH)
            .personal(ASSET_ID_PERSONALIZATION)
            .to_state()
            .update(GH_FIRST_BLOCK)
            .update(&owner.public_address())
            .update(&name)
            .update(&metadata)
            .update(std::slice::from_ref(&nonce))
            .finalize();

        // Try creating an asset identifier from this hash
        let asset_id = AssetIdentifier::new(asset_id_hash.as_array().to_owned())?;

        // If the asset id is valid, this asset is valid
        Ok(Asset {
            owner,
            name,
            metadata,
            nonce,
            id: asset_id,
        })
    }

    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn nonce(&self) -> u8 {
        self.nonce
    }

    pub fn owner(&self) -> [u8; PUBLIC_ADDRESS_SIZE] {
        self.owner.public_address()
    }

    pub fn id(&self) -> &AssetIdentifier {
        &self.id
    }

    pub fn asset_generator(&self) -> ExtendedPoint {
        self.id.asset_generator()
    }

    pub fn value_commitment_generator(&self) -> SubgroupPoint {
        self.id.value_commitment_generator()
    }

    pub fn read<R: io::Read>(mut reader: R) -> Result<Self, IronfishError> {
        let owner = PublicAddress::read(&mut reader)?;

        let mut name = [0; NAME_LENGTH];
        reader.read_exact(&mut name[..])?;

        let mut metadata = [0; METADATA_LENGTH];
        reader.read_exact(&mut metadata[..])?;

        let nonce = reader.read_u8()?;

        Asset::new_with_nonce(owner, name, metadata, nonce)
    }

    /// Stow the bytes of this struct in the given writer.
    pub fn write<W: io::Write>(&self, mut writer: W) -> Result<(), IronfishError> {
        self.owner.write(&mut writer)?;
        writer.write_all(&self.name)?;
        writer.write_all(&self.metadata)?;
        writer.write_u8(self.nonce)?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{util::str_to_array, PublicAddress, SaplingKey};

    use super::Asset;

    #[test]
    fn test_asset_new() {
        let key = SaplingKey::generate_key();
        let owner = key.public_address();
        let name = "name";
        let metadata = "{ 'token_identifier': '0x123' }";

        let asset = Asset::new(owner, name, metadata).expect("can create an asset");

        assert_eq!(asset.owner, owner);
        assert_eq!(asset.name, str_to_array(name));
        assert_eq!(asset.metadata, str_to_array(metadata));
    }

    #[test]
    fn test_asset_name_must_be_set() {
        let key = SaplingKey::generate_key();
        let owner = key.public_address();
        let metadata = "";

        let bad_asset1 = Asset::new(owner, "", metadata);
        assert!(bad_asset1.is_err());

        let bad_asset2 = Asset::new(owner, "   ", metadata);
        assert!(bad_asset2.is_err());

        let good_asset = Asset::new(owner, "foo", metadata);
        assert!(good_asset.is_ok());
    }

    #[test]
    fn test_asset_new_with_nonce() {
        let public_address = [
            81, 229, 109, 20, 111, 174, 52, 91, 120, 215, 34, 107, 174, 123, 78, 102, 189, 188,
            226, 7, 173, 7, 76, 135, 130, 203, 71, 131, 62, 219, 240, 68,
        ];
        let owner = PublicAddress::new(&public_address).unwrap();

        let name = str_to_array("name");
        let metadata = str_to_array("{ 'token_identifier': '0x123' }");
        let nonce = 1;

        let asset =
            Asset::new_with_nonce(owner, name, metadata, nonce).expect("can create an asset");

        assert_eq!(asset.owner, owner);
        assert_eq!(asset.name, name);
        assert_eq!(asset.metadata, metadata);
    }

    #[test]
    fn test_asset_new_with_nonce_invalid_nonce() {
        let nonce = 7;
        let public_address = [
            81, 229, 109, 20, 111, 174, 52, 91, 120, 215, 34, 107, 174, 123, 78, 102, 189, 188,
            226, 7, 173, 7, 76, 135, 130, 203, 71, 131, 62, 219, 240, 68,
        ];
        let owner = PublicAddress::new(&public_address).unwrap();

        let name = str_to_array("name");
        let metadata = str_to_array("{ 'token_identifier': '0x123' }");

        let asset_res = Asset::new_with_nonce(owner, name, metadata, nonce);

        assert!(asset_res.is_err());
    }
}
