use frame_support::traits::{
    Imbalance,
    tokens::{
        Fortitude, Precision, Preservation,
        fungible::{Balanced as _, Inspect as _},
    },
};
use safe_math::*;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::{OrderType, SwapHandler};

use super::*;

impl<T: Config> Pallet<T> {
    // Returns true if the passed hotkey allow delegative staking.
    //
    pub fn hotkey_is_delegate(hotkey: &T::AccountId) -> bool {
        Delegates::<T>::contains_key(hotkey)
    }

    // Sets the hotkey as a delegate with take.
    //
    pub fn delegate_hotkey(hotkey: &T::AccountId, take: u16) {
        Delegates::<T>::insert(hotkey, take);
    }

    // Returns the total amount of stake in the staking table.
    //
    pub fn get_total_stake() -> u64 {
        TotalStake::<T>::get()
    }

    // Increases the total amount of stake by the passed amount.
    //
    pub fn increase_total_stake(increment: u64) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_add(increment));
    }

    // Decreases the total amount of stake by the passed amount.
    //
    pub fn decrease_total_stake(decrement: u64) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_sub(decrement));
    }

    /// Returns the total amount of stake (in TAO) under a hotkey (delegative or otherwise)
    pub fn get_total_stake_for_hotkey(hotkey: &T::AccountId) -> u64 {
        Self::get_all_subnet_netuids()
            .into_iter()
            .map(|netuid| {
                let alpha = U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(
                    hotkey, netuid,
                ));
                let alpha_price = U96F32::saturating_from_num(
                    T::SwapInterface::current_alpha_price(netuid.into()),
                );
                alpha.saturating_mul(alpha_price)
            })
            .sum::<U96F32>()
            .saturating_to_num::<u64>()
    }

    // Returns the total amount of stake under a coldkey
    //
    pub fn get_total_stake_for_coldkey(coldkey: &T::AccountId) -> u64 {
        let hotkeys = StakingHotkeys::<T>::get(coldkey);
        hotkeys
            .iter()
            .map(|hotkey| {
                Alpha::<T>::iter_prefix((hotkey, coldkey))
                    .map(|(netuid, _)| {
                        let alpha_stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                            hotkey, coldkey, netuid,
                        );
                        T::SwapInterface::sim_swap(netuid.into(), OrderType::Sell, alpha_stake)
                            .map(|r| {
                                let fee: u64 = U96F32::saturating_from_num(r.fee_paid)
                                    .saturating_mul(T::SwapInterface::current_alpha_price(
                                        netuid.into(),
                                    ))
                                    .saturating_to_num();
                                r.amount_paid_out.saturating_add(fee)
                            })
                            .unwrap_or_default()
                    })
                    .sum::<u64>()
            })
            .sum::<u64>()
    }

    // Creates a cold - hot pairing account if the hotkey is not already an active account.
    //
    pub fn create_account_if_non_existent(coldkey: &T::AccountId, hotkey: &T::AccountId) {
        if !Self::hotkey_account_exists(hotkey) {
            Owner::<T>::insert(hotkey, coldkey);

            // Update OwnedHotkeys map
            let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
            if !hotkeys.contains(hotkey) {
                hotkeys.push(hotkey.clone());
                OwnedHotkeys::<T>::insert(coldkey, hotkeys);
            }

            // Update StakingHotkeys map
            let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
            if !staking_hotkeys.contains(hotkey) {
                staking_hotkeys.push(hotkey.clone());
                StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
            }
        }
    }

    //// If the hotkey is not a delegate, make it a delegate.
    pub fn maybe_become_delegate(hotkey: &T::AccountId) {
        if !Self::hotkey_is_delegate(hotkey) {
            Self::delegate_hotkey(hotkey, Self::get_hotkey_take(hotkey));
        }
    }

    /// Returns the coldkey owning this hotkey. This function should only be called for active accounts.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// The coldkey account ID that owns the hotkey.
    pub fn get_owning_coldkey_for_hotkey(hotkey: &T::AccountId) -> T::AccountId {
        Owner::<T>::get(hotkey)
    }

    /// Returns the hotkey take.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// The take value of the hotkey.
    pub fn get_hotkey_take(hotkey: &T::AccountId) -> u16 {
        Delegates::<T>::get(hotkey)
    }
    pub fn get_hotkey_take_float(hotkey: &T::AccountId) -> U96F32 {
        U96F32::saturating_from_num(Self::get_hotkey_take(hotkey))
            .safe_div(U96F32::saturating_from_num(u16::MAX))
    }

    /// Returns true if the hotkey account has been created.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// True if the hotkey account exists, false otherwise.
    pub fn hotkey_account_exists(hotkey: &T::AccountId) -> bool {
        Owner::<T>::contains_key(hotkey)
    }

    /// Returns true if the passed coldkey owns the hotkey.
    ///
    /// # Arguments
    /// * `coldkey` - The coldkey account ID.
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// True if the coldkey owns the hotkey, false otherwise.
    pub fn coldkey_owns_hotkey(coldkey: &T::AccountId, hotkey: &T::AccountId) -> bool {
        if Self::hotkey_account_exists(hotkey) {
            Owner::<T>::get(hotkey) == *coldkey
        } else {
            false
        }
    }

    /// Clears the nomination for an account, if it is a nominator account and the stake is below the minimum required threshold.
    pub fn clear_small_nomination_if_required(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) {
        // Verify if the account is a nominator account by checking ownership of the hotkey by the coldkey.
        if !Self::coldkey_owns_hotkey(coldkey, hotkey) {
            // If the stake is below the minimum required, it's considered a small nomination and needs to be cleared.
            // Log if the stake is below the minimum required
            let alpha_stake: u64 =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
            let min_alpha_stake =
                U96F32::saturating_from_num(Self::get_nominator_min_required_stake())
                    .safe_div(T::SwapInterface::current_alpha_price(netuid))
                    .saturating_to_num::<u64>();
            if alpha_stake < min_alpha_stake {
                // Log the clearing of a small nomination
                // Remove the stake from the nominator account. (this is a more forceful unstake operation which )
                // Actually deletes the staking account.
                // Do not apply any fees
                let maybe_cleared_stake = Self::unstake_from_subnet(
                    hotkey,
                    coldkey,
                    netuid,
                    alpha_stake,
                    T::SwapInterface::min_price(),
                    false,
                );

                if let Ok(cleared_stake) = maybe_cleared_stake {
                    // Add the stake to the coldkey account.
                    Self::add_balance_to_coldkey_account(coldkey, cleared_stake);
                } else {
                    // Just clear small alpha
                    let alpha =
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
                    Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey, coldkey, netuid, alpha,
                    );
                }
            }
        }
    }

    /// Clears small nominations for all accounts.
    ///
    /// WARN: This is an O(N) operation, where N is the number of staking accounts. It should be
    /// used with caution.
    pub fn clear_small_nominations() {
        // Loop through all staking accounts to identify and clear nominations below the minimum stake.
        for ((hotkey, coldkey, netuid), _) in Alpha::<T>::iter() {
            Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
        }
    }

    pub fn add_balance_to_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) {
        // infallible
        let _ = <T as Config>::Currency::deposit(coldkey, amount, Precision::BestEffort);
    }

    pub fn can_remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) -> bool {
        let current_balance = Self::get_coldkey_balance(coldkey);
        if amount > current_balance {
            return false;
        }

        // This bit is currently untested. @todo

        <T as Config>::Currency::can_withdraw(coldkey, amount)
            .into_result(false)
            .is_ok()
    }

    pub fn get_coldkey_balance(
        coldkey: &T::AccountId,
    ) -> <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance
    {
        <T as Config>::Currency::reducible_balance(
            coldkey,
            Preservation::Expendable,
            Fortitude::Polite,
        )
    }

    #[must_use = "Balance must be used to preserve total issuance of token"]
    pub fn remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) -> Result<u64, DispatchError> {
        if amount == 0 {
            return Ok(0);
        }

        let credit = <T as Config>::Currency::withdraw(
            coldkey,
            amount,
            Precision::BestEffort,
            Preservation::Preserve,
            Fortitude::Polite,
        )
        .map_err(|_| Error::<T>::BalanceWithdrawalError)?
        .peek();

        if credit == 0 {
            return Err(Error::<T>::ZeroBalanceAfterWithdrawn.into());
        }

        Ok(credit)
    }

    pub fn kill_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) -> Result<u64, DispatchError> {
        if amount == 0 {
            return Ok(0);
        }

        let credit = <T as Config>::Currency::withdraw(
            coldkey,
            amount,
            Precision::Exact,
            Preservation::Expendable,
            Fortitude::Force,
        )
        .map_err(|_| Error::<T>::BalanceWithdrawalError)?
        .peek();

        if credit == 0 {
            return Err(Error::<T>::ZeroBalanceAfterWithdrawn.into());
        }

        Ok(credit)
    }

    pub fn is_user_liquidity_enabled(netuid: NetUid) -> bool {
        T::SwapInterface::is_user_liquidity_enabled(netuid)
    }
}
