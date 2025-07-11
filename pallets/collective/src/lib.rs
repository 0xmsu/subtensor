// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Collective system: Members of a set of account IDs can make their collective feelings known
//! through dispatched calls from one of two specialized origins.
//!
//! The membership can be provided in one of two ways: either directly, using the Root-dispatchable
//! function `set_members`, or indirectly, through implementing the `ChangeMembers`.
//! The pallet assumes that the amount of members stays at or below `MaxMembers` for its weight
//! calculations, but enforces this neither in `set_members` nor in `change_members_sorted`.
//!
//! A "prime" member may be set to help determine the default vote behavior based on chain
//! config. If `PrimeDefaultVote` is used, the prime vote acts as the default vote in case of any
//! abstentions after the voting period. If `MoreThanMajorityThenPrimeDefaultVote` is used, then
//! abstentions will first follow the majority of the collective voting, and then the prime
//! member.
//!
//! Voting happens through motions comprising a proposal (i.e. a curried dispatchable) plus a
//! number of approvals required for it to pass and be called. Motions are open for members to
//! vote on for a minimum period given by `MotionDuration`. As soon as the needed number of
//! approvals is given, the motion is closed and executed. If the number of approvals is not reached
//! during the voting period, then `close` may be called by any account in order to force the end
//! the motion explicitly. If a prime member is defined then their vote is used in place of any
//! abstentions and the proposal is executed if there are enough approvals counting the new votes.
//!
//! If there are not, or if no prime is set, then the motion is dropped without being executed.

#![cfg_attr(not(feature = "std"), no_std)]
#![recursion_limit = "128"]

use frame_support::{
    dispatch::{DispatchResultWithPostInfo, GetDispatchInfo, Pays, PostDispatchInfo},
    ensure,
    pallet_prelude::*,
    traits::{
        Backing, ChangeMembers, EnsureOrigin, Get, GetBacking, InitializeMembers, StorageVersion,
    },
    weights::Weight,
};
use scale_info::TypeInfo;
use sp_io::storage;
use sp_runtime::traits::Dispatchable;
use sp_runtime::{RuntimeDebug, Saturating, traits::Hash};
use sp_std::{marker::PhantomData, prelude::*, result};

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;

pub use pallet::*;
use subtensor_macros::freeze_struct;
pub use weights::WeightInfo;

const LOG_TARGET: &str = "runtime::collective";

/// Simple index type for proposal counting.
pub type ProposalIndex = u32;

/// A number of members.
///
/// This also serves as a number of voting members, and since for motions, each member may
/// vote exactly once, therefore also the number of votes for any given motion.
pub type MemberCount = u32;

/// Default voting strategy when a member is inactive.
pub trait DefaultVote {
    /// Get the default voting strategy, given:
    ///
    /// - Whether the prime member voted Aye.
    /// - Raw number of yes votes.
    /// - Raw number of no votes.
    /// - Total number of member count.
    fn default_vote(
        prime_vote: Option<bool>,
        yes_votes: MemberCount,
        no_votes: MemberCount,
        len: MemberCount,
    ) -> bool;
}

/// Set the prime member's vote as the default vote.
pub struct PrimeDefaultVote;

impl DefaultVote for PrimeDefaultVote {
    fn default_vote(
        prime_vote: Option<bool>,
        _yes_votes: MemberCount,
        _no_votes: MemberCount,
        _len: MemberCount,
    ) -> bool {
        prime_vote.unwrap_or(false)
    }
}

/// First see if yes vote are over majority of the whole collective. If so, set the default vote
/// as yes. Otherwise, use the prime member's vote as the default vote.
pub struct MoreThanMajorityThenPrimeDefaultVote;

impl DefaultVote for MoreThanMajorityThenPrimeDefaultVote {
    fn default_vote(
        prime_vote: Option<bool>,
        yes_votes: MemberCount,
        _no_votes: MemberCount,
        len: MemberCount,
    ) -> bool {
        let more_than_majority = yes_votes.saturating_mul(2) > len;
        more_than_majority || prime_vote.unwrap_or(false)
    }
}

/// Origin for the collective module.
#[derive(PartialEq, Eq, Clone, RuntimeDebug, Encode, Decode, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(I))]
#[codec(mel_bound(AccountId: MaxEncodedLen))]
pub enum RawOrigin<AccountId, I> {
    /// It has been condoned by a given number of members of the collective from a given total.
    Members(MemberCount, MemberCount),
    /// It has been condoned by a single member of the collective.
    Member(AccountId),
    /// Dummy to manage the fact we have instancing.
    _Phantom(PhantomData<I>),
}

impl<AccountId, I> GetBacking for RawOrigin<AccountId, I> {
    fn get_backing(&self) -> Option<Backing> {
        match self {
            RawOrigin::Members(n, d) => Some(Backing {
                approvals: *n,
                eligible: *d,
            }),
            _ => None,
        }
    }
}

/// Info for keeping track of a motion being voted on.
#[freeze_struct("a8e7b0b34ad52b17")]
#[derive(PartialEq, Eq, Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct Votes<AccountId, BlockNumber> {
    /// The proposal's unique index.
    index: ProposalIndex,
    /// The number of approval votes that are needed to pass the motion.
    threshold: MemberCount,
    /// The current set of voters that approved it.
    ayes: Vec<AccountId>,
    /// The current set of voters that rejected it.
    nays: Vec<AccountId>,
    /// The hard end time of this vote.
    end: BlockNumber,
}

#[deny(missing_docs)]
#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_system::pallet_prelude::*;

    /// The current storage version.
    const STORAGE_VERSION: StorageVersion = StorageVersion::new(4);

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    #[pallet::without_storage_info]
    pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

    #[pallet::config]
    pub trait Config<I: 'static = ()>: frame_system::Config {
        /// The runtime origin type.
        type RuntimeOrigin: From<RawOrigin<Self::AccountId, I>>;

        /// The runtime call dispatch type.
        type Proposal: Parameter
            + Dispatchable<
                RuntimeOrigin = <Self as Config<I>>::RuntimeOrigin,
                PostInfo = PostDispatchInfo,
            > + From<frame_system::Call<Self>>
            + GetDispatchInfo;

        /// The runtime event type.
        type RuntimeEvent: From<Event<Self, I>>
            + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The time-out for council motions.
        type MotionDuration: Get<BlockNumberFor<Self>>;

        /// Maximum number of proposals allowed to be active in parallel.
        type MaxProposals: Get<ProposalIndex>;

        /// The maximum number of members supported by the pallet. Used for weight estimation.
        ///
        /// NOTE:
        /// + Benchmarks will need to be re-run and weights adjusted if this changes.
        /// + This pallet assumes that dependents keep to the limit without enforcing it.
        type MaxMembers: Get<MemberCount>;

        /// Default vote strategy of this collective.
        type DefaultVote: DefaultVote;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// Origin allowed to set collective members
        type SetMembersOrigin: EnsureOrigin<<Self as frame_system::Config>::RuntimeOrigin>;

        /// Origin allowed to propose
        type CanPropose: CanPropose<Self::AccountId>;

        /// Origin allowed to vote
        type CanVote: CanVote<Self::AccountId>;

        /// Members to expect in a vote
        type GetVotingMembers: GetVotingMembers<MemberCount>;
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
        /// The phantom just for type place holder.
        pub phantom: PhantomData<I>,
        /// The initial members of the collective.
        pub members: Vec<T::AccountId>,
    }

    impl<T: Config<I>, I: 'static> Default for GenesisConfig<T, I> {
        fn default() -> Self {
            Self {
                phantom: Default::default(),
                members: Default::default(),
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config<I>, I: 'static> BuildGenesisConfig for GenesisConfig<T, I> {
        fn build(&self) {
            use sp_std::collections::btree_set::BTreeSet;
            let members_set: BTreeSet<_> = self.members.iter().collect();
            assert_eq!(
                members_set.len(),
                self.members.len(),
                "Members cannot contain duplicate accounts."
            );

            Pallet::<T, I>::initialize_members(&self.members)
        }
    }

    /// Origin for the collective pallet.
    #[pallet::origin]
    pub type Origin<T, I = ()> = RawOrigin<<T as frame_system::Config>::AccountId, I>;

    /// The hashes of the active proposals.
    #[pallet::storage]
    #[pallet::getter(fn proposals)]
    pub type Proposals<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BoundedVec<T::Hash, T::MaxProposals>, ValueQuery>;

    /// Actual proposal for a given hash, if it's current.
    #[pallet::storage]
    #[pallet::getter(fn proposal_of)]
    pub type ProposalOf<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Identity, T::Hash, <T as Config<I>>::Proposal, OptionQuery>;

    /// Votes on a given proposal, if it is ongoing.
    #[pallet::storage]
    #[pallet::getter(fn voting)]
    pub type Voting<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Identity, T::Hash, Votes<T::AccountId, BlockNumberFor<T>>, OptionQuery>;

    /// Proposals so far.
    #[pallet::storage]
    #[pallet::getter(fn proposal_count)]
    pub type ProposalCount<T: Config<I>, I: 'static = ()> = StorageValue<_, u32, ValueQuery>;

    /// The current members of the collective. This is stored sorted (just by value).
    #[pallet::storage]
    #[pallet::getter(fn members)]
    pub type Members<T: Config<I>, I: 'static = ()> =
        StorageValue<_, Vec<T::AccountId>, ValueQuery>;

    /// The prime member that helps determine the default vote behavior in case of absentations.
    #[pallet::storage]
    #[pallet::getter(fn prime)]
    pub type Prime<T: Config<I>, I: 'static = ()> = StorageValue<_, T::AccountId, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config<I>, I: 'static = ()> {
        /// A motion (given hash) has been proposed (by given account) with a threshold (given
        /// `MemberCount`).
        Proposed {
            /// The account that proposed the motion.
            account: T::AccountId,
            /// The index of the proposal.
            proposal_index: ProposalIndex,
            /// The hash of the proposal.
            proposal_hash: T::Hash,
            /// The threshold of member for the proposal.
            threshold: MemberCount,
        },
        /// A motion (given hash) has been voted on by given account, leaving
        /// a tally (yes votes and no votes given respectively as `MemberCount`).
        Voted {
            /// The account that voted.
            account: T::AccountId,
            /// The hash of the proposal.
            proposal_hash: T::Hash,
            /// Whether the account voted aye.
            voted: bool,
            /// The number of yes votes.
            yes: MemberCount,
            /// The number of no votes.
            no: MemberCount,
        },
        /// A motion was approved by the required threshold.
        Approved {
            /// The hash of the proposal.
            proposal_hash: T::Hash,
        },
        /// A motion was not approved by the required threshold.
        Disapproved {
            /// The hash of the proposal.
            proposal_hash: T::Hash,
        },
        /// A motion was executed; result will be `Ok` if it returned without error.
        Executed {
            /// The hash of the proposal.
            proposal_hash: T::Hash,
            /// The result of the execution.
            result: DispatchResult,
        },
        /// A single member did some action; result will be `Ok` if it returned without error.
        MemberExecuted {
            /// The hash of the proposal.
            proposal_hash: T::Hash,
            /// The result of the execution.
            result: DispatchResult,
        },
        /// A proposal was closed because its threshold was reached or after its duration was up.
        Closed {
            /// The hash of the proposal.
            proposal_hash: T::Hash,
            /// Whether the proposal was approved.
            yes: MemberCount,
            /// Whether the proposal was rejected.
            no: MemberCount,
        },
    }

    #[pallet::error]
    pub enum Error<T, I = ()> {
        /// Account is not a member of collective
        NotMember,
        /// Duplicate proposals not allowed
        DuplicateProposal,
        /// Proposal must exist
        ProposalNotExists,
        /// Index mismatched the proposal hash
        IndexMismatchProposalHash,
        /// Duplicate vote ignored
        DuplicateVote,
        /// The call to close the proposal was made too early, before the end of the voting
        TooEarlyToCloseProposal,
        /// There can only be a maximum of `MaxProposals` active proposals.
        TooManyActiveProposals,
        /// The given weight-bound for the proposal was too low.
        ProposalWeightLessThanDispatchCallWeight,
        /// The given length-bound for the proposal was too low.
        ProposalLengthBoundLessThanProposalLength,
        /// The given motion duration for the proposal was too low.
        DurationLowerThanConfiguredMotionDuration,
    }

    // Note that councillor operations are assigned to the operational class.
    #[pallet::call]
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        /// Set the collective's membership.
        ///
        /// - `new_members`: The new member list. Be nice to the chain and provide it sorted.
        /// - `prime`: The prime member whose vote sets the default.
        /// - `old_count`: The upper bound for the previous number of members in storage. Used for
        ///   weight estimation.
        ///
        /// The dispatch of this call must be `SetMembersOrigin`.
        ///
        /// NOTE: Does not enforce the expected `MaxMembers` limit on the amount of members, but
        ///       the weight estimations rely on it to estimate dispatchable weight.
        ///
        /// # WARNING:
        ///
        /// The `pallet-collective` can also be managed by logic outside of the pallet through the
        /// implementation of the trait [`ChangeMembers`].
        /// Any call to `set_members` must be careful that the member set doesn't get out of sync
        /// with other logic managing the member set.
        ///
        /// ## Complexity:
        /// - `O(MP + N)` where:
        ///   - `M` old-members-count (code- and governance-bounded)
        ///   - `N` new-members-count (code- and governance-bounded)
        ///   - `P` proposals-count (code-bounded)
        #[pallet::call_index(0)]
        #[pallet::weight((
			T::WeightInfo::set_members(
				*old_count, // M
				new_members.len() as u32, // N
				T::MaxProposals::get() // P
			),
			DispatchClass::Operational
		))]
        pub fn set_members(
            origin: OriginFor<T>,
            new_members: Vec<T::AccountId>,
            prime: Option<T::AccountId>,
            old_count: MemberCount,
        ) -> DispatchResultWithPostInfo {
            T::SetMembersOrigin::ensure_origin(origin)?;
            if new_members.len() > T::MaxMembers::get() as usize {
                log::error!(
                    target: LOG_TARGET,
                    "New members count ({}) exceeds maximum amount of members expected ({}).",
                    new_members.len(),
                    T::MaxMembers::get(),
                );
            }

            let old = Members::<T, I>::get();
            if old.len() > old_count as usize {
                log::warn!(
                    target: LOG_TARGET,
                    "Wrong count used to estimate set_members weight. expected ({}) vs actual ({})",
                    old_count,
                    old.len(),
                );
            }
            let mut new_members = new_members;
            new_members.sort();
            <Self as ChangeMembers<T::AccountId>>::set_members_sorted(&new_members, &old);
            Prime::<T, I>::set(prime);

            Ok(Some(T::WeightInfo::set_members(
                old.len() as u32,         // M
                new_members.len() as u32, // N
                T::MaxProposals::get(),   // P
            ))
            .into())
        }

        /// Dispatch a proposal from a member using the `Member` origin.
        ///
        /// Origin must be a member of the collective.
        ///
        /// ## Complexity:
        /// - `O(B + M + P)` where:
        /// - `B` is `proposal` size in bytes (length-fee-bounded)
        /// - `M` members-count (code-bounded)
        /// - `P` complexity of dispatching `proposal`
        #[pallet::call_index(1)]
        #[pallet::weight((
			T::WeightInfo::execute(
				*length_bound, // B
				T::MaxMembers::get(), // M
			).saturating_add(proposal.get_dispatch_info().call_weight), // P
			DispatchClass::Operational
		))]
        pub fn execute(
            origin: OriginFor<T>,
            proposal: Box<<T as Config<I>>::Proposal>,
            #[pallet::compact] length_bound: u32,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;
            let members = Self::members();
            ensure!(members.contains(&who), Error::<T, I>::NotMember);

            let proposal_len = proposal.encoded_size();
            ensure!(
                proposal_len <= length_bound as usize,
                Error::<T, I>::ProposalLengthBoundLessThanProposalLength
            );

            let proposal_hash = T::Hashing::hash_of(&proposal);
            let result = proposal.dispatch(RawOrigin::Member(who).into());
            Self::deposit_event(Event::MemberExecuted {
                proposal_hash,
                result: result.map(|_| ()).map_err(|e| e.error),
            });

            Ok(get_result_weight(result)
                .map(|w| {
                    T::WeightInfo::execute(
                        proposal_len as u32,  // B
                        members.len() as u32, // M
                    )
                    .saturating_add(w) // P
                })
                .into())
        }

        /// Add a new proposal to either be voted on or executed directly.
        ///
        /// Requires the sender to be member.
        ///
        /// `threshold` determines whether `proposal` is executed directly (`threshold < 2`)
        /// or put up for voting.
        ///
        /// ## Complexity
        /// - `O(B + M + P1)` or `O(B + M + P2)` where:
        ///   - `B` is `proposal` size in bytes (length-fee-bounded)
        ///   - `M` is members-count (code- and governance-bounded)
        ///   - branching is influenced by `threshold` where:
        ///     - `P1` is proposal execution complexity (`threshold < 2`)
        ///     - `P2` is proposals-count (code-bounded) (`threshold >= 2`)
        #[pallet::call_index(2)]
        #[pallet::weight((
			T::WeightInfo::propose_proposed(
				*length_bound, // B
				T::MaxMembers::get(), // M
				T::MaxProposals::get(), // P2
			),
			DispatchClass::Operational
		))]
        pub fn propose(
            origin: OriginFor<T>,
            proposal: Box<<T as Config<I>>::Proposal>,
            #[pallet::compact] length_bound: u32,
            duration: BlockNumberFor<T>,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin.clone())?;
            ensure!(T::CanPropose::can_propose(&who), Error::<T, I>::NotMember);

            ensure!(
                duration >= T::MotionDuration::get(),
                Error::<T, I>::DurationLowerThanConfiguredMotionDuration
            );

            let threshold = T::GetVotingMembers::get_count()
                .checked_div(2)
                .unwrap_or(0)
                .saturating_add(1);

            let members = Self::members();
            let (proposal_len, active_proposals) =
                Self::do_propose_proposed(who, threshold, proposal, length_bound, duration)?;

            Ok(Some(T::WeightInfo::propose_proposed(
                proposal_len,         // B
                members.len() as u32, // M
                active_proposals,     // P2
            ))
            .into())
        }

        /// Add an aye or nay vote for the sender to the given proposal.
        ///
        /// Requires the sender to be a member.
        ///
        /// Transaction fees will be waived if the member is voting on any particular proposal
        /// for the first time and the call is successful. Subsequent vote changes will charge a
        /// fee.
        /// ## Complexity
        /// - `O(M)` where `M` is members-count (code- and governance-bounded)
        #[pallet::call_index(3)]
        #[pallet::weight((T::WeightInfo::vote(T::MaxMembers::get()), DispatchClass::Operational))]
        pub fn vote(
            origin: OriginFor<T>,
            proposal: T::Hash,
            #[pallet::compact] index: ProposalIndex,
            approve: bool,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin.clone())?;
            ensure!(T::CanVote::can_vote(&who), Error::<T, I>::NotMember);

            let members = Self::members();
            // Detects first vote of the member in the motion
            let is_account_voting_first_time = Self::do_vote(who, proposal, index, approve)?;

            if is_account_voting_first_time {
                Ok((Some(T::WeightInfo::vote(members.len() as u32)), Pays::No).into())
            } else {
                Ok((Some(T::WeightInfo::vote(members.len() as u32)), Pays::Yes).into())
            }
        }

        // NOTE: call_index(4) was `close_old_weight` and was removed due to weights v1
        // deprecation

        /// Disapprove a proposal, close, and remove it from the system, regardless of its current
        /// state.
        ///
        /// Must be called by the Root origin.
        ///
        /// Parameters:
        /// * `proposal_hash`: The hash of the proposal that should be disapproved.
        ///
        /// ## Complexity
        /// O(P) where P is the number of max proposals
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::disapprove_proposal(T::MaxProposals::get()))]
        pub fn disapprove_proposal(
            origin: OriginFor<T>,
            proposal_hash: T::Hash,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;
            let proposal_count = Self::do_disapprove_proposal(proposal_hash);
            Ok(Some(T::WeightInfo::disapprove_proposal(proposal_count)).into())
        }

        /// Close a vote that is either approved, disapproved or whose voting period has ended.
        ///
        /// May be called by any signed account in order to finish voting and close the proposal.
        ///
        /// If called before the end of the voting period it will only close the vote if it is
        /// has enough votes to be approved or disapproved.
        ///
        /// If called after the end of the voting period abstentions are counted as rejections
        /// unless there is a prime member set and the prime member cast an approval.
        ///
        /// If the close operation completes successfully with disapproval, the transaction fee will
        /// be waived. Otherwise execution of the approved operation will be charged to the caller.
        ///
        /// + `proposal_weight_bound`: The maximum amount of weight consumed by executing the closed
        /// proposal.
        /// + `length_bound`: The upper bound for the length of the proposal in storage. Checked via
        /// `storage::read` so it is `size_of::<u32>() == 4` larger than the pure length.
        ///
        /// ## Complexity
        /// - `O(B + M + P1 + P2)` where:
        ///   - `B` is `proposal` size in bytes (length-fee-bounded)
        ///   - `M` is members-count (code- and governance-bounded)
        ///   - `P1` is the complexity of `proposal` preimage.
        ///   - `P2` is proposal-count (code-bounded)
        #[pallet::call_index(6)]
        #[pallet::weight((
			{
				let b = *length_bound;
				let m = T::MaxMembers::get();
				let p1 = *proposal_weight_bound;
				let p2 = T::MaxProposals::get();
				T::WeightInfo::close_early_approved(b, m, p2)
					.max(T::WeightInfo::close_early_disapproved(m, p2))
					.max(T::WeightInfo::close_approved(b, m, p2))
					.max(T::WeightInfo::close_disapproved(m, p2))
					.saturating_add(p1)
			},
			DispatchClass::Operational
		))]
        pub fn close(
            origin: OriginFor<T>,
            proposal_hash: T::Hash,
            #[pallet::compact] index: ProposalIndex,
            proposal_weight_bound: Weight,
            #[pallet::compact] length_bound: u32,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            Self::do_close(proposal_hash, index, proposal_weight_bound, length_bound)
        }
    }
}

use frame_system::pallet_prelude::BlockNumberFor;

/// Return the weight of a dispatch call result as an `Option`.
///
/// Will return the weight regardless of what the state of the result is.
fn get_result_weight(result: DispatchResultWithPostInfo) -> Option<Weight> {
    match result {
        Ok(post_info) => post_info.actual_weight,
        Err(err) => err.post_info.actual_weight,
    }
}

impl<T: Config<I>, I: 'static> Pallet<T, I> {
    /// Check whether `who` is a member of the collective.
    pub fn is_member(who: &T::AccountId) -> bool {
        // Note: The dispatchables *do not* use this to check membership so make sure
        // to update those if this is changed.
        Self::members().contains(who)
    }

    /// Add a new proposal to be voted.
    pub fn do_propose_proposed(
        who: T::AccountId,
        threshold: MemberCount,
        proposal: Box<<T as Config<I>>::Proposal>,
        length_bound: MemberCount,
        duration: BlockNumberFor<T>,
    ) -> Result<(u32, u32), DispatchError> {
        let proposal_len = proposal.encoded_size();
        ensure!(
            proposal_len <= length_bound as usize,
            Error::<T, I>::ProposalLengthBoundLessThanProposalLength
        );

        let proposal_hash = T::Hashing::hash_of(&proposal);
        ensure!(
            !<ProposalOf<T, I>>::contains_key(proposal_hash),
            Error::<T, I>::DuplicateProposal
        );

        let active_proposals =
            <Proposals<T, I>>::try_mutate(|proposals| -> Result<usize, DispatchError> {
                proposals
                    .try_push(proposal_hash)
                    .map_err(|_| Error::<T, I>::TooManyActiveProposals)?;
                Ok(proposals.len())
            })?;

        let index = Self::proposal_count();
        <ProposalCount<T, I>>::try_mutate(|i| {
            *i = i
                .checked_add(1)
                .ok_or(Error::<T, I>::TooManyActiveProposals)?;
            Ok::<(), Error<T, I>>(())
        })?;
        <ProposalOf<T, I>>::insert(proposal_hash, proposal);
        let votes = {
            let end = frame_system::Pallet::<T>::block_number().saturating_add(duration);
            Votes {
                index,
                threshold,
                ayes: vec![],
                nays: vec![],
                end,
            }
        };
        <Voting<T, I>>::insert(proposal_hash, votes);

        Self::deposit_event(Event::Proposed {
            account: who,
            proposal_index: index,
            proposal_hash,
            threshold,
        });
        Ok((proposal_len as u32, active_proposals as u32))
    }

    /// Add an aye or nay vote for the member to the given proposal, returns true if it's the first
    /// vote of the member in the motion
    pub fn do_vote(
        who: T::AccountId,
        proposal: T::Hash,
        index: ProposalIndex,
        approve: bool,
    ) -> Result<bool, DispatchError> {
        let mut voting = Self::voting(proposal).ok_or(Error::<T, I>::ProposalNotExists)?;
        ensure!(
            voting.index == index,
            Error::<T, I>::IndexMismatchProposalHash
        );

        let position_yes = voting.ayes.iter().position(|a| a == &who);
        let position_no = voting.nays.iter().position(|a| a == &who);

        // Detects first vote of the member in the motion
        let is_account_voting_first_time = position_yes.is_none() && position_no.is_none();

        if approve {
            if position_yes.is_none() {
                voting.ayes.push(who.clone());
            } else {
                return Err(Error::<T, I>::DuplicateVote.into());
            }
            if let Some(pos) = position_no {
                voting.nays.swap_remove(pos);
            }
        } else {
            if position_no.is_none() {
                voting.nays.push(who.clone());
            } else {
                return Err(Error::<T, I>::DuplicateVote.into());
            }
            if let Some(pos) = position_yes {
                voting.ayes.swap_remove(pos);
            }
        }

        let yes_votes = voting.ayes.len() as MemberCount;
        let no_votes = voting.nays.len() as MemberCount;
        Self::deposit_event(Event::Voted {
            account: who,
            proposal_hash: proposal,
            voted: approve,
            yes: yes_votes,
            no: no_votes,
        });

        Voting::<T, I>::insert(proposal, voting);

        Ok(is_account_voting_first_time)
    }

    /// Close a vote that is either approved, disapproved or whose voting period has ended.
    pub fn do_close(
        proposal_hash: T::Hash,
        index: ProposalIndex,
        proposal_weight_bound: Weight,
        length_bound: u32,
    ) -> DispatchResultWithPostInfo {
        let voting = Self::voting(proposal_hash).ok_or(Error::<T, I>::ProposalNotExists)?;
        ensure!(
            voting.index == index,
            Error::<T, I>::IndexMismatchProposalHash
        );

        let mut no_votes = voting.nays.len() as MemberCount;
        let mut yes_votes = voting.ayes.len() as MemberCount;
        let seats = T::GetVotingMembers::get_count() as MemberCount;
        let approved = yes_votes >= voting.threshold;
        let disapproved = seats.saturating_sub(no_votes) < voting.threshold;
        // Allow (dis-)approving the proposal as soon as there are enough votes.
        if approved {
            let (proposal, len) = Self::validate_and_get_proposal(
                &proposal_hash,
                length_bound,
                proposal_weight_bound,
            )?;
            Self::deposit_event(Event::Closed {
                proposal_hash,
                yes: yes_votes,
                no: no_votes,
            });
            let (proposal_weight, proposal_count) =
                Self::do_approve_proposal(seats, yes_votes, proposal_hash, proposal);
            return Ok((
                Some(
                    T::WeightInfo::close_early_approved(len as u32, seats, proposal_count)
                        .saturating_add(proposal_weight),
                ),
                Pays::Yes,
            )
                .into());
        } else if disapproved {
            Self::deposit_event(Event::Closed {
                proposal_hash,
                yes: yes_votes,
                no: no_votes,
            });
            let proposal_count = Self::do_disapprove_proposal(proposal_hash);
            return Ok((
                Some(T::WeightInfo::close_early_disapproved(
                    seats,
                    proposal_count,
                )),
                Pays::No,
            )
                .into());
        }

        // Only allow actual closing of the proposal after the voting period has ended.
        ensure!(
            frame_system::Pallet::<T>::block_number() >= voting.end,
            Error::<T, I>::TooEarlyToCloseProposal
        );

        let prime_vote = Self::prime().map(|who| voting.ayes.iter().any(|a| a == &who));

        // default voting strategy.
        let default = T::DefaultVote::default_vote(prime_vote, yes_votes, no_votes, seats);

        let abstentions = seats.saturating_sub(yes_votes.saturating_add(no_votes));
        match default {
            true => yes_votes = yes_votes.saturating_add(abstentions),
            false => no_votes = no_votes.saturating_add(abstentions),
        }
        let approved = yes_votes >= voting.threshold;

        if approved {
            let (proposal, len) = Self::validate_and_get_proposal(
                &proposal_hash,
                length_bound,
                proposal_weight_bound,
            )?;
            Self::deposit_event(Event::Closed {
                proposal_hash,
                yes: yes_votes,
                no: no_votes,
            });
            let (proposal_weight, proposal_count) =
                Self::do_approve_proposal(seats, yes_votes, proposal_hash, proposal);
            Ok((
                Some(
                    T::WeightInfo::close_approved(len as u32, seats, proposal_count)
                        .saturating_add(proposal_weight),
                ),
                Pays::Yes,
            )
                .into())
        } else {
            Self::deposit_event(Event::Closed {
                proposal_hash,
                yes: yes_votes,
                no: no_votes,
            });
            let proposal_count = Self::do_disapprove_proposal(proposal_hash);
            Ok((
                Some(T::WeightInfo::close_disapproved(seats, proposal_count)),
                Pays::No,
            )
                .into())
        }
    }

    /// Ensure that the right proposal bounds were passed and get the proposal from storage.
    ///
    /// Checks the length in storage via `storage::read` which adds an extra `size_of::<u32>() == 4`
    /// to the length.
    fn validate_and_get_proposal(
        hash: &T::Hash,
        length_bound: u32,
        weight_bound: Weight,
    ) -> Result<(<T as Config<I>>::Proposal, usize), DispatchError> {
        let key = ProposalOf::<T, I>::hashed_key_for(hash);
        // read the length of the proposal storage entry directly
        let proposal_len =
            storage::read(&key, &mut [0; 0], 0).ok_or(Error::<T, I>::ProposalNotExists)?;
        ensure!(
            proposal_len <= length_bound,
            Error::<T, I>::ProposalLengthBoundLessThanProposalLength
        );
        let proposal = ProposalOf::<T, I>::get(hash).ok_or(Error::<T, I>::ProposalNotExists)?;
        let proposal_weight = proposal.get_dispatch_info().call_weight;
        ensure!(
            proposal_weight.all_lte(weight_bound),
            Error::<T, I>::ProposalWeightLessThanDispatchCallWeight
        );
        Ok((proposal, proposal_len as usize))
    }

    /// Weight:
    /// If `approved`:
    /// - the weight of `proposal` preimage.
    /// - two events deposited.
    /// - two removals, one mutation.
    /// - computation and i/o `O(P + L)` where:
    ///   - `P` is number of active proposals,
    ///   - `L` is the encoded length of `proposal` preimage.
    ///
    /// If not `approved`:
    /// - one event deposited.
    /// - two removals, one mutation.
    /// - computation and i/o `O(P)` where:
    ///   - `P` is number of active proposals
    fn do_approve_proposal(
        seats: MemberCount,
        yes_votes: MemberCount,
        proposal_hash: T::Hash,
        proposal: <T as Config<I>>::Proposal,
    ) -> (Weight, u32) {
        Self::deposit_event(Event::Approved { proposal_hash });

        let dispatch_weight = proposal.get_dispatch_info().call_weight;
        let origin = RawOrigin::Members(yes_votes, seats).into();
        let result = proposal.dispatch(origin);
        Self::deposit_event(Event::Executed {
            proposal_hash,
            result: result.map(|_| ()).map_err(|e| e.error),
        });
        // default to the dispatch info weight for safety
        let proposal_weight = get_result_weight(result).unwrap_or(dispatch_weight); // P1

        let proposal_count = Self::remove_proposal(proposal_hash);
        (proposal_weight, proposal_count)
    }

    /// Removes a proposal from the pallet, and deposit the `Disapproved` event.
    pub fn do_disapprove_proposal(proposal_hash: T::Hash) -> u32 {
        // disapproved
        Self::deposit_event(Event::Disapproved { proposal_hash });
        Self::remove_proposal(proposal_hash)
    }

    // Removes a proposal from the pallet, cleaning up votes and the vector of proposals.
    fn remove_proposal(proposal_hash: T::Hash) -> u32 {
        // remove proposal and vote
        ProposalOf::<T, I>::remove(proposal_hash);
        Voting::<T, I>::remove(proposal_hash);
        let num_proposals = Proposals::<T, I>::mutate(|proposals| {
            proposals.retain(|h| h != &proposal_hash);
            proposals.len().saturating_add(1) // calculate weight based on original length
        });
        num_proposals as u32
    }

    pub fn remove_votes(who: &T::AccountId) -> Result<bool, DispatchError> {
        for h in Self::proposals().into_iter() {
            <Voting<T, I>>::mutate(h, |v| {
                if let Some(mut votes) = v.take() {
                    votes.ayes.retain(|i| i != who);
                    votes.nays.retain(|i| i != who);
                    *v = Some(votes);
                }
            });
        }

        Ok(true)
    }

    pub fn has_voted(
        proposal: T::Hash,
        index: ProposalIndex,
        who: &T::AccountId,
    ) -> Result<bool, DispatchError> {
        let voting = Self::voting(proposal).ok_or(Error::<T, I>::ProposalNotExists)?;
        ensure!(
            voting.index == index,
            Error::<T, I>::IndexMismatchProposalHash
        );

        let position_yes = voting.ayes.iter().position(|a| a == who);
        let position_no = voting.nays.iter().position(|a| a == who);

        Ok(position_yes.is_some() || position_no.is_some())
    }
}

impl<T: Config<I>, I: 'static> ChangeMembers<T::AccountId> for Pallet<T, I> {
    /// Update the members of the collective. Votes are updated and the prime is reset.
    ///
    /// NOTE: Does not enforce the expected `MaxMembers` limit on the amount of members, but
    ///       the weight estimations rely on it to estimate dispatchable weight.
    ///
    /// ## Complexity
    /// - `O(MP + N)`
    ///   - where `M` old-members-count (governance-bounded)
    ///   - where `N` new-members-count (governance-bounded)
    ///   - where `P` proposals-count
    fn change_members_sorted(
        _incoming: &[T::AccountId],
        outgoing: &[T::AccountId],
        new: &[T::AccountId],
    ) {
        if new.len() > T::MaxMembers::get() as usize {
            log::error!(
                target: LOG_TARGET,
                "New members count ({}) exceeds maximum amount of members expected ({}).",
                new.len(),
                T::MaxMembers::get(),
            );
        }
        // remove accounts from all current voting in motions.
        let mut outgoing = outgoing.to_vec();
        outgoing.sort();
        for h in Self::proposals().into_iter() {
            <Voting<T, I>>::mutate(h, |v| {
                if let Some(mut votes) = v.take() {
                    votes.ayes.retain(|i| outgoing.binary_search(i).is_err());
                    votes.nays.retain(|i| outgoing.binary_search(i).is_err());
                    *v = Some(votes);
                }
            });
        }
        Members::<T, I>::put(new);
        Prime::<T, I>::kill();
    }

    fn set_prime(prime: Option<T::AccountId>) {
        Prime::<T, I>::set(prime);
    }

    fn get_prime() -> Option<T::AccountId> {
        Prime::<T, I>::get()
    }
}

impl<T: Config<I>, I: 'static> InitializeMembers<T::AccountId> for Pallet<T, I> {
    fn initialize_members(members: &[T::AccountId]) {
        if !members.is_empty() {
            assert!(
                <Members<T, I>>::get().is_empty(),
                "Members are already initialized!"
            );
            <Members<T, I>>::put(members);
        }
    }
}

/// Ensure that the origin `o` represents at least `n` members. Returns `Ok` or an `Err`
/// otherwise.
pub fn ensure_members<OuterOrigin, AccountId, I>(
    o: OuterOrigin,
    n: MemberCount,
) -> result::Result<MemberCount, &'static str>
where
    OuterOrigin: Into<result::Result<RawOrigin<AccountId, I>, OuterOrigin>>,
{
    match o.into() {
        Ok(RawOrigin::Members(x, _)) if x >= n => Ok(n),
        _ => Err("bad origin: expected to be a threshold number of members"),
    }
}

pub struct EnsureMember<AccountId, I: 'static>(PhantomData<(AccountId, I)>);
impl<
    O: Into<Result<RawOrigin<AccountId, I>, O>> + From<RawOrigin<AccountId, I>>,
    I,
    AccountId: Decode,
> EnsureOrigin<O> for EnsureMember<AccountId, I>
{
    type Success = AccountId;
    fn try_origin(o: O) -> Result<Self::Success, O> {
        o.into().and_then(|o| match o {
            RawOrigin::Member(id) => Ok(id),
            r => Err(O::from(r)),
        })
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<O, ()> {
        let zero_account_id =
            AccountId::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
                .expect("infinite length input; no invalid inputs for type; qed");
        Ok(O::from(RawOrigin::Member(zero_account_id)))
    }
}

pub struct EnsureMembers<AccountId, I: 'static, const N: u32>(PhantomData<(AccountId, I)>);
impl<
    O: Into<Result<RawOrigin<AccountId, I>, O>> + From<RawOrigin<AccountId, I>>,
    AccountId,
    I,
    const N: u32,
> EnsureOrigin<O> for EnsureMembers<AccountId, I, N>
{
    type Success = (MemberCount, MemberCount);
    fn try_origin(o: O) -> Result<Self::Success, O> {
        o.into().and_then(|o| match o {
            RawOrigin::Members(n, m) if n >= N => Ok((n, m)),
            r => Err(O::from(r)),
        })
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<O, ()> {
        Ok(O::from(RawOrigin::Members(N, N)))
    }
}

pub struct EnsureProportionMoreThan<AccountId, I: 'static, const N: u32, const D: u32>(
    PhantomData<(AccountId, I)>,
);
impl<
    O: Into<Result<RawOrigin<AccountId, I>, O>> + From<RawOrigin<AccountId, I>>,
    AccountId,
    I,
    const N: u32,
    const D: u32,
> EnsureOrigin<O> for EnsureProportionMoreThan<AccountId, I, N, D>
{
    type Success = ();
    fn try_origin(o: O) -> Result<Self::Success, O> {
        o.into().and_then(|o| match o {
            RawOrigin::Members(n, m) if n.saturating_mul(D) > N.saturating_mul(m) => Ok(()),
            r => Err(O::from(r)),
        })
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<O, ()> {
        Ok(O::from(RawOrigin::Members(1u32, 0u32)))
    }
}

pub struct EnsureProportionAtLeast<AccountId, I: 'static, const N: u32, const D: u32>(
    PhantomData<(AccountId, I)>,
);
impl<
    O: Into<Result<RawOrigin<AccountId, I>, O>> + From<RawOrigin<AccountId, I>>,
    AccountId,
    I,
    const N: u32,
    const D: u32,
> EnsureOrigin<O> for EnsureProportionAtLeast<AccountId, I, N, D>
{
    type Success = ();
    fn try_origin(o: O) -> Result<Self::Success, O> {
        o.into().and_then(|o| match o {
            RawOrigin::Members(n, m) if n.saturating_mul(D) >= N.saturating_mul(m) => Ok(()),
            r => Err(O::from(r)),
        })
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<O, ()> {
        Ok(O::from(RawOrigin::Members(0u32, 0u32)))
    }
}

/// CanPropose
pub trait CanPropose<AccountId> {
    /// Check whether or not the passed AccountId can propose a new motion
    fn can_propose(account: &AccountId) -> bool;
}

impl<T> CanPropose<T> for () {
    fn can_propose(_: &T) -> bool {
        false
    }
}

/// CanVote
pub trait CanVote<AccountId> {
    /// Check whether or not the passed AccountId can vote on a motion
    fn can_vote(account: &AccountId) -> bool;
}

impl<T> CanVote<T> for () {
    fn can_vote(_: &T) -> bool {
        false
    }
}

pub trait GetVotingMembers<MemberCount> {
    fn get_count() -> MemberCount;
}

impl GetVotingMembers<MemberCount> for () {
    fn get_count() -> MemberCount {
        0
    }
}
