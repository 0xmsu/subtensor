use super::*;
use frame_support::{
    pallet_prelude::{Identity, OptionQuery},
    storage_alias,
    traits::{DefensiveResult, Get},
    weights::Weight,
};
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

// TODO (camfairchild): TEST MIGRATION

/// Module containing deprecated storage format for LoadedEmission
pub mod deprecated_loaded_emission_format {
    use super::*;

    #[storage_alias]
    pub(super) type LoadedEmission<T: Config> =
        StorageMap<Pallet<T>, Identity, u16, Vec<(AccountIdOf<T>, u64)>, OptionQuery>;
}

/// Migrates the storage to create the root network
///
/// This function performs the following steps:
/// 1. Checks if the root network already exists
/// 2. If not, creates the root network with default settings
/// 3. Removes all existing senate members
///
/// # Arguments
///
/// * `T` - The Config trait of the pallet
///
/// # Returns
///
/// * `Weight` - The computational weight of this operation
///
/// # Example
///
/// ```ignore
/// let weight = migrate_create_root_network::<Runtime>();
/// ```
pub fn migrate_create_root_network<T: Config>() -> Weight {
    // Initialize weight counter
    let mut weight = T::DbWeight::get().reads(1);

    // Check if root network already exists
    if NetworksAdded::<T>::get(NetUid::ROOT) {
        // Return early if root network already exists
        return weight;
    }

    // Set the root network as added
    NetworksAdded::<T>::insert(NetUid::ROOT, true);

    // Increment the total number of networks
    TotalNetworks::<T>::mutate(|n| *n = n.saturating_add(1));

    // Set the maximum number of UIDs to the number of senate members
    MaxAllowedUids::<T>::insert(NetUid::ROOT, 64);

    // Set the maximum number of validators to all members
    MaxAllowedValidators::<T>::insert(NetUid::ROOT, 64);

    // Set the minimum allowed weights to zero (no weight restrictions)
    MinAllowedWeights::<T>::insert(NetUid::ROOT, 0);

    // Set the maximum weight limit to u16::MAX (no weight restrictions)
    MaxWeightsLimit::<T>::insert(NetUid::ROOT, u16::MAX);

    // Set default root tempo
    Tempo::<T>::insert(NetUid::ROOT, 100);

    // Set the root network as open for registration
    NetworkRegistrationAllowed::<T>::insert(NetUid::ROOT, true);

    // Set target registrations for validators as 1 per block
    TargetRegistrationsPerInterval::<T>::insert(NetUid::ROOT, 1);

    // TODO: Consider if WeightsSetRateLimit should be set
    // WeightsSetRateLimit::<T>::insert(NetUid::ROOT, 7200);

    // Accrue weight for database writes
    weight.saturating_accrue(T::DbWeight::get().writes(8));

    // Remove all existing senate members
    for hotkey_i in T::SenateMembers::members().iter() {
        // Remove votes associated with the member
        T::TriumvirateInterface::remove_votes(hotkey_i).defensive_ok();
        // Remove the member from the senate
        T::SenateMembers::remove_member(hotkey_i).defensive_ok();

        // Accrue weight for database operations
        weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));
    }

    log::info!("Migrated create root network");
    weight
}
