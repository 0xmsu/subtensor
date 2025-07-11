#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]
// Some arithmetic operations can't use the saturating equivalent, such as the PerThing types
#![allow(clippy::arithmetic_side_effects)]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

use core::num::NonZeroU64;

pub mod check_nonce;
mod migrations;

extern crate alloc;

use codec::{Compact, Decode, Encode};
use frame_support::dispatch::DispatchResult;
use frame_support::traits::{Imbalance, InsideBoth};
use frame_support::{
    PalletId,
    dispatch::DispatchResultWithPostInfo,
    genesis_builder_helper::{build_state, get_preset},
    pallet_prelude::Get,
    traits::{
        Contains, LinearStoragePrice, OnUnbalanced,
        fungible::{
            DecreaseIssuance, HoldConsideration, Imbalance as FungibleImbalance, IncreaseIssuance,
        },
    },
};
use frame_system::{EnsureNever, EnsureRoot, EnsureRootWithSuccess, RawOrigin};
use pallet_commitments::{CanCommit, OnMetadataCommitment};
use pallet_grandpa::{
    AuthorityId as GrandpaId, AuthorityList as GrandpaAuthorityList, fg_primitives,
};
use pallet_registry::CanRegisterIdentity;
use pallet_subtensor::rpc_info::{
    delegate_info::DelegateInfo,
    dynamic_info::DynamicInfo,
    metagraph::{Metagraph, SelectiveMetagraph},
    neuron_info::{NeuronInfo, NeuronInfoLite},
    show_subnet::SubnetState,
    stake_info::StakeInfo,
    subnet_info::{SubnetHyperparams, SubnetHyperparamsV2, SubnetInfo, SubnetInfov2},
};
use smallvec::smallvec;
use sp_api::impl_runtime_apis;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{
    H160, H256, OpaqueMetadata, U256,
    crypto::{ByteArray, KeyTypeId},
};
use sp_runtime::Cow;
use sp_runtime::generic::Era;
use sp_runtime::{
    AccountId32, ApplyExtrinsicResult, ConsensusEngineId, generic, impl_opaque_keys,
    traits::{
        AccountIdLookup, BlakeTwo256, Block as BlockT, DispatchInfoOf, Dispatchable, NumberFor,
        One, PostDispatchInfoOf, UniqueSaturatedInto, Verify,
    },
    transaction_validity::{TransactionSource, TransactionValidity, TransactionValidityError},
};
use sp_std::cmp::Ordering;
use sp_std::prelude::*;
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;
use subtensor_precompiles::Precompiles;
use subtensor_runtime_common::{time::*, *};

// A few exports that help ease life for downstream crates.
pub use frame_support::{
    StorageValue, construct_runtime, parameter_types,
    traits::{
        ConstBool, ConstU8, ConstU32, ConstU64, ConstU128, FindAuthor, InstanceFilter,
        KeyOwnerProofSystem, OnFinalize, OnTimestampSet, PrivilegeCmp, Randomness, StorageInfo,
    },
    weights::{
        IdentityFee, Weight, WeightToFeeCoefficient, WeightToFeeCoefficients,
        WeightToFeePolynomial,
        constants::{
            BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND,
        },
    },
};
pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_timestamp::Call as TimestampCall;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use sp_runtime::{Perbill, Permill};

use core::marker::PhantomData;

use scale_info::TypeInfo;

// Frontier
use fp_rpc::TransactionStatus;
use pallet_ethereum::{Call::transact, PostLogContent, Transaction as EthereumTransaction};
use pallet_evm::{
    Account as EVMAccount, BalanceConverter, EvmBalance, FeeCalculator, Runner, SubstrateBalance,
};

// Drand
impl pallet_drand::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = pallet_drand::crypto::TestAuthId;
    type Verifier = pallet_drand::verifier::QuicknetVerifier;
    type UnsignedPriority = ConstU64<{ 1 << 20 }>;
    type HttpFetchTimeout = ConstU64<1_000>;
}

impl frame_system::offchain::SigningTypes for Runtime {
    type Public = <Signature as Verify>::Signer;
    type Signature = Signature;
}

impl<C> frame_system::offchain::CreateTransactionBase<C> for Runtime
where
    RuntimeCall: From<C>,
{
    type Extrinsic = UncheckedExtrinsic;
    type RuntimeCall = RuntimeCall;
}

impl frame_system::offchain::CreateInherent<pallet_drand::Call<Runtime>> for Runtime {
    fn create_inherent(call: Self::RuntimeCall) -> Self::Extrinsic {
        UncheckedExtrinsic::new_bare(call)
    }
}

impl frame_system::offchain::CreateSignedTransaction<pallet_drand::Call<Runtime>> for Runtime {
    fn create_signed_transaction<
        S: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>,
    >(
        call: RuntimeCall,
        public: Self::Public,
        account: Self::AccountId,
        nonce: Self::Nonce,
    ) -> Option<Self::Extrinsic> {
        use sp_runtime::traits::StaticLookup;

        let address = <Runtime as frame_system::Config>::Lookup::unlookup(account.clone());
        let extra: TransactionExtensions = (
            frame_system::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            frame_system::CheckEra::<Runtime>::from(Era::Immortal),
            check_nonce::CheckNonce::<Runtime>::from(nonce).into(),
            frame_system::CheckWeight::<Runtime>::new(),
            pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(0),
            pallet_subtensor::SubtensorTransactionExtension::<Runtime>::new(),
            frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(true),
        );

        let raw_payload = SignedPayload::new(call.clone(), extra.clone()).ok()?;
        let signature = raw_payload.using_encoded(|payload| S::sign(payload, public))?;

        Some(UncheckedExtrinsic::new_signed(
            call, address, signature, extra,
        ))
    }
}

// Subtensor module
pub use pallet_scheduler;
pub use pallet_subtensor;

// Member type for membership
type MemberCount = u32;

// Method used to calculate the fee of an extrinsic
pub const fn deposit(items: u32, bytes: u32) -> Balance {
    pub const ITEMS_FEE: Balance = 2_000 * 10_000;
    pub const BYTES_FEE: Balance = 100 * 10_000;
    (items as Balance)
        .saturating_mul(ITEMS_FEE)
        .saturating_add((bytes as Balance).saturating_mul(BYTES_FEE))
}

// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
// the specifics of the runtime. They can then be made to be agnostic over specific formats
// of data like extrinsics, allowing for them to continue syncing the network through upgrades
// to even the core data structures.
pub mod opaque {
    use super::*;

    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

    // Opaque block header type.
    pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
    // Opaque block type.
    pub type Block = generic::Block<Header, UncheckedExtrinsic>;
    // Opaque block identifier type.
    pub type BlockId = generic::BlockId<Block>;

    impl_opaque_keys! {
        pub struct SessionKeys {
            pub aura: Aura,
            pub grandpa: Grandpa,
        }
    }
}

// To learn more about runtime versioning, see:
// https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name: Cow::Borrowed("node-subtensor"),
    impl_name: Cow::Borrowed("node-subtensor"),
    authoring_version: 1,
    // The version of the runtime specification. A full node will not attempt to use its native
    //   runtime in substitute for the on-chain Wasm runtime unless all of `spec_name`,
    //   `spec_version`, and `authoring_version` are the same between Wasm and native.
    // This value is set to 100 to notify Polkadot-JS App (https://polkadot.js.org/apps) to use
    //   the compatible custom types.
    spec_version: 292,
    impl_version: 1,
    apis: RUNTIME_API_VERSIONS,
    transaction_version: 1,
    system_version: 1,
};

pub const MAXIMUM_BLOCK_WEIGHT: Weight =
    Weight::from_parts(4u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX);

// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion {
        runtime_version: VERSION,
        can_author_with: Default::default(),
    }
}

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
    pub const BlockHashCount: BlockNumber = 2400;
    pub const Version: RuntimeVersion = VERSION;
    // We allow for 2 seconds of compute with a 6 second average block time.
    pub BlockWeights: frame_system::limits::BlockWeights =
        frame_system::limits::BlockWeights::with_sensible_defaults(
            MAXIMUM_BLOCK_WEIGHT,
            NORMAL_DISPATCH_RATIO,
        );
    pub BlockLength: frame_system::limits::BlockLength = frame_system::limits::BlockLength
        ::max_with_normal_ratio(10 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
    pub const SS58Prefix: u8 = 42;
}

pub struct NoNestingCallFilter;

impl Contains<RuntimeCall> for NoNestingCallFilter {
    fn contains(call: &RuntimeCall) -> bool {
        match call {
            RuntimeCall::Utility(inner) => {
                let calls = match inner {
                    pallet_utility::Call::force_batch { calls } => calls,
                    pallet_utility::Call::batch { calls } => calls,
                    pallet_utility::Call::batch_all { calls } => calls,
                    _ => &Vec::new(),
                };

                !calls.iter().any(|call| {
					matches!(call, RuntimeCall::Utility(inner) if matches!(inner, pallet_utility::Call::force_batch { .. } | pallet_utility::Call::batch_all { .. } | pallet_utility::Call::batch { .. }))
				})
            }
            _ => true,
        }
    }
}

// Configure FRAME pallets to include in runtime.

impl frame_system::Config for Runtime {
    // The basic call filter to use in dispatchable.
    type BaseCallFilter = InsideBoth<SafeMode, NoNestingCallFilter>;
    // Block & extrinsics weights: base values and limits.
    type BlockWeights = BlockWeights;
    // The maximum length of a block (in bytes).
    type BlockLength = BlockLength;
    // The identifier used to distinguish between accounts.
    type AccountId = AccountId;
    // The aggregated dispatch type that is available for extrinsics.
    type RuntimeCall = RuntimeCall;
    // The aggregated runtime tasks.
    type RuntimeTask = RuntimeTask;
    // The lookup mechanism to get account ID from whatever is passed in dispatchers.
    type Lookup = AccountIdLookup<AccountId, ()>;
    // The type for hashing blocks and tries.
    type Hash = Hash;
    // The hashing algorithm used.
    type Hashing = BlakeTwo256;
    // The ubiquitous event type.
    type RuntimeEvent = RuntimeEvent;
    // The ubiquitous origin type.
    type RuntimeOrigin = RuntimeOrigin;
    // Maximum number of block number to block hash mappings to keep (oldest pruned first).
    type BlockHashCount = BlockHashCount;
    // The weight of database operations that the runtime can invoke.
    type DbWeight = RocksDbWeight;
    // Version of the runtime.
    type Version = Version;
    // Converts a module to the index of the module in `construct_runtime!`.
    //
    // This type is being generated by `construct_runtime!`.
    type PalletInfo = PalletInfo;
    // What to do if a new account is created.
    type OnNewAccount = ();
    // What to do if an account is fully reaped from the system.
    type OnKilledAccount = ();
    // The data to be stored in an account.
    type AccountData = pallet_balances::AccountData<Balance>;
    // Weight information for the extrinsics of this pallet.
    type SystemWeightInfo = ();
    // This is used as an identifier of the chain. 42 is the generic substrate prefix.
    type SS58Prefix = SS58Prefix;
    // The set code logic, just the default since we're not a parachain.
    type OnSetCode = ();
    type MaxConsumers = frame_support::traits::ConstU32<16>;
    type Nonce = Nonce;
    type Block = Block;
    type SingleBlockMigrations = Migrations;
    type MultiBlockMigrator = ();
    type PreInherents = ();
    type PostInherents = ();
    type PostTransactions = ();
    type ExtensionsWeightInfo = ();
}

impl pallet_insecure_randomness_collective_flip::Config for Runtime {}

impl pallet_aura::Config for Runtime {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = ConstU32<32>;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
    type SlotDuration = pallet_aura::MinimumPeriodTimesTwo<Runtime>;
}

impl pallet_grandpa::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;

    type KeyOwnerProof = sp_core::Void;

    type WeightInfo = ();
    type MaxAuthorities = ConstU32<32>;
    type MaxSetIdSessionEntries = ConstU64<0>;
    type MaxNominators = ConstU32<20>;

    type EquivocationReportSystem = ();
}

impl pallet_timestamp::Config for Runtime {
    // A timestamp: milliseconds since the unix epoch.
    type Moment = u64;
    type OnTimestampSet = Aura;
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_utility::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    pub const DisallowPermissionlessEnterDuration: BlockNumber = 0;
    pub const DisallowPermissionlessExtendDuration: BlockNumber = 0;

    pub const RootEnterDuration: BlockNumber = 5 * 60 * 24; // 24 hours

    pub const RootExtendDuration: BlockNumber = 5 * 60 * 12; // 12 hours

    pub const DisallowPermissionlessEntering: Option<Balance> = None;
    pub const DisallowPermissionlessExtending: Option<Balance> = None;
    pub const DisallowPermissionlessRelease: Option<BlockNumber> = None;
}

pub struct SafeModeWhitelistedCalls;
impl Contains<RuntimeCall> for SafeModeWhitelistedCalls {
    fn contains(call: &RuntimeCall) -> bool {
        matches!(
            call,
            RuntimeCall::Sudo(_)
                | RuntimeCall::Multisig(_)
                | RuntimeCall::System(_)
                | RuntimeCall::SafeMode(_)
                | RuntimeCall::Timestamp(_)
                | RuntimeCall::SubtensorModule(
                    pallet_subtensor::Call::set_weights { .. }
                        | pallet_subtensor::Call::set_tao_weights { .. }
                        | pallet_subtensor::Call::serve_axon { .. }
                )
                | RuntimeCall::Commitments(pallet_commitments::Call::set_commitment { .. })
        )
    }
}

impl pallet_safe_mode::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type RuntimeHoldReason = RuntimeHoldReason;
    type WhitelistedCalls = SafeModeWhitelistedCalls;
    type EnterDuration = DisallowPermissionlessEnterDuration;
    type ExtendDuration = DisallowPermissionlessExtendDuration;
    type EnterDepositAmount = DisallowPermissionlessEntering;
    type ExtendDepositAmount = DisallowPermissionlessExtending;
    type ForceEnterOrigin = EnsureRootWithSuccess<AccountId, RootEnterDuration>;
    type ForceExtendOrigin = EnsureRootWithSuccess<AccountId, RootExtendDuration>;
    type ForceExitOrigin = EnsureRoot<AccountId>;
    type ForceDepositOrigin = EnsureRoot<AccountId>;
    type Notify = ();
    type ReleaseDelay = DisallowPermissionlessRelease;
    type WeightInfo = pallet_safe_mode::weights::SubstrateWeight<Runtime>;
}

// Existential deposit.
pub const EXISTENTIAL_DEPOSIT: u64 = 500;

impl pallet_balances::Config for Runtime {
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ConstU32<50>;
    type ReserveIdentifier = [u8; 8];
    // The type for recording an account's balance.
    type Balance = Balance;
    // The ubiquitous event type.
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ConstU64<EXISTENTIAL_DEPOSIT>;
    type AccountStore = System;
    type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;

    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
    type FreezeIdentifier = RuntimeFreezeReason;
    type MaxFreezes = ConstU32<50>;
    type DoneSlashHandler = ();
}

pub struct LinearWeightToFee;

impl WeightToFeePolynomial for LinearWeightToFee {
    type Balance = Balance;

    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        let coefficient = WeightToFeeCoefficient {
            coeff_integer: 0,
            coeff_frac: Perbill::from_parts(500_000),
            negative: false,
            degree: 1,
        };

        smallvec!(coefficient)
    }
}

parameter_types! {
    pub const OperationalFeeMultiplier: u8 = 5;
    pub FeeMultiplier: Multiplier = Multiplier::one();
}

/// Deduct the transaction fee from the Subtensor Pallet TotalIssuance when dropping the transaction
/// fee.
pub struct TransactionFeeHandler;
impl
    OnUnbalanced<
        FungibleImbalance<
            u64,
            DecreaseIssuance<AccountId32, pallet_balances::Pallet<Runtime>>,
            IncreaseIssuance<AccountId32, pallet_balances::Pallet<Runtime>>,
        >,
    > for TransactionFeeHandler
{
    fn on_nonzero_unbalanced(
        credit: FungibleImbalance<
            u64,
            DecreaseIssuance<AccountId32, pallet_balances::Pallet<Runtime>>,
            IncreaseIssuance<AccountId32, pallet_balances::Pallet<Runtime>>,
        >,
    ) {
        let ti_before = pallet_subtensor::TotalIssuance::<Runtime>::get();
        pallet_subtensor::TotalIssuance::<Runtime>::put(ti_before.saturating_sub(credit.peek()));
        drop(credit);
    }
}

impl pallet_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = FungibleAdapter<Balances, TransactionFeeHandler>;
    // Convert dispatch weight to a chargeable fee.
    type WeightToFee = LinearWeightToFee;
    type OperationalFeeMultiplier = OperationalFeeMultiplier;
    type LengthToFee = IdentityFee<Balance>;
    type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
    type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
}

// Configure collective pallet for council
parameter_types! {
    pub const CouncilMotionDuration: BlockNumber = 12 * HOURS;
    pub const CouncilMaxProposals: u32 = 10;
    pub const CouncilMaxMembers: u32 = 3;
}

// Configure collective pallet for Senate
parameter_types! {
    pub const SenateMaxMembers: u32 = 12;
}

use pallet_collective::{CanPropose, CanVote, GetVotingMembers};
pub struct CanProposeToTriumvirate;
impl CanPropose<AccountId> for CanProposeToTriumvirate {
    fn can_propose(account: &AccountId) -> bool {
        Triumvirate::is_member(account)
    }
}

pub struct CanVoteToTriumvirate;
impl CanVote<AccountId> for CanVoteToTriumvirate {
    fn can_vote(_: &AccountId) -> bool {
        //Senate::is_member(account)
        false // Disable voting from pallet_collective::vote
    }
}

use pallet_subtensor::{CollectiveInterface, MemberManagement, ProxyInterface};
pub struct ManageSenateMembers;
impl MemberManagement<AccountId> for ManageSenateMembers {
    fn add_member(account: &AccountId) -> DispatchResultWithPostInfo {
        let who = Address::Id(account.clone());
        SenateMembers::add_member(RawOrigin::Root.into(), who)
    }

    fn remove_member(account: &AccountId) -> DispatchResultWithPostInfo {
        let who = Address::Id(account.clone());
        SenateMembers::remove_member(RawOrigin::Root.into(), who)
    }

    fn swap_member(rm: &AccountId, add: &AccountId) -> DispatchResultWithPostInfo {
        let remove = Address::Id(rm.clone());
        let add = Address::Id(add.clone());

        Triumvirate::remove_votes(rm)?;
        SenateMembers::swap_member(RawOrigin::Root.into(), remove, add)
    }

    fn is_member(account: &AccountId) -> bool {
        SenateMembers::members().contains(account)
    }

    fn members() -> Vec<AccountId> {
        SenateMembers::members().into()
    }

    fn max_members() -> u32 {
        SenateMaxMembers::get()
    }
}

pub struct GetSenateMemberCount;
impl GetVotingMembers<MemberCount> for GetSenateMemberCount {
    fn get_count() -> MemberCount {
        SenateMembers::members().len() as u32
    }
}
impl Get<MemberCount> for GetSenateMemberCount {
    fn get() -> MemberCount {
        SenateMaxMembers::get()
    }
}

pub struct TriumvirateVotes;
impl CollectiveInterface<AccountId, Hash, u32> for TriumvirateVotes {
    fn remove_votes(hotkey: &AccountId) -> Result<bool, sp_runtime::DispatchError> {
        Triumvirate::remove_votes(hotkey)
    }

    fn add_vote(
        hotkey: &AccountId,
        proposal: Hash,
        index: u32,
        approve: bool,
    ) -> Result<bool, sp_runtime::DispatchError> {
        Triumvirate::do_vote(hotkey.clone(), proposal, index, approve)
    }
}

type EnsureMajoritySenate =
    pallet_collective::EnsureProportionMoreThan<AccountId, TriumvirateCollective, 1, 2>;

// We call pallet_collective TriumvirateCollective
type TriumvirateCollective = pallet_collective::Instance1;
impl pallet_collective::Config<TriumvirateCollective> for Runtime {
    type RuntimeOrigin = RuntimeOrigin;
    type Proposal = RuntimeCall;
    type RuntimeEvent = RuntimeEvent;
    type MotionDuration = CouncilMotionDuration;
    type MaxProposals = CouncilMaxProposals;
    type MaxMembers = GetSenateMemberCount;
    type DefaultVote = pallet_collective::PrimeDefaultVote;
    type WeightInfo = pallet_collective::weights::SubstrateWeight<Runtime>;
    type SetMembersOrigin = EnsureNever<AccountId>;
    type CanPropose = CanProposeToTriumvirate;
    type CanVote = CanVoteToTriumvirate;
    type GetVotingMembers = GetSenateMemberCount;
}

// We call council members Triumvirate
#[allow(dead_code)]
type TriumvirateMembership = pallet_membership::Instance1;
impl pallet_membership::Config<TriumvirateMembership> for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AddOrigin = EnsureRoot<AccountId>;
    type RemoveOrigin = EnsureRoot<AccountId>;
    type SwapOrigin = EnsureRoot<AccountId>;
    type ResetOrigin = EnsureRoot<AccountId>;
    type PrimeOrigin = EnsureRoot<AccountId>;
    type MembershipInitialized = Triumvirate;
    type MembershipChanged = Triumvirate;
    type MaxMembers = CouncilMaxMembers;
    type WeightInfo = pallet_membership::weights::SubstrateWeight<Runtime>;
}

// We call our top K delegates membership Senate
#[allow(dead_code)]
type SenateMembership = pallet_membership::Instance2;
impl pallet_membership::Config<SenateMembership> for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AddOrigin = EnsureRoot<AccountId>;
    type RemoveOrigin = EnsureRoot<AccountId>;
    type SwapOrigin = EnsureRoot<AccountId>;
    type ResetOrigin = EnsureRoot<AccountId>;
    type PrimeOrigin = EnsureRoot<AccountId>;
    type MembershipInitialized = ();
    type MembershipChanged = ();
    type MaxMembers = SenateMaxMembers;
    type WeightInfo = pallet_membership::weights::SubstrateWeight<Runtime>;
}

impl pallet_sudo::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;

    type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    // According to multisig pallet, key and value size be computed as follows:
    // value size is `4 + sizeof((BlockNumber, Balance, AccountId))` bytes
    // key size is `32 + sizeof(AccountId)` bytes.
    // For our case, One storage item; key size is 32+32=64 bytes; value is size 4+4+8+32 bytes = 48 bytes.
    pub const DepositBase: Balance = deposit(1, 112);
    // Additional storage item size of 32 bytes.
    pub const DepositFactor: Balance = deposit(0, 32);
    pub const MaxSignatories: u32 = 100;
}

impl pallet_multisig::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type DepositBase = DepositBase;
    type DepositFactor = DepositFactor;
    type MaxSignatories = MaxSignatories;
    type WeightInfo = pallet_multisig::weights::SubstrateWeight<Runtime>;
}

// Proxy Pallet config
parameter_types! {
    // One storage item; key size sizeof(AccountId) = 32, value sizeof(Balance) = 8; 40 total
    pub const ProxyDepositBase: Balance = deposit(1, 40);
    // Adding 32 bytes + sizeof(ProxyType) = 32 + 1
    pub const ProxyDepositFactor: Balance = deposit(0, 33);
    pub const MaxProxies: u32 = 20; // max num proxies per acct
    pub const MaxPending: u32 = 15 * 5; // max blocks pending ~15min
    // 16 bytes
    pub const AnnouncementDepositBase: Balance =  deposit(1, 16);
    // 68 bytes per announcement
    pub const AnnouncementDepositFactor: Balance = deposit(0, 68);
}

impl InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, c: &RuntimeCall) -> bool {
        match self {
            ProxyType::Any => true,
            ProxyType::NonTransfer => !matches!(
                c,
                RuntimeCall::Balances(..)
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::schedule_swap_coldkey { .. }
                    )
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey { .. })
            ),
            ProxyType::NonFungibile => !matches!(
                c,
                RuntimeCall::Balances(..)
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake_limit { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::remove_stake_limit { .. }
                    )
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::remove_stake_full_limit { .. }
                    )
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::unstake_all_alpha { .. }
                    )
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake_limit { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::burned_register { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::root_register { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::schedule_swap_coldkey { .. }
                    )
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey { .. })
            ),
            ProxyType::Transfer => matches!(
                c,
                RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { .. })
                    | RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. })
                    | RuntimeCall::Balances(pallet_balances::Call::transfer_all { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake { .. })
            ),
            ProxyType::SmallTransfer => match c {
                RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                    value, ..
                }) => *value < SMALL_TRANSFER_LIMIT,
                RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
                    value,
                    ..
                }) => *value < SMALL_TRANSFER_LIMIT,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
                    alpha_amount,
                    ..
                }) => *alpha_amount < SMALL_TRANSFER_LIMIT,
                _ => false,
            },
            ProxyType::Owner => {
                matches!(
                    c,
                    RuntimeCall::AdminUtils(..)
                        | RuntimeCall::SubtensorModule(
                            pallet_subtensor::Call::set_subnet_identity { .. }
                        )
                        | RuntimeCall::SubtensorModule(
                            pallet_subtensor::Call::update_symbol { .. }
                        )
                ) && !matches!(
                    c,
                    RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_sn_owner_hotkey { .. }
                    )
                )
            }
            ProxyType::NonCritical => !matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::dissolve_network { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::root_register { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::burned_register { .. })
                    | RuntimeCall::Triumvirate(..)
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_tao_weights { .. })
                    | RuntimeCall::Sudo(..)
            ),
            ProxyType::Triumvirate => matches!(
                c,
                RuntimeCall::Triumvirate(..) | RuntimeCall::TriumvirateMembers(..)
            ),
            ProxyType::Senate => matches!(c, RuntimeCall::SenateMembers(..)),
            ProxyType::Governance => matches!(
                c,
                RuntimeCall::SenateMembers(..)
                    | RuntimeCall::Triumvirate(..)
                    | RuntimeCall::TriumvirateMembers(..)
            ),
            ProxyType::Staking => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::unstake_all_alpha { .. }
                    )
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake_limit { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_stake { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake_limit { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::remove_stake_limit { .. }
                    )
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::remove_stake_full_limit { .. }
                    )
            ),
            ProxyType::Registration => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::burned_register { .. })
                    | RuntimeCall::SubtensorModule(pallet_subtensor::Call::register { .. })
            ),
            ProxyType::RootWeights => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_tao_weights { .. })
            ),
            ProxyType::ChildKeys => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_children { .. })
                    | RuntimeCall::SubtensorModule(
                        pallet_subtensor::Call::set_childkey_take { .. }
                    )
            ),
            ProxyType::SudoUncheckedSetCode => match c {
                RuntimeCall::Sudo(pallet_sudo::Call::sudo_unchecked_weight { call, weight: _ }) => {
                    let inner_call: RuntimeCall = *call.clone();

                    matches!(
                        inner_call,
                        RuntimeCall::System(frame_system::Call::set_code { .. })
                    )
                }
                _ => false,
            },
            ProxyType::SwapHotkey => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey { .. })
            ),
            ProxyType::SubnetLeaseBeneficiary => matches!(
                c,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::start_call { .. })
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_serving_rate_limit { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_min_difficulty { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_max_difficulty { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_weights_version_key { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_adjustment_alpha { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_max_weight_limit { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_immunity_period { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_min_allowed_weights { .. }
                    )
                    | RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_kappa { .. })
                    | RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_rho { .. })
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_activity_cutoff { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_network_registration_allowed { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_network_pow_registration_allowed { .. }
                    )
                    | RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_burn { .. })
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_bonds_moving_average { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_bonds_penalty { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_commit_reveal_weights_enabled { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_liquid_alpha_enabled { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_alpha_values { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_commit_reveal_weights_interval { .. }
                    )
                    | RuntimeCall::AdminUtils(
                        pallet_admin_utils::Call::sudo_set_toggle_transfer { .. }
                    )
            ),
        }
    }
    fn is_superset(&self, o: &Self) -> bool {
        match (self, o) {
            (x, y) if x == y => true,
            (ProxyType::Any, _) => true,
            (_, ProxyType::Any) => false,
            (ProxyType::NonTransfer, _) => {
                // NonTransfer is NOT a superset of Transfer or SmallTransfer
                !matches!(o, ProxyType::Transfer | ProxyType::SmallTransfer)
            }
            (ProxyType::Governance, ProxyType::Triumvirate | ProxyType::Senate) => true,
            (ProxyType::Transfer, ProxyType::SmallTransfer) => true,
            _ => false,
        }
    }
}

impl pallet_proxy::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProxyType = ProxyType;
    type ProxyDepositBase = ProxyDepositBase;
    type ProxyDepositFactor = ProxyDepositFactor;
    type MaxProxies = MaxProxies;
    type WeightInfo = pallet_proxy::weights::SubstrateWeight<Runtime>;
    type MaxPending = MaxPending;
    type CallHasher = BlakeTwo256;
    type AnnouncementDepositBase = AnnouncementDepositBase;
    type AnnouncementDepositFactor = AnnouncementDepositFactor;
}

pub struct Proxier;
impl ProxyInterface<AccountId> for Proxier {
    fn add_lease_beneficiary_proxy(lease: &AccountId, beneficiary: &AccountId) -> DispatchResult {
        pallet_proxy::Pallet::<Runtime>::add_proxy_delegate(
            lease,
            beneficiary.clone(),
            ProxyType::SubnetLeaseBeneficiary,
            0,
        )
    }

    fn remove_lease_beneficiary_proxy(
        lease: &AccountId,
        beneficiary: &AccountId,
    ) -> DispatchResult {
        pallet_proxy::Pallet::<Runtime>::remove_proxy_delegate(
            lease,
            beneficiary.clone(),
            ProxyType::SubnetLeaseBeneficiary,
            0,
        )
    }
}

parameter_types! {
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        BlockWeights::get().max_block;
    pub const MaxScheduledPerBlock: u32 = 50;
    pub const NoPreimagePostponement: Option<u32> = Some(10);
}

/// Used the compare the privilege of an origin inside the scheduler.
pub struct OriginPrivilegeCmp;

impl PrivilegeCmp<OriginCaller> for OriginPrivilegeCmp {
    fn cmp_privilege(left: &OriginCaller, right: &OriginCaller) -> Option<Ordering> {
        if left == right {
            return Some(Ordering::Equal);
        }

        match (left, right) {
            // Root is greater than anything.
            (OriginCaller::system(frame_system::RawOrigin::Root), _) => Some(Ordering::Greater),
            // Check which one has more yes votes.
            (
                OriginCaller::Triumvirate(pallet_collective::RawOrigin::Members(
                    l_yes_votes,
                    l_count,
                )),
                OriginCaller::Triumvirate(pallet_collective::RawOrigin::Members(
                    r_yes_votes,
                    r_count,
                )), // Equivalent to (l_yes_votes / l_count).cmp(&(r_yes_votes / r_count))
            ) => Some(
                l_yes_votes
                    .saturating_mul(*r_count)
                    .cmp(&r_yes_votes.saturating_mul(*l_count)),
            ),
            // For every other origin we don't care, as they are not used for `ScheduleOrigin`.
            _ => None,
        }
    }
}

impl pallet_scheduler::Config for Runtime {
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeEvent = RuntimeEvent;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = MaxScheduledPerBlock;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
    type OriginPrivilegeCmp = OriginPrivilegeCmp;
    type Preimages = Preimage;
}

parameter_types! {
    pub const PreimageMaxSize: u32 = 4096 * 1024;
    pub const PreimageBaseDeposit: Balance = deposit(2, 64);
    pub const PreimageByteDeposit: Balance = deposit(0, 1);
    pub const PreimageHoldReason: RuntimeHoldReason =
        RuntimeHoldReason::Preimage(pallet_preimage::HoldReason::Preimage);
}

impl pallet_preimage::Config for Runtime {
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountId>;
    type Consideration = HoldConsideration<
        AccountId,
        Balances,
        PreimageHoldReason,
        LinearStoragePrice<PreimageBaseDeposit, PreimageByteDeposit, Balance>,
    >;
}

pub struct AllowIdentityReg;

impl CanRegisterIdentity<AccountId> for AllowIdentityReg {
    #[cfg(not(feature = "runtime-benchmarks"))]
    fn can_register(address: &AccountId, identified: &AccountId) -> bool {
        if address != identified {
            SubtensorModule::coldkey_owns_hotkey(address, identified)
                && SubtensorModule::is_hotkey_registered_on_network(NetUid::ROOT, identified)
        } else {
            SubtensorModule::is_subnet_owner(address)
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn can_register(_: &AccountId, _: &AccountId) -> bool {
        true
    }
}

// Configure registry pallet.
parameter_types! {
    pub const MaxAdditionalFields: u32 = 1;
    pub const InitialDeposit: Balance = 100_000_000; // 0.1 TAO
    pub const FieldDeposit: Balance = 100_000_000; // 0.1 TAO
}

impl pallet_registry::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeHoldReason = RuntimeHoldReason;
    type Currency = Balances;
    type CanRegister = AllowIdentityReg;
    type WeightInfo = pallet_registry::weights::SubstrateWeight<Runtime>;

    type MaxAdditionalFields = MaxAdditionalFields;
    type InitialDeposit = InitialDeposit;
    type FieldDeposit = FieldDeposit;
}

parameter_types! {
    pub const MaxCommitFieldsInner: u32 = 3;
    pub const CommitmentInitialDeposit: Balance = 0; // Free
    pub const CommitmentFieldDeposit: Balance = 0; // Free
}

#[subtensor_macros::freeze_struct("7c76bd954afbb54e")]
#[derive(Clone, Eq, PartialEq, Encode, Decode, TypeInfo)]
pub struct MaxCommitFields;
impl Get<u32> for MaxCommitFields {
    fn get() -> u32 {
        MaxCommitFieldsInner::get()
    }
}

#[subtensor_macros::freeze_struct("c39297f5eb97ee82")]
pub struct AllowCommitments;
impl CanCommit<AccountId> for AllowCommitments {
    #[cfg(not(feature = "runtime-benchmarks"))]
    fn can_commit(netuid: NetUid, address: &AccountId) -> bool {
        SubtensorModule::is_hotkey_registered_on_network(netuid, address)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn can_commit(_: NetUid, _: &AccountId) -> bool {
        true
    }
}

pub struct ResetBondsOnCommit;
impl OnMetadataCommitment<AccountId> for ResetBondsOnCommit {
    #[cfg(not(feature = "runtime-benchmarks"))]
    fn on_metadata_commitment(netuid: NetUid, address: &AccountId) {
        let _ = SubtensorModule::do_reset_bonds(netuid, address);
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn on_metadata_commitment(_: NetUid, _: &AccountId) {}
}

impl pallet_commitments::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type WeightInfo = pallet_commitments::weights::SubstrateWeight<Runtime>;

    type CanCommit = AllowCommitments;
    type OnMetadataCommitment = ResetBondsOnCommit;

    type MaxFields = MaxCommitFields;
    type InitialDeposit = CommitmentInitialDeposit;
    type FieldDeposit = CommitmentFieldDeposit;
    type TempoInterface = TempoInterface;
}

pub struct TempoInterface;
impl pallet_commitments::GetTempoInterface for TempoInterface {
    fn get_epoch_index(netuid: NetUid, cur_block: u64) -> u64 {
        SubtensorModule::get_epoch_index(netuid, cur_block)
    }
}

impl pallet_commitments::GetTempoInterface for Runtime {
    fn get_epoch_index(netuid: NetUid, cur_block: u64) -> u64 {
        SubtensorModule::get_epoch_index(netuid, cur_block)
    }
}

#[cfg(not(feature = "fast-blocks"))]
pub const INITIAL_SUBNET_TEMPO: u16 = 360;

#[cfg(feature = "fast-blocks")]
pub const INITIAL_SUBNET_TEMPO: u16 = 10;

#[cfg(not(feature = "fast-blocks"))]
pub const INITIAL_CHILDKEY_TAKE_RATELIMIT: u64 = 216000; // 30 days at 12 seconds per block

#[cfg(feature = "fast-blocks")]
pub const INITIAL_CHILDKEY_TAKE_RATELIMIT: u64 = 5;

// Configure the pallet subtensor.
parameter_types! {
    pub const SubtensorInitialRho: u16 = 10;
    pub const SubtensorInitialAlphaSigmoidSteepness: i16 = 1000;
    pub const SubtensorInitialKappa: u16 = 32_767; // 0.5 = 65535/2
    pub const SubtensorInitialMaxAllowedUids: u16 = 4096;
    pub const SubtensorInitialIssuance: u64 = 0;
    pub const SubtensorInitialMinAllowedWeights: u16 = 1024;
    pub const SubtensorInitialEmissionValue: u16 = 0;
    pub const SubtensorInitialMaxWeightsLimit: u16 = 1000; // 1000/2^16 = 0.015
    pub const SubtensorInitialValidatorPruneLen: u64 = 1;
    pub const SubtensorInitialScalingLawPower: u16 = 50; // 0.5
    pub const SubtensorInitialMaxAllowedValidators: u16 = 128;
    pub const SubtensorInitialTempo: u16 = INITIAL_SUBNET_TEMPO;
    pub const SubtensorInitialDifficulty: u64 = 10_000_000;
    pub const SubtensorInitialAdjustmentInterval: u16 = 100;
    pub const SubtensorInitialAdjustmentAlpha: u64 = 0; // no weight to previous value.
    pub const SubtensorInitialTargetRegistrationsPerInterval: u16 = 2;
    pub const SubtensorInitialImmunityPeriod: u16 = 4096;
    pub const SubtensorInitialActivityCutoff: u16 = 5000;
    pub const SubtensorInitialMaxRegistrationsPerBlock: u16 = 1;
    pub const SubtensorInitialPruningScore : u16 = u16::MAX;
    pub const SubtensorInitialBondsMovingAverage: u64 = 900_000;
    pub const SubtensorInitialBondsPenalty: u16 = u16::MAX;
    pub const SubtensorInitialBondsResetOn: bool = false;
    pub const SubtensorInitialDefaultTake: u16 = 11_796; // 18% honest number.
    pub const SubtensorInitialMinDelegateTake: u16 = 0; // Allow 0% delegate take
    pub const SubtensorInitialDefaultChildKeyTake: u16 = 0; // Allow 0% childkey take
    pub const SubtensorInitialMinChildKeyTake: u16 = 0; // 0 %
    pub const SubtensorInitialMaxChildKeyTake: u16 = 11_796; // 18 %
    pub const SubtensorInitialWeightsVersionKey: u64 = 0;
    pub const SubtensorInitialMinDifficulty: u64 = 10_000_000;
    pub const SubtensorInitialMaxDifficulty: u64 = u64::MAX / 4;
    pub const SubtensorInitialServingRateLimit: u64 = 50;
    pub const SubtensorInitialBurn: u64 = 100_000_000; // 0.1 tao
    pub const SubtensorInitialMinBurn: u64 = 500_000; // 500k RAO
    pub const SubtensorInitialMaxBurn: u64 = 100_000_000_000; // 100 tao
    pub const SubtensorInitialTxRateLimit: u64 = 1000;
    pub const SubtensorInitialTxDelegateTakeRateLimit: u64 = 216000; // 30 days at 12 seconds per block
    pub const SubtensorInitialTxChildKeyTakeRateLimit: u64 = INITIAL_CHILDKEY_TAKE_RATELIMIT;
    pub const SubtensorInitialRAORecycledForRegistration: u64 = 0; // 0 rao
    pub const SubtensorInitialSenateRequiredStakePercentage: u64 = 1; // 1 percent of total stake
    pub const SubtensorInitialNetworkImmunity: u64 = 7 * 7200;
    pub const SubtensorInitialMinAllowedUids: u16 = 128;
    pub const SubtensorInitialMinLockCost: u64 = 1_000_000_000_000; // 1000 TAO
    pub const SubtensorInitialSubnetOwnerCut: u16 = 11_796; // 18 percent
    // pub const SubtensorInitialSubnetLimit: u16 = 12; // (DEPRECATED)
    pub const SubtensorInitialNetworkLockReductionInterval: u64 = 14 * 7200;
    pub const SubtensorInitialNetworkRateLimit: u64 = 7200;
    pub const SubtensorInitialKeySwapCost: u64 = 100_000_000; // 0.1 TAO
    pub const InitialAlphaHigh: u16 = 58982; // Represents 0.9 as per the production default
    pub const InitialAlphaLow: u16 = 45875; // Represents 0.7 as per the production default
    pub const InitialLiquidAlphaOn: bool = false; // Default value for LiquidAlphaOn
    pub const InitialYuma3On: bool = false; // Default value for Yuma3On
    // pub const SubtensorInitialNetworkMaxStake: u64 = u64::MAX; // (DEPRECATED)
    pub const InitialColdkeySwapScheduleDuration: BlockNumber = 5 * 24 * 60 * 60 / 12; // 5 days
    pub const InitialColdkeySwapRescheduleDuration: BlockNumber = 24 * 60 * 60 / 12; // 1 day
    pub const InitialDissolveNetworkScheduleDuration: BlockNumber = 5 * 24 * 60 * 60 / 12; // 5 days
    pub const SubtensorInitialTaoWeight: u64 = 971_718_665_099_567_868; // 0.05267697438728329% tao weight.
    pub const InitialEmaPriceHalvingPeriod: u64 = 201_600_u64; // 4 weeks
    pub const DurationOfStartCall: u64 = if cfg!(feature = "fast-blocks") {
        10 // Only 10 blocks for fast blocks
    } else {
        7 * 24 * 60 * 60 / 12 // 7 days
    };
    pub const SubtensorInitialKeySwapOnSubnetCost: u64 = 1_000_000; // 0.001 TAO
    pub const HotkeySwapOnSubnetInterval : BlockNumber = 5 * 24 * 60 * 60 / 12; // 5 days
    pub const LeaseDividendsDistributionInterval: BlockNumber = 100; // 100 blocks
}

impl pallet_subtensor::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type SudoRuntimeCall = RuntimeCall;
    type Currency = Balances;
    type CouncilOrigin = EnsureMajoritySenate;
    type SenateMembers = ManageSenateMembers;
    type TriumvirateInterface = TriumvirateVotes;
    type Scheduler = Scheduler;
    type InitialRho = SubtensorInitialRho;
    type InitialAlphaSigmoidSteepness = SubtensorInitialAlphaSigmoidSteepness;
    type InitialKappa = SubtensorInitialKappa;
    type InitialMaxAllowedUids = SubtensorInitialMaxAllowedUids;
    type InitialBondsMovingAverage = SubtensorInitialBondsMovingAverage;
    type InitialBondsPenalty = SubtensorInitialBondsPenalty;
    type InitialBondsResetOn = SubtensorInitialBondsResetOn;
    type InitialIssuance = SubtensorInitialIssuance;
    type InitialMinAllowedWeights = SubtensorInitialMinAllowedWeights;
    type InitialEmissionValue = SubtensorInitialEmissionValue;
    type InitialMaxWeightsLimit = SubtensorInitialMaxWeightsLimit;
    type InitialValidatorPruneLen = SubtensorInitialValidatorPruneLen;
    type InitialScalingLawPower = SubtensorInitialScalingLawPower;
    type InitialTempo = SubtensorInitialTempo;
    type InitialDifficulty = SubtensorInitialDifficulty;
    type InitialAdjustmentInterval = SubtensorInitialAdjustmentInterval;
    type InitialAdjustmentAlpha = SubtensorInitialAdjustmentAlpha;
    type InitialTargetRegistrationsPerInterval = SubtensorInitialTargetRegistrationsPerInterval;
    type InitialImmunityPeriod = SubtensorInitialImmunityPeriod;
    type InitialActivityCutoff = SubtensorInitialActivityCutoff;
    type InitialMaxRegistrationsPerBlock = SubtensorInitialMaxRegistrationsPerBlock;
    type InitialPruningScore = SubtensorInitialPruningScore;
    type InitialMaxAllowedValidators = SubtensorInitialMaxAllowedValidators;
    type InitialDefaultDelegateTake = SubtensorInitialDefaultTake;
    type InitialDefaultChildKeyTake = SubtensorInitialDefaultChildKeyTake;
    type InitialMinDelegateTake = SubtensorInitialMinDelegateTake;
    type InitialMinChildKeyTake = SubtensorInitialMinChildKeyTake;
    type InitialWeightsVersionKey = SubtensorInitialWeightsVersionKey;
    type InitialMaxDifficulty = SubtensorInitialMaxDifficulty;
    type InitialMinDifficulty = SubtensorInitialMinDifficulty;
    type InitialServingRateLimit = SubtensorInitialServingRateLimit;
    type InitialBurn = SubtensorInitialBurn;
    type InitialMaxBurn = SubtensorInitialMaxBurn;
    type InitialMinBurn = SubtensorInitialMinBurn;
    type InitialTxRateLimit = SubtensorInitialTxRateLimit;
    type InitialTxDelegateTakeRateLimit = SubtensorInitialTxDelegateTakeRateLimit;
    type InitialTxChildKeyTakeRateLimit = SubtensorInitialTxChildKeyTakeRateLimit;
    type InitialMaxChildKeyTake = SubtensorInitialMaxChildKeyTake;
    type InitialRAORecycledForRegistration = SubtensorInitialRAORecycledForRegistration;
    type InitialSenateRequiredStakePercentage = SubtensorInitialSenateRequiredStakePercentage;
    type InitialNetworkImmunityPeriod = SubtensorInitialNetworkImmunity;
    type InitialNetworkMinAllowedUids = SubtensorInitialMinAllowedUids;
    type InitialNetworkMinLockCost = SubtensorInitialMinLockCost;
    type InitialNetworkLockReductionInterval = SubtensorInitialNetworkLockReductionInterval;
    type InitialSubnetOwnerCut = SubtensorInitialSubnetOwnerCut;
    type InitialNetworkRateLimit = SubtensorInitialNetworkRateLimit;
    type KeySwapCost = SubtensorInitialKeySwapCost;
    type AlphaHigh = InitialAlphaHigh;
    type AlphaLow = InitialAlphaLow;
    type LiquidAlphaOn = InitialLiquidAlphaOn;
    type Yuma3On = InitialYuma3On;
    type InitialTaoWeight = SubtensorInitialTaoWeight;
    type Preimages = Preimage;
    type InitialColdkeySwapScheduleDuration = InitialColdkeySwapScheduleDuration;
    type InitialColdkeySwapRescheduleDuration = InitialColdkeySwapRescheduleDuration;
    type InitialDissolveNetworkScheduleDuration = InitialDissolveNetworkScheduleDuration;
    type InitialEmaPriceHalvingPeriod = InitialEmaPriceHalvingPeriod;
    type DurationOfStartCall = DurationOfStartCall;
    type SwapInterface = Swap;
    type KeySwapOnSubnetCost = SubtensorInitialKeySwapOnSubnetCost;
    type HotkeySwapOnSubnetInterval = HotkeySwapOnSubnetInterval;
    type ProxyInterface = Proxier;
    type LeaseDividendsDistributionInterval = LeaseDividendsDistributionInterval;
}

parameter_types! {
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const SwapMaxFeeRate: u16 = 10000; // 15.26%
    pub const SwapMaxPositions: u32 = 100;
    pub const SwapMinimumLiquidity: u64 = 1_000;
    pub const SwapMinimumReserve: NonZeroU64 = NonZeroU64::new(1_000_000)
        .expect("1_000_000 fits NonZeroU64");
}

impl pallet_subtensor_swap::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type SubnetInfo = SubtensorModule;
    type BalanceOps = SubtensorModule;
    type ProtocolId = SwapProtocolId;
    type MaxFeeRate = SwapMaxFeeRate;
    type MaxPositions = SwapMaxPositions;
    type MinimumLiquidity = SwapMinimumLiquidity;
    type MinimumReserve = SwapMinimumReserve;
    // TODO: set measured weights when the pallet been benchmarked and the type is generated
    type WeightInfo = pallet_subtensor_swap::weights::DefaultWeight<Runtime>;
}

use sp_runtime::BoundedVec;

pub struct AuraPalletIntrf;
impl pallet_admin_utils::AuraInterface<AuraId, ConstU32<32>> for AuraPalletIntrf {
    fn change_authorities(new: BoundedVec<AuraId, ConstU32<32>>) {
        Aura::change_authorities(new);
    }
}

pub struct GrandpaInterfaceImpl;
impl pallet_admin_utils::GrandpaInterface<Runtime> for GrandpaInterfaceImpl {
    fn schedule_change(
        next_authorities: Vec<(pallet_grandpa::AuthorityId, u64)>,
        in_blocks: BlockNumber,
        forced: Option<BlockNumber>,
    ) -> sp_runtime::DispatchResult {
        Grandpa::schedule_change(next_authorities, in_blocks, forced)
    }
}

impl pallet_admin_utils::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type AuthorityId = AuraId;
    type MaxAuthorities = ConstU32<32>;
    type Aura = AuraPalletIntrf;
    type Grandpa = GrandpaInterfaceImpl;
    type Balance = Balance;
}

/// Define the ChainId
/// EVM Chain ID will be set by sudo transaction for each chain
///     Mainnet Finney: 0x03C4 - Unicode for lowercase tau
///     TestNet Finney: 0x03B1 - Unicode for lowercase alpha
impl pallet_evm_chain_id::Config for Runtime {}

pub struct ConfigurableChainId;

impl Get<u64> for ConfigurableChainId {
    fn get() -> u64 {
        pallet_evm_chain_id::ChainId::<Runtime>::get()
    }
}

pub struct FindAuthorTruncated<F>(PhantomData<F>);
impl<F: FindAuthor<u32>> FindAuthor<H160> for FindAuthorTruncated<F> {
    fn find_author<'a, I>(digests: I) -> Option<H160>
    where
        I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
    {
        if let Some(author_index) = F::find_author(digests) {
            pallet_aura::Authorities::<Runtime>::get()
                .get(author_index as usize)
                .and_then(|authority_id| {
                    let raw_vec = authority_id.to_raw_vec();
                    raw_vec.get(4..24).map(H160::from_slice)
                })
        } else {
            None
        }
    }
}

const BLOCK_GAS_LIMIT: u64 = 75_000_000;

/// `WeightPerGas` is an approximate ratio of the amount of Weight per Gas.
///
fn weight_per_gas() -> Weight {
    (NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT)
        .checked_div(BLOCK_GAS_LIMIT)
        .unwrap_or_default()
}

parameter_types! {
    pub BlockGasLimit: U256 = U256::from(BLOCK_GAS_LIMIT);
    pub const GasLimitPovSizeRatio: u64 = 0;
    pub PrecompilesValue: Precompiles<Runtime> = Precompiles::<_>::new();
    pub WeightPerGas: Weight = weight_per_gas();
}

/// The difference between EVM decimals and Substrate decimals.
/// Substrate balances has 9 decimals, while EVM has 18, so the
/// difference factor is 9 decimals, or 10^9
const EVM_TO_SUBSTRATE_DECIMALS: u64 = 1_000_000_000_u64;

pub struct SubtensorEvmBalanceConverter;

impl BalanceConverter for SubtensorEvmBalanceConverter {
    /// Convert from Substrate balance (u64) to EVM balance (U256)
    fn into_evm_balance(value: SubstrateBalance) -> Option<EvmBalance> {
        let value = value.into_u256();
        if let Some(evm_value) = value.checked_mul(U256::from(EVM_TO_SUBSTRATE_DECIMALS)) {
            // Ensure the result fits within the maximum U256 value
            if evm_value <= U256::MAX {
                Some(EvmBalance::new(evm_value))
            } else {
                // Log value too large
                log::debug!(
                    "SubtensorEvmBalanceConverter::into_evm_balance( {:?} ) larger than U256::MAX",
                    value
                );
                None
            }
        } else {
            // Log overflow
            log::debug!(
                "SubtensorEvmBalanceConverter::into_evm_balance( {:?} ) overflow",
                value
            );
            None
        }
    }

    /// Convert from EVM balance (U256) to Substrate balance (u64)
    fn into_substrate_balance(value: EvmBalance) -> Option<SubstrateBalance> {
        let value = value.into_u256();
        if let Some(substrate_value) = value.checked_div(U256::from(EVM_TO_SUBSTRATE_DECIMALS)) {
            // Ensure the result fits within the TAO balance type (u64)
            if substrate_value <= U256::from(u64::MAX) {
                Some(SubstrateBalance::new(substrate_value))
            } else {
                // Log value too large
                log::debug!(
                    "SubtensorEvmBalanceConverter::into_substrate_balance( {:?} ) larger than u64::MAX",
                    value
                );
                None
            }
        } else {
            // Log overflow
            log::debug!(
                "SubtensorEvmBalanceConverter::into_substrate_balance( {:?} ) overflow",
                value
            );
            None
        }
    }
}

impl pallet_evm::Config for Runtime {
    type FeeCalculator = BaseFee;
    type GasWeightMapping = pallet_evm::FixedGasWeightMapping<Self>;
    type WeightPerGas = WeightPerGas;
    type BlockHashMapping = pallet_ethereum::EthereumBlockHashMapping<Self>;
    type CallOrigin = pallet_evm::EnsureAddressTruncated;
    type WithdrawOrigin = pallet_evm::EnsureAddressTruncated;
    type AddressMapping = pallet_evm::HashedAddressMapping<BlakeTwo256>;
    type Currency = Balances;
    type RuntimeEvent = RuntimeEvent;
    type PrecompilesType = Precompiles<Self>;
    type PrecompilesValue = PrecompilesValue;
    type ChainId = ConfigurableChainId;
    type BlockGasLimit = BlockGasLimit;
    type Runner = pallet_evm::runner::stack::Runner<Self>;
    type OnChargeTransaction = ();
    type OnCreate = ();
    type FindAuthor = FindAuthorTruncated<Aura>;
    type GasLimitPovSizeRatio = GasLimitPovSizeRatio;
    type Timestamp = Timestamp;
    type WeightInfo = pallet_evm::weights::SubstrateWeight<Self>;
    type BalanceConverter = SubtensorEvmBalanceConverter;
    type AccountProvider = pallet_evm::FrameSystemAccountProvider<Self>;
    type GasLimitStorageGrowthRatio = ();
}

parameter_types! {
    pub const PostBlockAndTxnHashes: PostLogContent = PostLogContent::BlockAndTxnHashes;
}

// Required for the IntermediateStateRoot
impl sp_core::Get<sp_version::RuntimeVersion> for Runtime {
    fn get() -> sp_version::RuntimeVersion {
        VERSION
    }
}

impl pallet_ethereum::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type StateRoot = pallet_ethereum::IntermediateStateRoot<Self>;
    type PostLogContent = PostBlockAndTxnHashes;
    type ExtraDataLength = ConstU32<30>;
}

parameter_types! {
    pub BoundDivision: U256 = U256::from(1024);
}

parameter_types! {
    pub DefaultBaseFeePerGas: U256 = U256::from(20_000_000_000_u128);
    pub DefaultElasticity: Permill = Permill::from_parts(125_000);
}
pub struct BaseFeeThreshold;
impl pallet_base_fee::BaseFeeThreshold for BaseFeeThreshold {
    fn lower() -> Permill {
        Permill::zero()
    }
    fn ideal() -> Permill {
        Permill::from_parts(500_000)
    }
    fn upper() -> Permill {
        Permill::from_parts(1_000_000)
    }
}
impl pallet_base_fee::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Threshold = BaseFeeThreshold;
    type DefaultBaseFeePerGas = DefaultBaseFeePerGas;
    type DefaultElasticity = DefaultElasticity;
}

#[derive(Clone)]
pub struct TransactionConverter<B>(PhantomData<B>);

impl<B> Default for TransactionConverter<B> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<B: BlockT> fp_rpc::ConvertTransaction<<B as BlockT>::Extrinsic> for TransactionConverter<B> {
    fn convert_transaction(
        &self,
        transaction: pallet_ethereum::Transaction,
    ) -> <B as BlockT>::Extrinsic {
        let extrinsic = UncheckedExtrinsic::new_bare(
            pallet_ethereum::Call::<Runtime>::transact { transaction }.into(),
        );
        let encoded = extrinsic.encode();
        <B as BlockT>::Extrinsic::decode(&mut &encoded[..])
            .expect("Encoded extrinsic is always valid")
    }
}

impl fp_self_contained::SelfContainedCall for RuntimeCall {
    type SignedInfo = H160;

    fn is_self_contained(&self) -> bool {
        match self {
            RuntimeCall::Ethereum(call) => call.is_self_contained(),
            _ => false,
        }
    }

    fn check_self_contained(&self) -> Option<Result<Self::SignedInfo, TransactionValidityError>> {
        match self {
            RuntimeCall::Ethereum(call) => call.check_self_contained(),
            _ => None,
        }
    }

    fn validate_self_contained(
        &self,
        info: &Self::SignedInfo,
        dispatch_info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> Option<TransactionValidity> {
        match self {
            RuntimeCall::Ethereum(call) => call.validate_self_contained(info, dispatch_info, len),
            _ => None,
        }
    }

    fn pre_dispatch_self_contained(
        &self,
        info: &Self::SignedInfo,
        dispatch_info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> Option<Result<(), TransactionValidityError>> {
        match self {
            RuntimeCall::Ethereum(call) => {
                call.pre_dispatch_self_contained(info, dispatch_info, len)
            }
            _ => None,
        }
    }

    fn apply_self_contained(
        self,
        info: Self::SignedInfo,
    ) -> Option<sp_runtime::DispatchResultWithInfo<PostDispatchInfoOf<Self>>> {
        match self {
            call @ RuntimeCall::Ethereum(pallet_ethereum::Call::transact { .. }) => {
                Some(call.dispatch(RuntimeOrigin::from(
                    pallet_ethereum::RawOrigin::EthereumTransaction(info),
                )))
            }
            _ => None,
        }
    }
}

// Crowdloan
parameter_types! {
    pub const CrowdloanPalletId: PalletId = PalletId(*b"bt/cloan");
    pub const MinimumDeposit: Balance = 10_000_000_000; // 10 TAO
    pub const AbsoluteMinimumContribution: Balance = 100_000_000; // 0.1 TAO
    pub const MinimumBlockDuration: BlockNumber = if cfg!(feature = "fast-blocks") {
        50
    } else {
        50400 // 7 days minimum (7 * 24 * 60 * 60 / 12)
    };
    pub const MaximumBlockDuration: BlockNumber = if cfg!(feature = "fast-blocks") {
       20000
    } else {
        432000 // 60 days maximum (60 * 24 * 60 * 60 / 12)
    };
    pub const RefundContributorsLimit: u32 = 50;
    pub const MaxContributors: u32 = 500;
}

impl pallet_crowdloan::Config for Runtime {
    type PalletId = CrowdloanPalletId;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type WeightInfo = pallet_crowdloan::weights::SubstrateWeight<Runtime>;
    type Preimages = Preimage;
    type MinimumDeposit = MinimumDeposit;
    type AbsoluteMinimumContribution = AbsoluteMinimumContribution;
    type MinimumBlockDuration = MinimumBlockDuration;
    type MaximumBlockDuration = MaximumBlockDuration;
    type RefundContributorsLimit = RefundContributorsLimit;
    type MaxContributors = MaxContributors;
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
    pub struct Runtime
    {
        System: frame_system = 0,
        RandomnessCollectiveFlip: pallet_insecure_randomness_collective_flip = 1,
        Timestamp: pallet_timestamp = 2,
        Aura: pallet_aura = 3,
        Grandpa: pallet_grandpa = 4,
        Balances: pallet_balances = 5,
        TransactionPayment: pallet_transaction_payment = 6,
        SubtensorModule: pallet_subtensor = 7,
        Triumvirate: pallet_collective::<Instance1>::{Pallet, Call, Storage, Origin<T>, Event<T>, Config<T>} = 8,
        TriumvirateMembers: pallet_membership::<Instance1>::{Pallet, Call, Storage, Event<T>, Config<T>} = 9,
        SenateMembers: pallet_membership::<Instance2>::{Pallet, Call, Storage, Event<T>, Config<T>} = 10,
        Utility: pallet_utility = 11,
        Sudo: pallet_sudo = 12,
        Multisig: pallet_multisig = 13,
        Preimage: pallet_preimage = 14,
        Scheduler: pallet_scheduler = 15,
        Proxy: pallet_proxy = 16,
        Registry: pallet_registry = 17,
        Commitments: pallet_commitments = 18,
        AdminUtils: pallet_admin_utils = 19,
        SafeMode: pallet_safe_mode = 20,

        // Frontier
        Ethereum: pallet_ethereum = 21,
        EVM: pallet_evm = 22,
        EVMChainId: pallet_evm_chain_id = 23,
        // pallet_dynamic_fee was 24
        BaseFee: pallet_base_fee = 25,

        Drand: pallet_drand = 26,
        Crowdloan: pallet_crowdloan = 27,
        Swap: pallet_subtensor_swap = 28,
    }
);

// The address format for describing accounts.
pub type Address = sp_runtime::MultiAddress<AccountId, ()>;
// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
// The extensions to the basic transaction logic.
pub type TransactionExtensions = (
    frame_system::CheckNonZeroSender<Runtime>,
    frame_system::CheckSpecVersion<Runtime>,
    frame_system::CheckTxVersion<Runtime>,
    frame_system::CheckGenesis<Runtime>,
    frame_system::CheckEra<Runtime>,
    check_nonce::CheckNonce<Runtime>,
    frame_system::CheckWeight<Runtime>,
    pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
    pallet_subtensor::SubtensorTransactionExtension<Runtime>,
    frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
);

type Migrations = (
    // Leave this migration in the runtime, so every runtime upgrade tiny rounding errors (fractions of fractions
    // of a cent) are cleaned up. These tiny rounding errors occur due to floating point coversion.
    pallet_subtensor::migrations::migrate_init_total_issuance::initialise_total_issuance::Migration<
        Runtime,
    >,
);

// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
    fp_self_contained::UncheckedExtrinsic<Address, RuntimeCall, Signature, TransactionExtensions>;

/// Extrinsic type that has already been checked.
pub type CheckedExtrinsic =
    fp_self_contained::CheckedExtrinsic<AccountId, RuntimeCall, TransactionExtensions, H160>;

// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TransactionExtensions>;
// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
    Runtime,
    Block,
    frame_system::ChainContext<Runtime>,
    Runtime,
    AllPalletsWithSystem,
    Migrations,
>;

#[cfg(feature = "runtime-benchmarks")]
#[macro_use]
extern crate frame_benchmarking;

#[cfg(feature = "runtime-benchmarks")]
mod benches {
    define_benchmarks!(
        [frame_benchmarking, BaselineBench::<Runtime>]
        [frame_system, SystemBench::<Runtime>]
        [pallet_balances, Balances]
        [pallet_timestamp, Timestamp]
        [pallet_sudo, Sudo]
        [pallet_registry, Registry]
        [pallet_commitments, Commitments]
        [pallet_admin_utils, AdminUtils]
        [pallet_subtensor, SubtensorModule]
        [pallet_drand, Drand]
        [pallet_crowdloan, Crowdloan]
        [pallet_subtensor_swap, Swap]
    );
}

fn generate_genesis_json() -> Vec<u8> {
    let json_str = r#"{
      "aura": {
        "authorities": [
          "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
        ]
      },
      "balances": {
        "balances": [
          [
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            1000000000000000
          ],
          [
            "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty",
            1000000000000000
          ]
        ]
      },
      "grandpa": {
        "authorities": [
          [
            "5FA9nQDVg267DEd8m1ZypXLBnvN7SFxYwV7ndqSYGiN9TTpu",
            1
          ]
        ]
      },
      "sudo": {
        "key": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
      },
      "subtensorModule": {
        "balancesIssuance": 0,
        "stakes": []
      }
    }"#;

    json_str.as_bytes().to_vec()
}

impl_runtime_apis! {
    impl sp_api::Core<Block> for Runtime {
        fn version() -> RuntimeVersion {
            VERSION
        }

        fn execute_block(block: Block) {
            Executive::execute_block(block);
        }

        fn initialize_block(header: &<Block as BlockT>::Header) -> sp_runtime::ExtrinsicInclusionMode {
            Executive::initialize_block(header)
        }
    }

    impl sp_api::Metadata<Block> for Runtime {
        fn metadata() -> OpaqueMetadata {
            OpaqueMetadata::new(Runtime::metadata().into())
        }

        fn metadata_at_version(version: u32) -> Option<OpaqueMetadata> {
            Runtime::metadata_at_version(version)
        }

        fn metadata_versions() -> sp_std::vec::Vec<u32> {
            Runtime::metadata_versions()
        }
    }

    impl sp_block_builder::BlockBuilder<Block> for Runtime {
        fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
            Executive::apply_extrinsic(extrinsic)
        }

        fn finalize_block() -> <Block as BlockT>::Header {
            Executive::finalize_block()
        }

        fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
            data.create_extrinsics()
        }

        fn check_inherents(
            block: Block,
            data: sp_inherents::InherentData,
        ) -> sp_inherents::CheckInherentsResult {
            data.check_extrinsics(&block)
        }
    }

    impl sp_genesis_builder::GenesisBuilder<Block> for Runtime {
        fn build_state(config: Vec<u8>) -> sp_genesis_builder::Result {
            build_state::<RuntimeGenesisConfig>(config)
        }

        fn get_preset(id: &Option<sp_genesis_builder::PresetId>) -> Option<Vec<u8>> {
            get_preset::<RuntimeGenesisConfig>(id, |preset_id| {
                let benchmark_id: sp_genesis_builder::PresetId = "benchmark".into();
                if *preset_id == benchmark_id {
                    Some(generate_genesis_json())
                } else {
                    None
                }
            })
        }

        fn preset_names() -> Vec<sp_genesis_builder::PresetId> {
            vec!["benchmark".into()]
        }
    }

    impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
        fn validate_transaction(
            source: TransactionSource,
            tx: <Block as BlockT>::Extrinsic,
            block_hash: <Block as BlockT>::Hash,
        ) -> TransactionValidity {
            use codec::DecodeLimit;
            use frame_support::pallet_prelude::{InvalidTransaction, TransactionValidityError};
            use frame_support::traits::ExtrinsicCall;
            let encoded = tx.call().encode();
            if RuntimeCall::decode_all_with_depth_limit(8, &mut encoded.as_slice()).is_err() {
                log::warn!("failed to decode with depth limit of 8");
                return Err(TransactionValidityError::Invalid(InvalidTransaction::Call));
            }
            Executive::validate_transaction(source, tx, block_hash)
        }
    }

    impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
        fn offchain_worker(header: &<Block as BlockT>::Header) {
            Executive::offchain_worker(header)
        }
    }

    impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
        fn slot_duration() -> sp_consensus_aura::SlotDuration {
            sp_consensus_aura::SlotDuration::from_millis(Aura::slot_duration())
        }

        fn authorities() -> Vec<AuraId> {
            pallet_aura::Authorities::<Runtime>::get().into_inner()
        }
    }

    impl sp_session::SessionKeys<Block> for Runtime {
        fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
            opaque::SessionKeys::generate(seed)
        }

        fn decode_session_keys(
            encoded: Vec<u8>,
        ) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
            opaque::SessionKeys::decode_into_raw_public_keys(&encoded)
        }
    }

    impl fg_primitives::GrandpaApi<Block> for Runtime {
        fn grandpa_authorities() -> GrandpaAuthorityList {
            Grandpa::grandpa_authorities()
        }

        fn current_set_id() -> fg_primitives::SetId {
            Grandpa::current_set_id()
        }

        fn submit_report_equivocation_unsigned_extrinsic(
            _equivocation_proof: fg_primitives::EquivocationProof<
                <Block as BlockT>::Hash,
                NumberFor<Block>,
            >,
            _key_owner_proof: fg_primitives::OpaqueKeyOwnershipProof,
        ) -> Option<()> {
            None
        }

        fn generate_key_ownership_proof(
            _set_id: fg_primitives::SetId,
            _authority_id: GrandpaId,
        ) -> Option<fg_primitives::OpaqueKeyOwnershipProof> {
            // NOTE: this is the only implementation possible since we've
            // defined our key owner proof type as a bottom type (i.e. a type
            // with no values).
            None
        }
    }

    impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
        fn account_nonce(account: AccountId) -> Index {
            System::account_nonce(account)
        }
    }

    impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
        fn query_info(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_info(uxt, len)
        }
        fn query_fee_details(
            uxt: <Block as BlockT>::Extrinsic,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_fee_details(uxt, len)
        }
        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, RuntimeCall>
        for Runtime
    {
        fn query_call_info(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
            TransactionPayment::query_call_info(call, len)
        }
        fn query_call_fee_details(
            call: RuntimeCall,
            len: u32,
        ) -> pallet_transaction_payment::FeeDetails<Balance> {
            TransactionPayment::query_call_fee_details(call, len)
        }
        fn query_weight_to_fee(weight: Weight) -> Balance {
            TransactionPayment::weight_to_fee(weight)
        }
        fn query_length_to_fee(length: u32) -> Balance {
            TransactionPayment::length_to_fee(length)
        }
    }

    impl fp_rpc::EthereumRuntimeRPCApi<Block> for Runtime {
        fn chain_id() -> u64 {
            <Runtime as pallet_evm::Config>::ChainId::get()
        }

        fn account_basic(address: H160) -> EVMAccount {
            let (account, _) = pallet_evm::Pallet::<Runtime>::account_basic(&address);
            account
        }

        fn gas_price() -> U256 {
            let (gas_price, _) = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price();
            gas_price
        }

        fn account_code_at(address: H160) -> Vec<u8> {
            pallet_evm::AccountCodes::<Runtime>::get(address)
        }

        fn author() -> H160 {
            <pallet_evm::Pallet<Runtime>>::find_author()
        }

        fn storage_at(address: H160, index: U256) -> H256 {
            let index_hash = H256::from_slice(&index.to_big_endian());
            pallet_evm::AccountStorages::<Runtime>::get(address, index_hash)
        }

        fn call(
            from: H160,
            to: H160,
            data: Vec<u8>,
            value: U256,
            gas_limit: U256,
            max_fee_per_gas: Option<U256>,
            max_priority_fee_per_gas: Option<U256>,
            nonce: Option<U256>,
            estimate: bool,
            access_list: Option<Vec<(H160, Vec<H256>)>>,
        ) -> Result<pallet_evm::CallInfo, sp_runtime::DispatchError> {
            use pallet_evm::GasWeightMapping as _;

            let config = if estimate {
                let mut config = <Runtime as pallet_evm::Config>::config().clone();
                config.estimate = true;
                Some(config)
            } else {
                None
            };

                    // Estimated encoded transaction size must be based on the heaviest transaction
                    // type (EIP1559Transaction) to be compatible with all transaction types.
                    let mut estimated_transaction_len = data.len() +
                        // pallet ethereum index: 1
                        // transact call index: 1
                        // Transaction enum variant: 1
                        // chain_id 8 bytes
                        // nonce: 32
                        // max_priority_fee_per_gas: 32
                        // max_fee_per_gas: 32
                        // gas_limit: 32
                        // action: 21 (enum varianrt + call address)
                        // value: 32
                        // access_list: 1 (empty vec size)
                        // 65 bytes signature
                        258;

                    if access_list.is_some() {
                        estimated_transaction_len += access_list.encoded_size();
                    }


                    let gas_limit = if gas_limit > U256::from(u64::MAX) {
                        u64::MAX
                    } else {
                        gas_limit.low_u64()
                    };
            let without_base_extrinsic_weight = true;

            let (weight_limit, proof_size_base_cost) =
                match <Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(
                    gas_limit,
                    without_base_extrinsic_weight
                ) {
                    weight_limit if weight_limit.proof_size() > 0 => {
                        (Some(weight_limit), Some(estimated_transaction_len as u64))
                    }
                    _ => (None, None),
                };

            <Runtime as pallet_evm::Config>::Runner::call(
                from,
                to,
                data,
                value,
                gas_limit.unique_saturated_into(),
                max_fee_per_gas,
                max_priority_fee_per_gas,
                nonce,
                access_list.unwrap_or_default(),
                false,
                true,
                weight_limit,
                proof_size_base_cost,
                config.as_ref().unwrap_or(<Runtime as pallet_evm::Config>::config()),
            ).map_err(|err| err.error.into())
        }

        fn create(
            from: H160,
            data: Vec<u8>,
            value: U256,
            gas_limit: U256,
            max_fee_per_gas: Option<U256>,
            max_priority_fee_per_gas: Option<U256>,
            nonce: Option<U256>,
            estimate: bool,
            access_list: Option<Vec<(H160, Vec<H256>)>>,
        ) -> Result<pallet_evm::CreateInfo, sp_runtime::DispatchError> {
            use pallet_evm::GasWeightMapping as _;

            let config = if estimate {
                let mut config = <Runtime as pallet_evm::Config>::config().clone();
                config.estimate = true;
                Some(config)
            } else {
                None
            };


            let mut estimated_transaction_len = data.len() +
                // from: 20
                // value: 32
                // gas_limit: 32
                // nonce: 32
                // 1 byte transaction action variant
                // chain id 8 bytes
                // 65 bytes signature
                190;

            if max_fee_per_gas.is_some() {
                estimated_transaction_len += 32;
            }
            if max_priority_fee_per_gas.is_some() {
                estimated_transaction_len += 32;
            }
            if access_list.is_some() {
                estimated_transaction_len += access_list.encoded_size();
            }


            let gas_limit = if gas_limit > U256::from(u64::MAX) {
                u64::MAX
            } else {
                gas_limit.low_u64()
            };
            let without_base_extrinsic_weight = true;

            let (weight_limit, proof_size_base_cost) =
                match <Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(
                    gas_limit,
                    without_base_extrinsic_weight
                ) {
                    weight_limit if weight_limit.proof_size() > 0 => {
                        (Some(weight_limit), Some(estimated_transaction_len as u64))
                    }
                    _ => (None, None),
                };

            let whitelist = pallet_evm::WhitelistedCreators::<Runtime>::get();
            let whitelist_disabled = pallet_evm::DisableWhitelistCheck::<Runtime>::get();
            <Runtime as pallet_evm::Config>::Runner::create(
                from,
                data,
                value,
                gas_limit.unique_saturated_into(),
                max_fee_per_gas,
                max_priority_fee_per_gas,
                nonce,
                access_list.unwrap_or_default(),
                whitelist,
                whitelist_disabled,
                false,
                true,
                weight_limit,
                proof_size_base_cost,
                config.as_ref().unwrap_or(<Runtime as pallet_evm::Config>::config()),
            ).map_err(|err| err.error.into())
        }

        fn current_transaction_statuses() -> Option<Vec<TransactionStatus>> {
            pallet_ethereum::CurrentTransactionStatuses::<Runtime>::get()
        }

        fn current_block() -> Option<pallet_ethereum::Block> {
            pallet_ethereum::CurrentBlock::<Runtime>::get()
        }

        fn current_receipts() -> Option<Vec<pallet_ethereum::Receipt>> {
            pallet_ethereum::CurrentReceipts::<Runtime>::get()
        }

        fn current_all() -> (
            Option<pallet_ethereum::Block>,
            Option<Vec<pallet_ethereum::Receipt>>,
            Option<Vec<TransactionStatus>>
        ) {
            (
                pallet_ethereum::CurrentBlock::<Runtime>::get(),
                pallet_ethereum::CurrentReceipts::<Runtime>::get(),
                pallet_ethereum::CurrentTransactionStatuses::<Runtime>::get()
            )
        }

        fn extrinsic_filter(
            xts: Vec<<Block as BlockT>::Extrinsic>,
        ) -> Vec<EthereumTransaction> {
            xts.into_iter().filter_map(|xt| match xt.0.function {
                RuntimeCall::Ethereum(transact { transaction }) => Some(transaction),
                _ => None
            }).collect::<Vec<EthereumTransaction>>()
        }

        fn elasticity() -> Option<Permill> {
            Some(pallet_base_fee::Elasticity::<Runtime>::get())
        }

        fn gas_limit_multiplier_support() {}

        fn pending_block(
            xts: Vec<<Block as BlockT>::Extrinsic>,
        ) -> (Option<pallet_ethereum::Block>, Option<Vec<TransactionStatus>>) {
            for ext in xts.into_iter() {
                let _ = Executive::apply_extrinsic(ext);
            }

            Ethereum::on_finalize(System::block_number() + 1);

            (
                pallet_ethereum::CurrentBlock::<Runtime>::get(),
                pallet_ethereum::CurrentTransactionStatuses::<Runtime>::get()
            )
        }

        fn initialize_pending_block(header: &<Block as BlockT>::Header) {
            Executive::initialize_block(header);
        }
    }

    impl fp_rpc::ConvertTransactionRuntimeApi<Block> for Runtime {
        fn convert_transaction(transaction: EthereumTransaction) -> <Block as BlockT>::Extrinsic {
            UncheckedExtrinsic::new_bare(
                pallet_ethereum::Call::<Runtime>::transact { transaction }.into(),
            )
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    impl frame_benchmarking::Benchmark<Block> for Runtime {
        fn benchmark_metadata(extra: bool) -> (
            Vec<frame_benchmarking::BenchmarkList>,
            Vec<frame_support::traits::StorageInfo>,
        ) {
            use frame_benchmarking::{baseline, Benchmarking, BenchmarkList};
            use frame_support::traits::StorageInfoTrait;
            use frame_system_benchmarking::Pallet as SystemBench;
            use baseline::Pallet as BaselineBench;

            let mut list = Vec::<BenchmarkList>::new();
            list_benchmarks!(list, extra);

            let storage_info = AllPalletsWithSystem::storage_info();

            (list, storage_info)
        }

        fn dispatch_benchmark(
            config: frame_benchmarking::BenchmarkConfig
        ) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, alloc::string::String> {
            use frame_benchmarking::{baseline, Benchmarking, BenchmarkBatch};
            use sp_storage::TrackedStorageKey;

            use frame_system_benchmarking::Pallet as SystemBench;
            use baseline::Pallet as BaselineBench;

            #[allow(non_local_definitions)]
            impl frame_system_benchmarking::Config for Runtime {}

            #[allow(non_local_definitions)]
            impl baseline::Config for Runtime {}

            use frame_support::traits::WhitelistedStorageKeys;
            let whitelist: Vec<TrackedStorageKey> = AllPalletsWithSystem::whitelisted_storage_keys();

            let mut batches = Vec::<BenchmarkBatch>::new();
            let params = (&config, &whitelist);
            add_benchmarks!(params, batches);

            Ok(batches)
        }
    }

    #[cfg(feature = "try-runtime")]
    impl frame_try_runtime::TryRuntime<Block> for Runtime {
        #[allow(clippy::unwrap_used)]
        fn on_runtime_upgrade(checks: frame_try_runtime::UpgradeCheckSelect) -> (Weight, Weight) {
            // NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
            // have a backtrace here. If any of the pre/post migration checks fail, we shall stop
            // right here and right now.
            let weight = Executive::try_runtime_upgrade(checks).unwrap();
            (weight, BlockWeights::get().max_block)
        }

        fn execute_block(
            block: Block,
            state_root_check: bool,
            signature_check: bool,
            select: frame_try_runtime::TryStateSelect
        ) -> Weight {
            // NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
            // have a backtrace here.
            Executive::try_execute_block(block, state_root_check, signature_check, select).expect("execute-block failed")
        }
    }

    impl subtensor_custom_rpc_runtime_api::DelegateInfoRuntimeApi<Block> for Runtime {
        fn get_delegates() -> Vec<DelegateInfo<AccountId32>> {
            SubtensorModule::get_delegates()
        }

        fn get_delegate(delegate_account: AccountId32) -> Option<DelegateInfo<AccountId32>> {
            SubtensorModule::get_delegate(delegate_account)
        }

        fn get_delegated(delegatee_account: AccountId32) -> Vec<(DelegateInfo<AccountId32>, (Compact<NetUid>, Compact<u64>))> {
            SubtensorModule::get_delegated(delegatee_account)
        }
    }

    impl subtensor_custom_rpc_runtime_api::NeuronInfoRuntimeApi<Block> for Runtime {
        fn get_neurons_lite(netuid: NetUid) -> Vec<NeuronInfoLite<AccountId32>> {
            SubtensorModule::get_neurons_lite(netuid)
        }

        fn get_neuron_lite(netuid: NetUid, uid: u16) -> Option<NeuronInfoLite<AccountId32>> {
            SubtensorModule::get_neuron_lite(netuid, uid)
        }

        fn get_neurons(netuid: NetUid) -> Vec<NeuronInfo<AccountId32>> {
            SubtensorModule::get_neurons(netuid)
        }

        fn get_neuron(netuid: NetUid, uid: u16) -> Option<NeuronInfo<AccountId32>> {
            SubtensorModule::get_neuron(netuid, uid)
        }
    }

    impl subtensor_custom_rpc_runtime_api::SubnetInfoRuntimeApi<Block> for Runtime {
        fn get_subnet_info(netuid: NetUid) -> Option<SubnetInfo<AccountId32>> {
            SubtensorModule::get_subnet_info(netuid)
        }

        fn get_subnets_info() -> Vec<Option<SubnetInfo<AccountId32>>> {
            SubtensorModule::get_subnets_info()
        }

        fn get_subnet_info_v2(netuid: NetUid) -> Option<SubnetInfov2<AccountId32>> {
            SubtensorModule::get_subnet_info_v2(netuid)
        }

        fn get_subnets_info_v2() -> Vec<Option<SubnetInfov2<AccountId32>>> {
            SubtensorModule::get_subnets_info_v2()
        }

        fn get_subnet_hyperparams(netuid: NetUid) -> Option<SubnetHyperparams> {
            SubtensorModule::get_subnet_hyperparams(netuid)
        }

        fn get_subnet_hyperparams_v2(netuid: NetUid) -> Option<SubnetHyperparamsV2> {
            SubtensorModule::get_subnet_hyperparams_v2(netuid)
        }

        fn get_dynamic_info(netuid: NetUid) -> Option<DynamicInfo<AccountId32>> {
            SubtensorModule::get_dynamic_info(netuid)
        }

        fn get_metagraph(netuid: NetUid) -> Option<Metagraph<AccountId32>> {
            SubtensorModule::get_metagraph(netuid)
        }

        fn get_subnet_state(netuid: NetUid) -> Option<SubnetState<AccountId32>> {
            SubtensorModule::get_subnet_state(netuid)
        }

        fn get_all_metagraphs() -> Vec<Option<Metagraph<AccountId32>>> {
            SubtensorModule::get_all_metagraphs()
        }

        fn get_all_dynamic_info() -> Vec<Option<DynamicInfo<AccountId32>>> {
            SubtensorModule::get_all_dynamic_info()
        }

        fn get_selective_metagraph(netuid: NetUid, metagraph_indexes: Vec<u16>) -> Option<SelectiveMetagraph<AccountId32>> {
            SubtensorModule::get_selective_metagraph(netuid, metagraph_indexes)
        }

    }

    impl subtensor_custom_rpc_runtime_api::StakeInfoRuntimeApi<Block> for Runtime {
        fn get_stake_info_for_coldkey( coldkey_account: AccountId32 ) -> Vec<StakeInfo<AccountId32>> {
            SubtensorModule::get_stake_info_for_coldkey( coldkey_account )
        }

        fn get_stake_info_for_coldkeys( coldkey_accounts: Vec<AccountId32> ) -> Vec<(AccountId32, Vec<StakeInfo<AccountId32>>)> {
            SubtensorModule::get_stake_info_for_coldkeys( coldkey_accounts )
        }

        fn get_stake_info_for_hotkey_coldkey_netuid( hotkey_account: AccountId32, coldkey_account: AccountId32, netuid: NetUid ) -> Option<StakeInfo<AccountId32>> {
            SubtensorModule::get_stake_info_for_hotkey_coldkey_netuid( hotkey_account, coldkey_account, netuid )
        }

        fn get_stake_fee( origin: Option<(AccountId32, NetUid)>, origin_coldkey_account: AccountId32, destination: Option<(AccountId32, NetUid)>, destination_coldkey_account: AccountId32, amount: u64 ) -> u64 {
            SubtensorModule::get_stake_fee( origin, origin_coldkey_account, destination, destination_coldkey_account, amount )
        }
    }

    impl subtensor_custom_rpc_runtime_api::SubnetRegistrationRuntimeApi<Block> for Runtime {
        fn get_network_registration_cost() -> u64 {
            SubtensorModule::get_network_lock_cost()
        }
    }


    impl pallet_subtensor_swap_runtime_api::SwapRuntimeApi<Block> for Runtime {
        fn current_alpha_price(netuid: u16) -> u64 {
            use substrate_fixed::types::U96F32;

            pallet_subtensor_swap::Pallet::<Runtime>::current_price(netuid.into())
                .saturating_mul(U96F32::from_num(1_000_000_000))
                .saturating_to_num()
        }
    }
}

#[test]
fn check_whitelist() {
    use crate::*;
    use frame_support::traits::WhitelistedStorageKeys;
    use sp_core::hexdisplay::HexDisplay;
    use std::collections::HashSet;
    let whitelist: HashSet<String> = AllPalletsWithSystem::whitelisted_storage_keys()
        .iter()
        .map(|e| HexDisplay::from(&e.key).to_string())
        .collect();

    // Block Number
    assert!(whitelist.contains("26aa394eea5630e07c48ae0c9558cef702a5c1b19ab7a04f536c519aca4983ac"));
    // Total Issuance
    assert!(whitelist.contains("c2261276cc9d1f8598ea4b6a74b15c2f57c875e4cff74148e4628f264b974c80"));
    // Execution Phase
    assert!(whitelist.contains("26aa394eea5630e07c48ae0c9558cef7ff553b5a9862a516939d82b3d3d8661a"));
    // Event Count
    assert!(whitelist.contains("26aa394eea5630e07c48ae0c9558cef70a98fdbe9ce6c55837576c60c7af3850"));
    // System Events
    assert!(whitelist.contains("26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7"));
}

#[test]
fn test_into_substrate_balance_valid() {
    // Valid conversion within u64 range
    let evm_balance: EvmBalance = 1_000_000_000_000_000_000u128.into(); // 1 TAO in EVM
    let expected_substrate_balance: SubstrateBalance = 1_000_000_000u128.into(); // 1 TAO in Substrate

    let result = SubtensorEvmBalanceConverter::into_substrate_balance(evm_balance);
    assert_eq!(result, Some(expected_substrate_balance));
}

#[test]
fn test_into_substrate_balance_large_value() {
    // Maximum valid balance for u64
    let evm_balance = EvmBalance::new(U256::from(u64::MAX) * U256::from(EVM_TO_SUBSTRATE_DECIMALS)); // Max u64 TAO in EVM
    let expected_substrate_balance = SubstrateBalance::new(U256::from(u64::MAX));

    let result = SubtensorEvmBalanceConverter::into_substrate_balance(evm_balance);
    assert_eq!(result, Some(expected_substrate_balance));
}

#[test]
fn test_into_substrate_balance_exceeds_u64() {
    // EVM balance that exceeds u64 after conversion
    let evm_balance = EvmBalance::new(
        (U256::from(u64::MAX) + U256::from(1)) * U256::from(EVM_TO_SUBSTRATE_DECIMALS),
    );

    let result = SubtensorEvmBalanceConverter::into_substrate_balance(evm_balance);
    assert_eq!(result, None); // Exceeds u64, should return None
}

#[test]
fn test_into_substrate_balance_precision_loss() {
    // EVM balance with precision loss
    let evm_balance = EvmBalance::new(U256::from(1_000_000_000_123_456_789u128)); // 1 TAO + extra precision in EVM
    let expected_substrate_balance = SubstrateBalance::new(U256::from(1_000_000_000u128)); // Truncated to 1 TAO in Substrate

    let result = SubtensorEvmBalanceConverter::into_substrate_balance(evm_balance);
    assert_eq!(result, Some(expected_substrate_balance));
}

#[test]
fn test_into_substrate_balance_zero_value() {
    // Zero balance should convert to zero
    let evm_balance = EvmBalance::new(U256::from(0));
    let expected_substrate_balance = SubstrateBalance::new(U256::from(0));

    let result = SubtensorEvmBalanceConverter::into_substrate_balance(evm_balance);
    assert_eq!(result, Some(expected_substrate_balance));
}

#[test]
fn test_into_evm_balance_valid() {
    // Valid conversion from Substrate to EVM
    let substrate_balance: SubstrateBalance = 1_000_000_000u128.into(); // 1 TAO in Substrate
    let expected_evm_balance = EvmBalance::new(U256::from(1_000_000_000_000_000_000u128)); // 1 TAO in EVM

    let result = SubtensorEvmBalanceConverter::into_evm_balance(substrate_balance);
    assert_eq!(result, Some(expected_evm_balance));
}

#[test]
fn test_into_evm_balance_overflow() {
    // Substrate balance larger than u64::MAX but valid within U256
    let substrate_balance = SubstrateBalance::new(U256::from(u64::MAX) + U256::from(1)); // Large balance
    let expected_evm_balance =
        EvmBalance::new(substrate_balance.into_u256() * U256::from(EVM_TO_SUBSTRATE_DECIMALS));

    let result = SubtensorEvmBalanceConverter::into_evm_balance(substrate_balance);
    assert_eq!(result, Some(expected_evm_balance)); // Should return the scaled value
}
