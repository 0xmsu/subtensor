use super::*;
use crate::{
    Error,
    system::{ensure_root, ensure_signed, ensure_signed_or_root, pallet_prelude::BlockNumberFor},
};
use safe_math::*;
use sp_core::Get;
use sp_core::U256;
use sp_runtime::Saturating;
use substrate_fixed::types::{I32F32, U96F32};
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    pub fn ensure_subnet_owner_or_root(
        o: T::RuntimeOrigin,
        netuid: NetUid,
    ) -> Result<(), DispatchError> {
        let coldkey = ensure_signed_or_root(o);
        match coldkey {
            Ok(Some(who)) if SubnetOwner::<T>::get(netuid) == who => Ok(()),
            Ok(Some(_)) => Err(DispatchError::BadOrigin),
            Ok(None) => Ok(()),
            Err(x) => Err(x.into()),
        }
    }

    pub fn ensure_subnet_owner(o: T::RuntimeOrigin, netuid: NetUid) -> Result<(), DispatchError> {
        let coldkey = ensure_signed(o);
        match coldkey {
            Ok(who) if SubnetOwner::<T>::get(netuid) == who => Ok(()),
            Ok(_) => Err(DispatchError::BadOrigin),
            Err(x) => Err(x.into()),
        }
    }

    // ========================
    // ==== Global Setters ====
    // ========================
    pub fn set_tempo(netuid: NetUid, tempo: u16) {
        Tempo::<T>::insert(netuid, tempo);
        Self::deposit_event(Event::TempoSet(netuid, tempo));
    }
    pub fn set_last_adjustment_block(netuid: NetUid, last_adjustment_block: u64) {
        LastAdjustmentBlock::<T>::insert(netuid, last_adjustment_block);
    }
    pub fn set_blocks_since_last_step(netuid: NetUid, blocks_since_last_step: u64) {
        BlocksSinceLastStep::<T>::insert(netuid, blocks_since_last_step);
    }
    pub fn set_registrations_this_block(netuid: NetUid, registrations_this_block: u16) {
        RegistrationsThisBlock::<T>::insert(netuid, registrations_this_block);
    }
    pub fn set_last_mechanism_step_block(netuid: NetUid, last_mechanism_step_block: u64) {
        LastMechansimStepBlock::<T>::insert(netuid, last_mechanism_step_block);
    }
    pub fn set_registrations_this_interval(netuid: NetUid, registrations_this_interval: u16) {
        RegistrationsThisInterval::<T>::insert(netuid, registrations_this_interval);
    }
    pub fn set_pow_registrations_this_interval(
        netuid: NetUid,
        pow_registrations_this_interval: u16,
    ) {
        POWRegistrationsThisInterval::<T>::insert(netuid, pow_registrations_this_interval);
    }
    pub fn set_burn_registrations_this_interval(
        netuid: NetUid,
        burn_registrations_this_interval: u16,
    ) {
        BurnRegistrationsThisInterval::<T>::insert(netuid, burn_registrations_this_interval);
    }

    // ========================
    // ==== Global Getters ====
    // ========================
    pub fn get_total_issuance() -> u64 {
        TotalIssuance::<T>::get()
    }
    pub fn get_current_block_as_u64() -> u64 {
        TryInto::try_into(<frame_system::Pallet<T>>::block_number())
            .ok()
            .expect("blockchain will not exceed 2^64 blocks; QED.")
    }

    // ==============================
    // ==== YumaConsensus params ====
    // ==============================
    pub fn get_rank(netuid: NetUid) -> Vec<u16> {
        Rank::<T>::get(netuid)
    }
    pub fn get_trust(netuid: NetUid) -> Vec<u16> {
        Trust::<T>::get(netuid)
    }
    pub fn get_active(netuid: NetUid) -> Vec<bool> {
        Active::<T>::get(netuid)
    }
    pub fn get_emission(netuid: NetUid) -> Vec<u64> {
        Emission::<T>::get(netuid)
    }
    pub fn get_consensus(netuid: NetUid) -> Vec<u16> {
        Consensus::<T>::get(netuid)
    }
    pub fn get_incentive(netuid: NetUid) -> Vec<u16> {
        Incentive::<T>::get(netuid)
    }
    pub fn get_dividends(netuid: NetUid) -> Vec<u16> {
        Dividends::<T>::get(netuid)
    }
    pub fn get_last_update(netuid: NetUid) -> Vec<u64> {
        LastUpdate::<T>::get(netuid)
    }
    pub fn get_pruning_score(netuid: NetUid) -> Vec<u16> {
        PruningScores::<T>::get(netuid)
    }
    pub fn get_validator_trust(netuid: NetUid) -> Vec<u16> {
        ValidatorTrust::<T>::get(netuid)
    }
    pub fn get_validator_permit(netuid: NetUid) -> Vec<bool> {
        ValidatorPermit::<T>::get(netuid)
    }

    // ==================================
    // ==== YumaConsensus UID params ====
    // ==================================
    pub fn set_last_update_for_uid(netuid: NetUid, uid: u16, last_update: u64) {
        let mut updated_last_update_vec = Self::get_last_update(netuid);
        let Some(updated_last_update) = updated_last_update_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_last_update = last_update;
        LastUpdate::<T>::insert(netuid, updated_last_update_vec);
    }
    pub fn set_active_for_uid(netuid: NetUid, uid: u16, active: bool) {
        let mut updated_active_vec = Self::get_active(netuid);
        let Some(updated_active) = updated_active_vec.get_mut(uid as usize) else {
            return;
        };
        *updated_active = active;
        Active::<T>::insert(netuid, updated_active_vec);
    }
    pub fn set_pruning_score_for_uid(netuid: NetUid, uid: u16, pruning_score: u16) {
        log::debug!("netuid = {:?}", netuid);
        log::debug!(
            "SubnetworkN::<T>::get( netuid ) = {:?}",
            SubnetworkN::<T>::get(netuid)
        );
        log::debug!("uid = {:?}", uid);
        assert!(uid < SubnetworkN::<T>::get(netuid));
        PruningScores::<T>::mutate(netuid, |v| {
            if let Some(s) = v.get_mut(uid as usize) {
                *s = pruning_score;
            }
        });
    }
    pub fn set_validator_permit_for_uid(netuid: NetUid, uid: u16, validator_permit: bool) {
        let mut updated_validator_permits = Self::get_validator_permit(netuid);
        let Some(updated_validator_permit) = updated_validator_permits.get_mut(uid as usize) else {
            return;
        };
        *updated_validator_permit = validator_permit;
        ValidatorPermit::<T>::insert(netuid, updated_validator_permits);
    }
    pub fn set_stake_threshold(min_stake: u64) {
        StakeThreshold::<T>::put(min_stake);
        Self::deposit_event(Event::StakeThresholdSet(min_stake));
    }

    pub fn get_rank_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Rank::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_trust_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Trust::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_emission_for_uid(netuid: NetUid, uid: u16) -> u64 {
        let vec = Emission::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_active_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = Active::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_consensus_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Consensus::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_incentive_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Incentive::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_dividends_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = Dividends::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_last_update_for_uid(netuid: NetUid, uid: u16) -> u64 {
        let vec = LastUpdate::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_pruning_score_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = PruningScores::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(u16::MAX)
    }
    pub fn get_validator_trust_for_uid(netuid: NetUid, uid: u16) -> u16 {
        let vec = ValidatorTrust::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(0)
    }
    pub fn get_validator_permit_for_uid(netuid: NetUid, uid: u16) -> bool {
        let vec = ValidatorPermit::<T>::get(netuid);
        vec.get(uid as usize).copied().unwrap_or(false)
    }
    pub fn get_stake_threshold() -> u64 {
        StakeThreshold::<T>::get()
    }

    // ============================
    // ==== Subnetwork Getters ====
    // ============================
    pub fn get_tempo(netuid: NetUid) -> u16 {
        Tempo::<T>::get(netuid)
    }
    pub fn get_pending_emission(netuid: NetUid) -> u64 {
        PendingEmission::<T>::get(netuid)
    }
    pub fn get_last_adjustment_block(netuid: NetUid) -> u64 {
        LastAdjustmentBlock::<T>::get(netuid)
    }
    pub fn get_blocks_since_last_step(netuid: NetUid) -> u64 {
        BlocksSinceLastStep::<T>::get(netuid)
    }
    pub fn get_difficulty(netuid: NetUid) -> U256 {
        U256::from(Self::get_difficulty_as_u64(netuid))
    }
    pub fn get_registrations_this_block(netuid: NetUid) -> u16 {
        RegistrationsThisBlock::<T>::get(netuid)
    }
    pub fn get_last_mechanism_step_block(netuid: NetUid) -> u64 {
        LastMechansimStepBlock::<T>::get(netuid)
    }
    pub fn get_registrations_this_interval(netuid: NetUid) -> u16 {
        RegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_pow_registrations_this_interval(netuid: NetUid) -> u16 {
        POWRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_burn_registrations_this_interval(netuid: NetUid) -> u16 {
        BurnRegistrationsThisInterval::<T>::get(netuid)
    }
    pub fn get_neuron_block_at_registration(netuid: NetUid, neuron_uid: u16) -> u64 {
        BlockAtRegistration::<T>::get(netuid, neuron_uid)
    }

    // ========================
    // ===== Take checks ======
    // ========================
    pub fn do_take_checks(coldkey: &T::AccountId, hotkey: &T::AccountId) -> Result<(), Error<T>> {
        // Ensure we are delegating a known key.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the coldkey is the owner.
        ensure!(
            Self::coldkey_owns_hotkey(coldkey, hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        Ok(())
    }

    // ========================
    // === Token Management ===
    // ========================
    pub fn burn_tokens(amount: u64) {
        TotalIssuance::<T>::put(TotalIssuance::<T>::get().saturating_sub(amount));
    }
    pub fn coinbase(amount: u64) {
        TotalIssuance::<T>::put(TotalIssuance::<T>::get().saturating_add(amount));
    }

    pub fn set_subnet_locked_balance(netuid: NetUid, amount: u64) {
        SubnetLocked::<T>::insert(netuid, amount);
    }
    pub fn get_subnet_locked_balance(netuid: NetUid) -> u64 {
        SubnetLocked::<T>::get(netuid)
    }
    pub fn get_total_subnet_locked() -> u64 {
        let mut total_subnet_locked: u64 = 0;
        for (_, locked) in SubnetLocked::<T>::iter() {
            total_subnet_locked.saturating_accrue(locked);
        }
        total_subnet_locked
    }

    // ========================
    // ========= Sudo =========
    // ========================

    // Configure tx rate limiting
    pub fn get_tx_rate_limit() -> u64 {
        TxRateLimit::<T>::get()
    }
    pub fn set_tx_rate_limit(tx_rate_limit: u64) {
        TxRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxRateLimitSet(tx_rate_limit));
    }
    pub fn get_tx_delegate_take_rate_limit() -> u64 {
        TxDelegateTakeRateLimit::<T>::get()
    }
    pub fn set_tx_delegate_take_rate_limit(tx_rate_limit: u64) {
        TxDelegateTakeRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxDelegateTakeRateLimitSet(tx_rate_limit));
    }
    pub fn set_min_delegate_take(take: u16) {
        MinDelegateTake::<T>::put(take);
        Self::deposit_event(Event::MinDelegateTakeSet(take));
    }
    pub fn set_max_delegate_take(take: u16) {
        MaxDelegateTake::<T>::put(take);
        Self::deposit_event(Event::MaxDelegateTakeSet(take));
    }
    pub fn get_min_delegate_take() -> u16 {
        MinDelegateTake::<T>::get()
    }
    pub fn get_max_delegate_take() -> u16 {
        MaxDelegateTake::<T>::get()
    }
    pub fn get_default_delegate_take() -> u16 {
        // Default to maximum
        MaxDelegateTake::<T>::get()
    }
    // get_default_childkey_take
    pub fn get_default_childkey_take() -> u16 {
        // Default to maximum
        MinChildkeyTake::<T>::get()
    }
    pub fn get_tx_childkey_take_rate_limit() -> u64 {
        TxChildkeyTakeRateLimit::<T>::get()
    }
    pub fn set_tx_childkey_take_rate_limit(tx_rate_limit: u64) {
        TxChildkeyTakeRateLimit::<T>::put(tx_rate_limit);
        Self::deposit_event(Event::TxChildKeyTakeRateLimitSet(tx_rate_limit));
    }
    pub fn set_min_childkey_take(take: u16) {
        MinChildkeyTake::<T>::put(take);
        Self::deposit_event(Event::MinChildKeyTakeSet(take));
    }
    pub fn set_max_childkey_take(take: u16) {
        MaxChildkeyTake::<T>::put(take);
        Self::deposit_event(Event::MaxChildKeyTakeSet(take));
    }
    pub fn get_min_childkey_take() -> u16 {
        MinChildkeyTake::<T>::get()
    }

    pub fn get_max_childkey_take() -> u16 {
        MaxChildkeyTake::<T>::get()
    }

    pub fn get_serving_rate_limit(netuid: NetUid) -> u64 {
        ServingRateLimit::<T>::get(netuid)
    }
    pub fn set_serving_rate_limit(netuid: NetUid, serving_rate_limit: u64) {
        ServingRateLimit::<T>::insert(netuid, serving_rate_limit);
        Self::deposit_event(Event::ServingRateLimitSet(netuid, serving_rate_limit));
    }

    pub fn get_min_difficulty(netuid: NetUid) -> u64 {
        MinDifficulty::<T>::get(netuid)
    }
    pub fn set_min_difficulty(netuid: NetUid, min_difficulty: u64) {
        MinDifficulty::<T>::insert(netuid, min_difficulty);
        Self::deposit_event(Event::MinDifficultySet(netuid, min_difficulty));
    }

    pub fn get_max_difficulty(netuid: NetUid) -> u64 {
        MaxDifficulty::<T>::get(netuid)
    }
    pub fn set_max_difficulty(netuid: NetUid, max_difficulty: u64) {
        MaxDifficulty::<T>::insert(netuid, max_difficulty);
        Self::deposit_event(Event::MaxDifficultySet(netuid, max_difficulty));
    }

    pub fn get_weights_version_key(netuid: NetUid) -> u64 {
        WeightsVersionKey::<T>::get(netuid)
    }
    pub fn set_weights_version_key(netuid: NetUid, weights_version_key: u64) {
        WeightsVersionKey::<T>::insert(netuid, weights_version_key);
        Self::deposit_event(Event::WeightsVersionKeySet(netuid, weights_version_key));
    }

    pub fn get_weights_set_rate_limit(netuid: NetUid) -> u64 {
        WeightsSetRateLimit::<T>::get(netuid)
    }
    pub fn set_weights_set_rate_limit(netuid: NetUid, weights_set_rate_limit: u64) {
        WeightsSetRateLimit::<T>::insert(netuid, weights_set_rate_limit);
        Self::deposit_event(Event::WeightsSetRateLimitSet(
            netuid,
            weights_set_rate_limit,
        ));
    }

    pub fn get_adjustment_interval(netuid: NetUid) -> u16 {
        AdjustmentInterval::<T>::get(netuid)
    }
    pub fn set_adjustment_interval(netuid: NetUid, adjustment_interval: u16) {
        AdjustmentInterval::<T>::insert(netuid, adjustment_interval);
        Self::deposit_event(Event::AdjustmentIntervalSet(netuid, adjustment_interval));
    }

    pub fn get_adjustment_alpha(netuid: NetUid) -> u64 {
        AdjustmentAlpha::<T>::get(netuid)
    }
    pub fn set_adjustment_alpha(netuid: NetUid, adjustment_alpha: u64) {
        AdjustmentAlpha::<T>::insert(netuid, adjustment_alpha);
        Self::deposit_event(Event::AdjustmentAlphaSet(netuid, adjustment_alpha));
    }

    pub fn set_validator_prune_len(netuid: NetUid, validator_prune_len: u64) {
        ValidatorPruneLen::<T>::insert(netuid, validator_prune_len);
        Self::deposit_event(Event::ValidatorPruneLenSet(netuid, validator_prune_len));
    }

    pub fn get_scaling_law_power(netuid: NetUid) -> u16 {
        ScalingLawPower::<T>::get(netuid)
    }
    pub fn set_scaling_law_power(netuid: NetUid, scaling_law_power: u16) {
        ScalingLawPower::<T>::insert(netuid, scaling_law_power);
        Self::deposit_event(Event::ScalingLawPowerSet(netuid, scaling_law_power));
    }

    pub fn get_max_weight_limit(netuid: NetUid) -> u16 {
        MaxWeightsLimit::<T>::get(netuid)
    }
    pub fn set_max_weight_limit(netuid: NetUid, max_weight_limit: u16) {
        MaxWeightsLimit::<T>::insert(netuid, max_weight_limit);
        Self::deposit_event(Event::MaxWeightLimitSet(netuid, max_weight_limit));
    }

    pub fn get_immunity_period(netuid: NetUid) -> u16 {
        ImmunityPeriod::<T>::get(netuid)
    }
    pub fn set_immunity_period(netuid: NetUid, immunity_period: u16) {
        ImmunityPeriod::<T>::insert(netuid, immunity_period);
        Self::deposit_event(Event::ImmunityPeriodSet(netuid, immunity_period));
    }
    /// Check if a neuron is in immunity based on the current block
    pub fn get_neuron_is_immune(netuid: NetUid, uid: u16) -> bool {
        let registered_at = Self::get_neuron_block_at_registration(netuid, uid);
        let current_block = Self::get_current_block_as_u64();
        let immunity_period = Self::get_immunity_period(netuid);
        current_block.saturating_sub(registered_at) < u64::from(immunity_period)
    }

    pub fn get_min_allowed_weights(netuid: NetUid) -> u16 {
        MinAllowedWeights::<T>::get(netuid)
    }
    pub fn set_min_allowed_weights(netuid: NetUid, min_allowed_weights: u16) {
        MinAllowedWeights::<T>::insert(netuid, min_allowed_weights);
        Self::deposit_event(Event::MinAllowedWeightSet(netuid, min_allowed_weights));
    }

    pub fn get_max_allowed_uids(netuid: NetUid) -> u16 {
        MaxAllowedUids::<T>::get(netuid)
    }
    pub fn set_max_allowed_uids(netuid: NetUid, max_allowed: u16) {
        MaxAllowedUids::<T>::insert(netuid, max_allowed);
        Self::deposit_event(Event::MaxAllowedUidsSet(netuid, max_allowed));
    }

    pub fn get_kappa(netuid: NetUid) -> u16 {
        Kappa::<T>::get(netuid)
    }
    pub fn set_kappa(netuid: NetUid, kappa: u16) {
        Kappa::<T>::insert(netuid, kappa);
        Self::deposit_event(Event::KappaSet(netuid, kappa));
    }
    pub fn get_commit_reveal_weights_enabled(netuid: NetUid) -> bool {
        CommitRevealWeightsEnabled::<T>::get(netuid)
    }
    pub fn set_commit_reveal_weights_enabled(netuid: NetUid, enabled: bool) {
        CommitRevealWeightsEnabled::<T>::set(netuid, enabled);
        Self::deposit_event(Event::CommitRevealEnabled(netuid, enabled));
    }

    pub fn get_rho(netuid: NetUid) -> u16 {
        Rho::<T>::get(netuid)
    }
    pub fn set_rho(netuid: NetUid, rho: u16) {
        Rho::<T>::insert(netuid, rho);
    }

    pub fn get_activity_cutoff(netuid: NetUid) -> u16 {
        ActivityCutoff::<T>::get(netuid)
    }
    pub fn set_activity_cutoff(netuid: NetUid, activity_cutoff: u16) {
        ActivityCutoff::<T>::insert(netuid, activity_cutoff);
        Self::deposit_event(Event::ActivityCutoffSet(netuid, activity_cutoff));
    }

    // Registration Toggle utils
    pub fn get_network_registration_allowed(netuid: NetUid) -> bool {
        NetworkRegistrationAllowed::<T>::get(netuid)
    }
    pub fn set_network_registration_allowed(netuid: NetUid, registration_allowed: bool) {
        NetworkRegistrationAllowed::<T>::insert(netuid, registration_allowed);
        Self::deposit_event(Event::RegistrationAllowed(netuid, registration_allowed));
    }

    pub fn get_network_pow_registration_allowed(netuid: NetUid) -> bool {
        NetworkPowRegistrationAllowed::<T>::get(netuid)
    }
    pub fn set_network_pow_registration_allowed(netuid: NetUid, registration_allowed: bool) {
        NetworkPowRegistrationAllowed::<T>::insert(netuid, registration_allowed);
        Self::deposit_event(Event::PowRegistrationAllowed(netuid, registration_allowed));
    }

    pub fn get_target_registrations_per_interval(netuid: NetUid) -> u16 {
        TargetRegistrationsPerInterval::<T>::get(netuid)
    }
    pub fn set_target_registrations_per_interval(
        netuid: NetUid,
        target_registrations_per_interval: u16,
    ) {
        TargetRegistrationsPerInterval::<T>::insert(netuid, target_registrations_per_interval);
        Self::deposit_event(Event::RegistrationPerIntervalSet(
            netuid,
            target_registrations_per_interval,
        ));
    }

    pub fn get_burn_as_u64(netuid: NetUid) -> u64 {
        Burn::<T>::get(netuid)
    }
    pub fn set_burn(netuid: NetUid, burn: u64) {
        Burn::<T>::insert(netuid, burn);
    }

    pub fn get_min_burn_as_u64(netuid: NetUid) -> u64 {
        MinBurn::<T>::get(netuid)
    }
    pub fn set_min_burn(netuid: NetUid, min_burn: u64) {
        MinBurn::<T>::insert(netuid, min_burn);
        Self::deposit_event(Event::MinBurnSet(netuid, min_burn));
    }

    pub fn get_max_burn_as_u64(netuid: NetUid) -> u64 {
        MaxBurn::<T>::get(netuid)
    }
    pub fn set_max_burn(netuid: NetUid, max_burn: u64) {
        MaxBurn::<T>::insert(netuid, max_burn);
        Self::deposit_event(Event::MaxBurnSet(netuid, max_burn));
    }

    pub fn get_difficulty_as_u64(netuid: NetUid) -> u64 {
        Difficulty::<T>::get(netuid)
    }
    pub fn set_difficulty(netuid: NetUid, difficulty: u64) {
        Difficulty::<T>::insert(netuid, difficulty);
        Self::deposit_event(Event::DifficultySet(netuid, difficulty));
    }

    pub fn get_max_allowed_validators(netuid: NetUid) -> u16 {
        MaxAllowedValidators::<T>::get(netuid)
    }
    pub fn set_max_allowed_validators(netuid: NetUid, max_allowed_validators: u16) {
        MaxAllowedValidators::<T>::insert(netuid, max_allowed_validators);
        Self::deposit_event(Event::MaxAllowedValidatorsSet(
            netuid,
            max_allowed_validators,
        ));
    }

    pub fn get_bonds_moving_average(netuid: NetUid) -> u64 {
        BondsMovingAverage::<T>::get(netuid)
    }
    pub fn set_bonds_moving_average(netuid: NetUid, bonds_moving_average: u64) {
        BondsMovingAverage::<T>::insert(netuid, bonds_moving_average);
        Self::deposit_event(Event::BondsMovingAverageSet(netuid, bonds_moving_average));
    }

    pub fn get_bonds_penalty(netuid: NetUid) -> u16 {
        BondsPenalty::<T>::get(netuid)
    }
    pub fn set_bonds_penalty(netuid: NetUid, bonds_penalty: u16) {
        BondsPenalty::<T>::insert(netuid, bonds_penalty);
        Self::deposit_event(Event::BondsPenaltySet(netuid, bonds_penalty));
    }

    pub fn get_bonds_reset(netuid: NetUid) -> bool {
        BondsResetOn::<T>::get(netuid)
    }
    pub fn set_bonds_reset(netuid: NetUid, bonds_reset: bool) {
        BondsResetOn::<T>::insert(netuid, bonds_reset);
        Self::deposit_event(Event::BondsResetOnSet(netuid, bonds_reset));
    }

    pub fn get_max_registrations_per_block(netuid: NetUid) -> u16 {
        MaxRegistrationsPerBlock::<T>::get(netuid)
    }
    pub fn set_max_registrations_per_block(netuid: NetUid, max_registrations_per_block: u16) {
        MaxRegistrationsPerBlock::<T>::insert(netuid, max_registrations_per_block);
        Self::deposit_event(Event::MaxRegistrationsPerBlockSet(
            netuid,
            max_registrations_per_block,
        ));
    }

    pub fn get_subnet_owner(netuid: NetUid) -> T::AccountId {
        SubnetOwner::<T>::get(netuid)
    }
    pub fn get_subnet_owner_cut() -> u16 {
        SubnetOwnerCut::<T>::get()
    }
    pub fn get_float_subnet_owner_cut() -> U96F32 {
        U96F32::saturating_from_num(SubnetOwnerCut::<T>::get())
            .safe_div(U96F32::saturating_from_num(u16::MAX))
    }
    pub fn set_subnet_owner_cut(subnet_owner_cut: u16) {
        SubnetOwnerCut::<T>::set(subnet_owner_cut);
        Self::deposit_event(Event::SubnetOwnerCutSet(subnet_owner_cut));
    }

    pub fn get_owned_hotkeys(coldkey: &T::AccountId) -> Vec<T::AccountId> {
        OwnedHotkeys::<T>::get(coldkey)
    }
    pub fn get_all_staked_hotkeys(coldkey: &T::AccountId) -> Vec<T::AccountId> {
        StakingHotkeys::<T>::get(coldkey)
    }

    pub fn set_total_issuance(total_issuance: u64) {
        TotalIssuance::<T>::put(total_issuance);
    }

    pub fn get_rao_recycled(netuid: NetUid) -> u64 {
        RAORecycledForRegistration::<T>::get(netuid)
    }
    pub fn set_rao_recycled(netuid: NetUid, rao_recycled: u64) {
        RAORecycledForRegistration::<T>::insert(netuid, rao_recycled);
        Self::deposit_event(Event::RAORecycledForRegistrationSet(netuid, rao_recycled));
    }
    pub fn increase_rao_recycled(netuid: NetUid, inc_rao_recycled: u64) {
        let curr_rao_recycled = Self::get_rao_recycled(netuid);
        let rao_recycled = curr_rao_recycled.saturating_add(inc_rao_recycled);
        Self::set_rao_recycled(netuid, rao_recycled);
    }

    pub fn set_senate_required_stake_perc(required_percent: u64) {
        SenateRequiredStakePercentage::<T>::put(required_percent);
    }

    pub fn is_senate_member(hotkey: &T::AccountId) -> bool {
        T::SenateMembers::is_member(hotkey)
    }

    pub fn do_set_senate_required_stake_perc(
        origin: T::RuntimeOrigin,
        required_percent: u64,
    ) -> DispatchResult {
        ensure_root(origin)?;

        Self::set_senate_required_stake_perc(required_percent);
        Self::deposit_event(Event::SenateRequiredStakePercentSet(required_percent));
        Ok(())
    }

    pub fn is_subnet_owner(address: &T::AccountId) -> bool {
        SubnetOwner::<T>::iter_values().any(|owner| *address == owner)
    }

    /// The NominatorMinRequiredStake is the factor by which we multiply
    /// the DefaultMinStake to get nominator minimum stake. With DefaulMinStake
    /// of 0.001 TAO and NominatorMinRequiredStake of 100_000_000, the
    /// minimum nomination stake will be 0.1 TAO.
    pub fn get_nominator_min_required_stake() -> u64 {
        // Get the factor (It is stored in per-million format)
        let factor = NominatorMinRequiredStake::<T>::get();

        // Return the default minimum stake multiplied by factor
        // 21M * 10^9 * 10^6 fits u64, hence no need for fixed type usage here
        DefaultMinStake::<T>::get()
            .saturating_mul(factor)
            .safe_div(1_000_000)
    }

    pub fn set_nominator_min_required_stake(min_stake: u64) {
        NominatorMinRequiredStake::<T>::put(min_stake);
    }

    pub fn get_key_swap_cost() -> u64 {
        T::KeySwapCost::get()
    }

    pub fn get_alpha_values(netuid: NetUid) -> (u16, u16) {
        AlphaValues::<T>::get(netuid)
    }

    pub fn set_alpha_values_32(netuid: NetUid, low: I32F32, high: I32F32) {
        let low = (low.saturating_mul(I32F32::saturating_from_num(u16::MAX))).to_num::<u16>();
        let high = (high.saturating_mul(I32F32::saturating_from_num(u16::MAX))).to_num::<u16>();
        AlphaValues::<T>::insert(netuid, (low, high));
    }

    pub fn get_alpha_values_32(netuid: NetUid) -> (I32F32, I32F32) {
        let (alpha_low, alpha_high): (u16, u16) = AlphaValues::<T>::get(netuid);
        let converted_low =
            I32F32::saturating_from_num(alpha_low).safe_div(I32F32::saturating_from_num(u16::MAX));
        let converted_high =
            I32F32::saturating_from_num(alpha_high).safe_div(I32F32::saturating_from_num(u16::MAX));

        (converted_low, converted_high)
    }

    pub fn set_alpha_sigmoid_steepness(netuid: NetUid, steepness: i16) {
        AlphaSigmoidSteepness::<T>::insert(netuid, steepness);
    }
    pub fn get_alpha_sigmoid_steepness(netuid: NetUid) -> I32F32 {
        let alpha = AlphaSigmoidSteepness::<T>::get(netuid);
        I32F32::saturating_from_num(alpha)
    }

    pub fn set_liquid_alpha_enabled(netuid: NetUid, enabled: bool) {
        LiquidAlphaOn::<T>::set(netuid, enabled);
    }

    pub fn get_liquid_alpha_enabled(netuid: NetUid) -> bool {
        LiquidAlphaOn::<T>::get(netuid)
    }

    pub fn set_yuma3_enabled(netuid: NetUid, enabled: bool) {
        Yuma3On::<T>::set(netuid, enabled);
    }

    pub fn get_yuma3_enabled(netuid: NetUid) -> bool {
        Yuma3On::<T>::get(netuid)
    }

    pub fn get_subtoken_enabled(netuid: NetUid) -> bool {
        SubtokenEnabled::<T>::get(netuid)
    }

    pub fn get_transfer_toggle(netuid: NetUid) -> bool {
        TransferToggle::<T>::get(netuid)
    }

    /// Set the duration for coldkey swap
    ///
    /// # Arguments
    ///
    /// * `duration` - The blocks for coldkey swap execution.
    ///
    /// # Effects
    ///
    /// * Update the ColdkeySwapScheduleDuration storage.
    /// * Emits a ColdkeySwapScheduleDurationSet evnet.
    pub fn set_coldkey_swap_schedule_duration(duration: BlockNumberFor<T>) {
        ColdkeySwapScheduleDuration::<T>::set(duration);
        Self::deposit_event(Event::ColdkeySwapScheduleDurationSet(duration));
    }

    /// Set the duration for dissolve network
    ///
    /// # Arguments
    ///
    /// * `duration` - The blocks for dissolve network execution.
    ///
    /// # Effects
    ///
    /// * Update the DissolveNetworkScheduleDuration storage.
    /// * Emits a DissolveNetworkScheduleDurationSet evnet.
    pub fn set_dissolve_network_schedule_duration(duration: BlockNumberFor<T>) {
        DissolveNetworkScheduleDuration::<T>::set(duration);
        Self::deposit_event(Event::DissolveNetworkScheduleDurationSet(duration));
    }

    /// Set the owner hotkey for a subnet.
    ///
    /// # Arguments
    ///
    /// * `netuid` - The unique identifier for the subnet.
    /// * `hotkey` - The new hotkey for the subnet owner.
    ///
    /// # Effects
    ///
    /// * Update the SubnetOwnerHotkey storage.
    /// * Emits a SubnetOwnerHotkeySet event.
    pub fn set_subnet_owner_hotkey(netuid: NetUid, hotkey: &T::AccountId) {
        SubnetOwnerHotkey::<T>::insert(netuid, hotkey.clone());
        Self::deposit_event(Event::SubnetOwnerHotkeySet(netuid, hotkey.clone()));
    }

    // Get the uid of the Owner Hotkey for a subnet.
    pub fn get_owner_uid(netuid: NetUid) -> Option<u16> {
        match SubnetOwnerHotkey::<T>::try_get(netuid) {
            Ok(owner_hotkey) => Uids::<T>::get(netuid, &owner_hotkey),
            Err(_) => None,
        }
    }
}
