// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cybersecurity & platform hardening, per docs/gungnir-capabilities.md §5.5.
//! Nothing else in this workspace specifies identity, access control, or
//! supply-chain assurance -- for a system ingesting external sensor feeds and
//! recommending physical-world actions (via gungnir-command), this is a baseline
//! requirement, not a later-stage hardening pass. Human-owned changes
//! (agentic-workflow.md). Posture by deployment profile: ARCHITECTURE.md §8.5.

pub mod asymmetric;
pub mod audit;
pub mod authn;
pub mod authz;
pub mod keys;
pub mod keystore;
mod os_keystore;
pub mod provider;
pub mod session;
pub mod token;

pub use asymmetric::{
    EscrowOfficerKey, EscrowPublicKey, EscrowedKey, KeystoreSnapshot, P256KeyProvider,
    RecoveredDataKey,
};
pub use audit::{AuditEntry, AuditLog, InMemoryAuditLog};
pub use authn::Authenticator;
pub use authz::{Authorizer, StaticRoleAuthorizer};
pub use keys::SignatureScheme;
pub use keystore::{PersistentKeyProvider, KEYSTORE_FILE};
pub use provider::InProcessKeyProvider;
pub use session::{
    audit_attempt, audit_sign_out, hash_passphrase, verify_account, Account, AccountStore,
    AuthFailure, BackOff, FileAccountStore, InMemoryAccountStore, LocalAccountAuthority,
    MissionTimeSeconds, OperatorSession, SessionAuthority, SessionState,
};
pub use token::TokenIssuer;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecurityError {
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("operator lacks permission: {0}")]
    Forbidden(String),
    /// A key still protects retained data and no override named what would become
    /// unreadable (docs/design/DN-22-key-management.md).
    #[error("key still protects retained data; destroying it needs a recorded override naming the affected data")]
    KeyStillProtectsData,
    /// The custody provider could not perform the operation.
    #[error("key provider unavailable: {0}")]
    KeyProviderUnavailable(String),
    #[error("unknown key {0}")]
    UnknownKey(String),
    /// The account store could not be reached, so nobody can be authenticated.
    ///
    /// A fault, not a rejection: `SessionState::StoreUnavailable` carries it, and a
    /// desktop that cannot reach its keystore still starts (DN-23 §5 rule 5).
    #[error("account store unavailable: {0}")]
    AccountStoreUnavailable(String),
    /// Hashing or verification could not be performed at all.
    #[error("authentication unavailable: {0}")]
    AuthenticationUnavailable(String),
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct OperatorId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Operator,
    Supervisor,
    Analyst,
    SensorManager,
    Administrator,
    /// Adopted by D-05 on 2026-09-04, in code from 2026-09-05 (GAP-068). Holds
    /// area-layer engagement acceptance, weapons control status, and coverage-gap
    /// acceptance (`docs/mission/roles-and-stakeholders.md` §4).
    Commander,
    /// Adopted by D-05. Drafts and rehearses plans; **holds no decision authority.**
    /// The authority matrix has no Planner row, so rather than infer one this role was
    /// given view-only coarse permission by the owner on 2026-09-05 and the planning
    /// surfaces it works in are read-and-draft. Widening it is a change to §4 first.
    Planner,
    /// The escrow holder (DN-22 §11, D-30, 2026-09-06): a named person per deployment
    /// who **operates nothing** and may only recover an escrowed journal key. No
    /// authority rule may grant it an operating action; its layout is the audit and
    /// health panels alone.
    SecurityOfficer,
    /// Adopted by D-05. Intelligence declarations and product release; may *request*
    /// sensor tasking but does not hold tasking authority, which is why it has no
    /// `TASK_SENSOR` permission below.
    IntelligenceAnalyst,
}

impl Role {
    /// Authority rank for conflict resolution (`gungnir-collab`): higher wins.
    ///
    /// The order was set by the owner on 2026-09-05 when the three D-05 roles entered
    /// the code. It preserves every relative order the five original roles had; the
    /// three new ones were placed by their authority in
    /// `docs/mission/roles-and-stakeholders.md` §4. Commander sits above Supervisor
    /// because it holds area-layer acceptance and coverage-gap acceptance that the
    /// supervisor does not; Planner sits low because it holds no decision authority.
    ///
    /// This is not a cosmetic ordering. `gungnir_collab::RoleRankArbiter` decides which
    /// side of a reconciliation conflict survives, so moving a role here changes whose
    /// version of the record wins.
    pub fn rank(self) -> u8 {
        match self {
            // Decides nothing, so no version of the record is ever its to win (D-30).
            Role::SecurityOfficer | Role::Analyst => 0,
            Role::IntelligenceAnalyst => 1,
            Role::SensorManager => 2,
            Role::Planner => 3,
            Role::Operator => 4,
            Role::Supervisor => 5,
            Role::Commander => 6,
            Role::Administrator => 7,
        }
    }

    /// Every role this build knows, for exhaustive tests and for configuration
    /// validation. Kept beside [`Role::rank`] so a new variant cannot be added without
    /// meeting both.
    pub const ALL: &'static [Role] = &[
        Role::Analyst,
        Role::IntelligenceAnalyst,
        Role::SensorManager,
        Role::Planner,
        Role::Operator,
        Role::Supervisor,
        Role::Commander,
        Role::Administrator,
        Role::SecurityOfficer,
    ];
}

/// The actions the authorizer knows about. Strings rather than an enum at the
/// trait boundary so new crates can add actions without touching this one; these
/// constants are the canonical names.
pub use keys::{
    may_destroy, DestructionOverride, EncryptionStatus, KeyId, KeyProvider, KeyPurpose, KeyState,
};

pub mod actions {
    pub const VIEW_PICTURE: &str = "picture.view";
    pub const SUBMIT_DETECTION: &str = "detection.submit";
    pub const DECIDE_PLAN: &str = "plan.decide";
    pub const OVERRIDE_PLAN: &str = "plan.override";
    pub const APPLY_CONFIG: &str = "config.apply";
    pub const TASK_SENSOR: &str = "sensor.task";
    pub const PROMOTE_MODEL: &str = "model.promote";
    pub const EXPORT_REPORT: &str = "report.export";
    /// Setting weapons control status (docs/design/DN-09-authority-and-control-status.md).
    pub const SET_CONTROL_STATUS: &str = "weapons.control_status";
    /// An effector reporting back on a handoff (docs/design/DN-07-handoff.md).
    pub const EFFECTOR_REPORT: &str = "effector.report";
    /// The warned party answering a warning (docs/design/DN-03-warning.md §5 rule 2,
    /// GAP-042). Distinct from [`ACKNOWLEDGE_HANDOVER`], which is a watch changing hands
    /// inside the deployment: this one is an outside party saying it was told, and the
    /// operator form of it is a person keying in what came over the radio. Modelled on
    /// [`EFFECTOR_REPORT`], which is the same shape of fact arriving by the same route.
    pub const ACKNOWLEDGE_WARNING: &str = "warning.acknowledge";
    /// Raising or lowering a releasability marking (docs/design/DN-17-releasability.md).
    pub const RELEASE_PRODUCT: &str = "product.release";
    /// Posting a marked warning, report or handoff to this deployment's node so a
    /// coalition partner's `GET /v2/exchange/{warnings,reports,handoffs}` can serve it
    /// (docs/design/DN-18-coalition-exchange.md amendment 2, GAP-065).
    ///
    /// **Deliberately not [`RELEASE_PRODUCT`].** That action is raising or lowering the
    /// marking itself; this one is transmitting a product that already carries whatever
    /// marking it has. The same split as [`EFFECTOR_REPORT`] and [`ACKNOWLEDGE_WARNING`]:
    /// two acts that touch the same product at different moments are two actions, not
    /// one, because PN-17's "what was exchanged with whom" and PN-20's "marking changes
    /// with the operator who made them" are two different audit facts a shared name
    /// would blur into each other.
    ///
    /// **Signed by the owner the same day.** Granting this to `Commander` and
    /// `IntelligenceAnalyst` in [`crate::authz::role_permits`] mirrors `RELEASE_PRODUCT`'s
    /// existing grant on the reasoning that the roles trusted to mark a product
    /// releasable are the roles trusted to send it, but that was this change's own
    /// judgment call, not a read of an existing row: `docs/mission/roles-and-stakeholders.md`
    /// §4 had no "publish to exchange" row before this change added one, per the
    /// precedent [`crate::actions::ASSIGN_ROLE`]'s doc comment records for widening
    /// authority.
    pub const PUBLISH_EXCHANGE: &str = "exchange.publish";
    /// Conducting an after-action review: open, record, conclude, close, promote
    /// (DN-20 §6). Audited under this name (GAP-059; signed by the owner 2026-09-06);
    /// not yet in the role table.
    pub const REVIEW_CONDUCT: &str = "review.conduct";
    /// Stating, tasking, declining or satisfying a collection requirement (DN-11).
    /// Audited under this name (GAP-059; signed by the owner 2026-09-06); tasking
    /// authority is `TASK_SENSOR`.
    pub const REQUIREMENT: &str = "requirement.state";
    /// Conducting an after-action review (docs/design/DN-20-after-action-review.md).
    pub const CONDUCT_REVIEW: &str = "review.conduct";
    /// Acknowledging a watch handover (docs/design/DN-21-battle-rhythm.md).
    pub const ACKNOWLEDGE_HANDOVER: &str = "handover.acknowledge";
    /// Recovering an escrowed journal key as the security officer (DN-22 §11, GAP-084).
    /// An offline act; audited by the recovery tool into a journal of its own.
    pub const KEY_ESCROW_RECOVER: &str = "key.escrow_recover";
    /// Assigning a role to an account (GAP-057, PN-20). Administrators alone: the
    /// authority matrix in `docs/mission/roles-and-stakeholders.md` §4 names nobody else
    /// for account administration, and widening it means adding the §4 row first.
    pub const ASSIGN_ROLE: &str = "account.assign_role";

    /// Every action this build knows, for validating an authority rule at load.
    pub const ALL: &[&str] = &[
        VIEW_PICTURE,
        SUBMIT_DETECTION,
        DECIDE_PLAN,
        OVERRIDE_PLAN,
        APPLY_CONFIG,
        TASK_SENSOR,
        PROMOTE_MODEL,
        EXPORT_REPORT,
        SET_CONTROL_STATUS,
        EFFECTOR_REPORT,
        ACKNOWLEDGE_WARNING,
        RELEASE_PRODUCT,
        PUBLISH_EXCHANGE,
        CONDUCT_REVIEW,
        ACKNOWLEDGE_HANDOVER,
        KEY_ESCROW_RECOVER,
    ];

    /// True when `action` is one this build knows.
    ///
    /// A misspelled action grants nothing and looks like a grant, so a baseline
    /// naming an unknown one is rejected rather than silently ignored.
    pub fn is_known(action: &str) -> bool {
        ALL.contains(&action)
    }
}
