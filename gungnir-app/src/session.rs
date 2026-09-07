// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Signing in and out on the desktop (GAP-057, DN-23 §5), and what a session changes.
//!
//! `AppState::sign_in` audits every attempt and never invents attribution; this module is
//! its caller. Two things follow a session on a desktop configured for a remote backend:
//! a successful sign-in establishes the node link with the operator's credential, since
//! connecting is an authenticated act, and a sign-out drops it and the desktop runs
//! embedded again -- said on PN-01 through `state.backend`, never left implied.

use crate::state::AppState;
use gungnir_config::BackendConfig;
use gungnir_security::{LocalAccountAuthority, OperatorId, SessionState};
use gungnir_ui::panels::audit::{AccountLine, AuditView, SessionAction, SessionLine, SignInDraft};

/// PN-20's view of the session, the accounts, the handoffs and the audit log.
///
/// `handoffs` is every handoff the desktop issued (`handoffs::rows`, GAP-040), delivered
/// ones included: this is the after-action account, and a delivered handoff is the row a
/// reconstruction needs most.
#[must_use]
pub fn audit_view<'a>(
    state: &'a AppState,
    accounts: &'a [AccountLine<'a>],
    audit: &'a [gungnir_ui::panels::config_editor::AuditLine<'a>],
    handoffs: &'a [gungnir_ui::panels::handoff::HandoffRow<'a>],
) -> AuditView<'a> {
    let session = match state.session_state() {
        SessionState::SignedIn(s) => SessionLine::SignedIn {
            operator: s.operator.0,
            role: format!("{:?}", s.role),
            expires_s: s.expires,
        },
        SessionState::NobodySignedIn => SessionLine::NobodySignedIn,
        SessionState::Expired { operator, at } => SessionLine::Expired {
            operator: operator.0,
            at_s: at,
        },
        SessionState::StoreUnavailable { reason } => SessionLine::StoreUnavailable { reason },
    };
    AuditView {
        session,
        accounts: match &state.accounts {
            Ok(_) => Ok(accounts),
            Err(reason) => Err(reason.as_str()),
        },
        audit,
        handoffs,
        now: state.clock.now(),
        can_sign_in: !matches!(state.session_state(), SessionState::StoreUnavailable { .. }),
        can_assign_roles: gungnir_security::authz::role_permits(
            state.role(),
            gungnir_security::actions::ASSIGN_ROLE,
        ),
    }
}

/// The role a PN-20 name means, or none: the panel offers names because it depends on
/// the model alone.
fn role_named(name: &str) -> Option<gungnir_security::Role> {
    use gungnir_security::Role;
    Some(match name {
        "Operator" => Role::Operator,
        "Supervisor" => Role::Supervisor,
        "Analyst" => Role::Analyst,
        "SensorManager" => Role::SensorManager,
        "Administrator" => Role::Administrator,
        "Commander" => Role::Commander,
        "Planner" => Role::Planner,
        "SecurityOfficer" => Role::SecurityOfficer,
        "IntelligenceAnalyst" => Role::IntelligenceAnalyst,
        _ => return None,
    })
}

/// Assign a role in the account store (GAP-057, PN-20): an authority-checked act on the
/// audit trail, written to the file the authority reads, after which the authority is
/// rebuilt so the change is in force without a restart.
pub fn assign_role(state: &mut AppState, operator: u64, role_name: &str) {
    use gungnir_config::AuthenticationProvider;
    use gungnir_security::{actions, authz::role_permits, FileAccountStore, OperatorId};
    if !role_permits(state.role(), actions::ASSIGN_ROLE) {
        state.alerts.push(format!(
            "role assignment refused: {:?} does not hold {}",
            state.role(),
            actions::ASSIGN_ROLE
        ));
        return;
    }
    let Some(role) = role_named(role_name) else {
        state.alerts.push(format!(
            "role assignment refused: {role_name:?} is not a role"
        ));
        return;
    };
    let AuthenticationProvider::LocalAccounts { accounts_path } =
        state.config.security.authentication.provider.clone()
    else {
        state
            .alerts
            .push("role assignment refused: no account store is configured".into());
        return;
    };
    let path = std::path::Path::new(&state.config.data_dir).join(accounts_path);
    let outcome = FileAccountStore::open(&path).and_then(|mut store| {
        store.assign_role(OperatorId(operator), role)?;
        store.save(&path)?;
        Ok(store.listing())
    });
    match outcome {
        Ok(listing) => {
            state.accounts = Ok(listing);
            crate::audit::record(
                state,
                actions::ASSIGN_ROLE,
                format!("operator {operator} assigned {role:?}"),
            );
            state
                .alerts
                .push(format!("operator {operator} is now {role:?}"));
        }
        Err(err) => state.alerts.push(format!("role assignment failed: {err}")),
    }
}

/// The account rows, owned so the view can borrow them.
#[must_use]
pub fn account_lines(state: &AppState) -> Vec<(u64, String)> {
    state
        .accounts
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|(op, role)| (op.0, format!("{role:?}")))
                .collect()
        })
        .unwrap_or_default()
}

/// Apply what PN-20 asked for.
pub fn apply(state: &mut AppState, draft: &mut SignInDraft, action: SessionAction) {
    match action {
        SessionAction::SignIn => {
            let Ok(operator) = draft.operator.trim().parse::<u64>() else {
                state
                    .alerts
                    .push("sign-in refused: the operator identifier is a number".into());
                draft.passphrase.clear();
                return;
            };
            let credential =
                LocalAccountAuthority::credential(OperatorId(operator), &draft.passphrase);
            let passphrase = std::mem::take(&mut draft.passphrase);
            match state.sign_in(&credential) {
                Ok(session) => {
                    state.alerts.push(format!(
                        "signed in as operator {} ({:?})",
                        session.operator.0, session.role
                    ));
                    connect_if_remote(state, operator, &passphrase);
                    // GAP-084: the keystore opens with the passphrase that just verified.
                    crate::keystore::unlock(state, &draft.passphrase);
                }
                // DN-23 §5 rule 4: one indistinguishable failure for an unknown operator
                // and a bad secret; rule 3: rate-limited, never a lockout.
                Err(err) => state.alerts.push(format!("sign-in refused: {err}")),
            }
        }
        SessionAction::AssignRole { operator, role } => assign_role(state, operator, role),
        SessionAction::SignOut => {
            let was = state.attributed_operator();
            state.sign_out();
            if let Some(op) = was {
                state.alerts.push(format!("operator {} signed out", op.0));
            }
            disconnect_if_remote(state);
        }
    }
}

/// The tick step (GAP-057): a session that lapsed on the clock is announced once, and
/// the link its credential established is dropped, because nothing may act under it.
pub fn sweep_expiry(state: &mut AppState) {
    match state.session_state() {
        SessionState::Expired { operator, at } => {
            if state.expiry_announced == Some(operator) {
                return;
            }
            state.expiry_announced = Some(operator);
            state.alerts.push(format!(
                "operator {}'s session expired at T+{at:.0} s (security.authentication.session_lifetime_s); \
                 nothing is attributed until somebody signs in again",
                operator.0
            ));
            disconnect_if_remote(state);
        }
        _ => state.expiry_announced = None,
    }
}

/// DN-23 §5, the connected profile: a sign-in on a desktop configured for a node is
/// what establishes the link, with that operator's credential.
fn connect_if_remote(state: &mut AppState, operator: u64, passphrase: &str) {
    let BackendConfig::Remote { endpoint } = state.config.backend.clone() else {
        return;
    };
    let credential = gungnir_remote::link::Credential {
        operator,
        passphrase: passphrase.to_string(),
    };
    let remote = gungnir_remote::RemoteEndpoint {
        url: endpoint.clone(),
        tls: link_tls(state),
    };
    match gungnir_remote::connect_with_link(&remote, credential, state.runtime.handle()) {
        Ok((tracking, intercept, link)) => {
            state.tracking = Box::new(tracking);
            state.intercept = Box::new(intercept);
            state.link = Some(link.clone());
            // GAP-004: sensor commands now travel through this link.
            crate::node_tasks::attach(state, link);
            state.fallback = None;
            state.backend = BackendConfig::Remote {
                endpoint: endpoint.clone(),
            };
            state
                .alerts
                .push(format!("connected to node {endpoint}; running remote"));
        }
        Err(err) => state.alerts.push(format!(
            "could not reach node {endpoint}: {err}; running embedded"
        )),
    }
}

/// What the link trusts and who this desktop is (GAP-060).
///
/// The roots are the baseline's. The identity is read from `GUNGNIR_TLS_CERT` and
/// `GUNGNIR_TLS_KEY`, the same development fallback the node uses, because a baseline
/// may not name key material or a path to it; a desktop identity issued through the key
/// provider (DN-22) is GAP-060's remaining half. Either variable unset means no identity,
/// which an `https` node refuses at the handshake and the strip reports.
pub fn link_tls(state: &AppState) -> gungnir_remote::LinkTls {
    link_tls_for(&state.config)
}

/// As [`link_tls`], from the baseline alone, for the peer links bound before the state
/// exists.
pub fn link_tls_for(config: &gungnir_config::ConfigBaseline) -> gungnir_remote::LinkTls {
    let identity_pem = match (
        std::env::var("GUNGNIR_TLS_CERT").ok(),
        std::env::var("GUNGNIR_TLS_KEY").ok(),
    ) {
        (Some(cert), Some(key)) => match (
            std::fs::read_to_string(&cert),
            std::fs::read_to_string(&key),
        ) {
            (Ok(cert), Ok(key)) => Some(format!("{cert}\n{key}")),
            (Err(err), _) => {
                tracing::warn!(path = %cert, %err, "GUNGNIR_TLS_CERT could not be read; connecting without an identity");
                None
            }
            (_, Err(err)) => {
                tracing::warn!(path = %key, %err, "GUNGNIR_TLS_KEY could not be read; connecting without an identity");
                None
            }
        },
        _ => None,
    };
    // The provider first, the environment second. Before 2026-09-06 there was only the
    // environment, and the code called it a fallback while it was the only path -- which
    // described an intention rather than the code (GAP-060).
    // One call, in the crate that owns the type: see `gungnir_remote::identity`. The
    // desktop deliberately does not build the certified key itself, because it would need
    // a `rustls` dependency of its own to name a value it only passes along.
    let issued = gungnir_remote::identity::issue_for_client("gungnir-app")
        .map_err(|err| tracing::warn!(%err, "this desktop could not issue its own identity"))
        .ok();
    if issued.is_some() {
        if identity_pem.is_some() {
            tracing::info!(
                "both an issued identity and GUNGNIR_TLS_CERT/KEY are available; using the \
                 issued one, whose private half never leaves the key provider"
            );
        }
    } else if identity_pem.is_some() {
        tracing::warn!(
            "this desktop could not issue its own identity and is using the \
             GUNGNIR_TLS_CERT/KEY development fallback, whose private key is on disk"
        );
    }
    gungnir_remote::LinkTls {
        trust_roots_pem: config.security.tls.trust_roots_pem.clone(),
        issued,
        identity_pem,
    }
}

/// Signing out drops the link: the credential that established it is gone.
fn disconnect_if_remote(state: &mut AppState) {
    if !matches!(state.backend, BackendConfig::Remote { .. }) {
        return;
    }
    let handle = state.runtime.handle().clone();
    state.tracking = Box::new(
        gungnir_tracking_service::LiveTrackingService::new(&handle)
            .with_staleness(state.config.policy.staleness.clone()),
    );
    state.intercept = Box::new(gungnir_intercept_service::DpInterceptService::new(
        state.config.allocation_horizon,
    ));
    state.backend = BackendConfig::Embedded;
    state.link = None;
    crate::node_tasks::detach(state);
    state.fallback = None;
    state
        .alerts
        .push("signed out: the node link is dropped; running embedded".into());
}
