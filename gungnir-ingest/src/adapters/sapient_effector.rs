// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A SAPIENT effector as a handoff endpoint (GAP-207; DN-07 §5; D-144).
//!
//! **The one effector adapter tranche A builds**, against the SAPIENT task path the owner
//! named in D-144 until a trial partner names its own interface. A handoff goes out as a
//! `Task` (`gungnir_sensor_management::sapient_task::engage_task` builds it), and what the
//! effector answers comes back as `TaskAck`s on the same connection, so **the outcome path
//! needs no listener**: a desktop with no node hears its effector the way it already hears
//! a SAPIENT sensor's acknowledgements, by reading the connection it opened. That is the
//! return path GAP-207 found missing in the disconnected profile.
//!
//! # Delivery and outcome are two things
//!
//! * **Delivered** means the effector has the task, and the only evidence of that SAPIENT
//!   gives is a `TaskAck` naming it, of any status. A write that left this process is not
//!   delivery: the middleware may be up and the effector behind it gone. So a task is
//!   [`TaskDelivery::Acknowledged`] on the first `TaskAck`, [`TaskDelivery::Unreachable`]
//!   when the write failed or no `TaskAck` came within [`ACK_WINDOW`], and waiting until
//!   then. The desk's store-and-forward schedule does the rest.
//! * **What the effector did** is each `TaskAck`, as [`effector_report`] reads it.
//!
//! # Only the effector speaks for the effector
//!
//! A `TaskAck` can close an engagement, so one is taken only when its `nodeId` is the
//! configured `destination_id` and its `taskId` is a task this link sent. A node that
//! registered as a type other than the configured one is refused too. Everything else is
//! counted on [`EffectorLinkStats`] with its reason, never applied. This is the same
//! strength of claim as the gateway's allow-list (`SourceAuthentication::AllowList`): the
//! middleware's connection is trusted to carry the node ids it reports, which is what a
//! SAPIENT deployment's middleware is for.
//!
//! # What is not built
//!
//! * The binary protobuf bearer, for the reason the detection adapter gives.
//! * Task state survives only the session: a desktop restarted mid-engagement no longer
//!   knows the task ids it sent, so a late `TaskAck` is refused as naming a task it did
//!   not send, and said.

use super::sapient::{parse_task_ack, SapientSource, TaskAckReport, TaskAckStatus};
use super::sapient::{TcpSapientSource, TcpTaskSink};
use gungnir_model::handoff::EffectorReport;
use gungnir_model::{DecisionId, MissionTime};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a sent task waits for its first `TaskAck` before it is unreachable.
///
/// Ten seconds: a SAPIENT node answers a task it can parse at once, accepting or
/// rejecting it, so silence this long is a node that does not have it. The desk then
/// posts it again on its own schedule (`gungnir_approval::RETRY_AFTER_S`), and the same
/// task id goes out, so an effector that was only slow does not act twice.
pub const ACK_WINDOW: Duration = Duration::from_secs(10);

/// How long a connection attempt may hold the caller.
///
/// Shorter than the three seconds a feed is given at start, because a reconnect is
/// attempted from the tick that posts a handoff, and a desktop's frame cannot wait three
/// seconds for a middleware that is down. A refused connection returns at once; this
/// bounds one that is silently dropped.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// What the link knows about one task it sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDelivery {
    /// The effector answered it: it has the task.
    Acknowledged,
    /// It did not reach the effector, and why. Worth sending again.
    Unreachable(String),
    /// It can never be sent as it stands, and why. Not worth sending again.
    Unsendable(String),
}

/// What the link has done, for a health line and for a test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectorLinkStats {
    pub connected: bool,
    /// Tasks written whole to the connection.
    pub sent: u64,
    /// `TaskAck`s taken from the effector for a task this link sent.
    pub acks: u64,
    /// Messages refused, keyed by why: a `TaskAck` from another node, one naming a task
    /// this link did not send, a node registered as the wrong type, or a line that was
    /// not a message at all.
    pub refused: BTreeMap<String, u64>,
    /// The last connection failure, while there is one.
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct SentTask {
    at: Instant,
    failed: Option<String>,
    acknowledged: bool,
}

#[derive(Debug, Default)]
struct Inner {
    connection: Option<(TcpSapientSource, TcpTaskSink)>,
    tasks: HashMap<String, SentTask>,
    /// The node type the effector registered as, once it has.
    registered: Option<Vec<String>>,
    stats: EffectorLinkStats,
}

/// One SAPIENT effector, reached through a middleware's TCP port (GAP-207).
#[derive(Debug)]
pub struct SapientEffectorLink {
    name: String,
    addr: SocketAddr,
    destination_id: String,
    node_type: &'static str,
    ack_window: Duration,
    inner: Mutex<Inner>,
}

impl SapientEffectorLink {
    /// A link to the effector `destination_id`, registered as `node_type` (a SAPIENT
    /// `NodeType` name, e.g. `NODE_TYPE_JAMMER`), through the middleware at `addr`. Not
    /// connected until [`SapientEffectorLink::connect`] or the first send.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        addr: SocketAddr,
        destination_id: impl Into<String>,
        node_type: &'static str,
    ) -> Self {
        Self {
            name: name.into(),
            addr,
            destination_id: destination_id.into(),
            node_type,
            ack_window: ACK_WINDOW,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// The same, waiting `window` for a first `TaskAck`: for a test that cannot wait out
    /// [`ACK_WINDOW`].
    #[must_use]
    pub fn with_ack_window(mut self, window: Duration) -> Self {
        self.ack_window = window;
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn destination_id(&self) -> &str {
        &self.destination_id
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // Never held across a panic that matters: every write under it is a counter or a
        // map entry, so a poisoned guard still holds a coherent state.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Open the connection, if it is not open.
    ///
    /// # Errors
    ///
    /// Why it could not be opened, which is also kept on the stats.
    pub fn connect(&self, timeout: Duration) -> Result<(), String> {
        let mut inner = self.lock();
        Self::connect_locked(&mut inner, self.addr, timeout)
    }

    fn connect_locked(
        inner: &mut Inner,
        addr: SocketAddr,
        timeout: Duration,
    ) -> Result<(), String> {
        if inner.connection.is_some() {
            return Ok(());
        }
        let opened = TcpSapientSource::connect(addr, timeout)
            .and_then(|source| source.sink().map(|sink| (source, sink)))
            .map_err(|e| e.to_string());
        match opened {
            Ok(connection) => {
                inner.connection = Some(connection);
                inner.stats.connected = true;
                inner.stats.last_error = None;
                Ok(())
            }
            Err(reason) => {
                inner.stats.connected = false;
                inner.stats.last_error = Some(reason.clone());
                Err(reason)
            }
        }
    }

    /// Write one task, connecting first if the link is down. Whatever happens, the task
    /// is recorded, so [`SapientEffectorLink::delivery`] can say what became of it.
    ///
    /// # Errors
    ///
    /// Why the task did not leave this process.
    pub fn send(&self, task_id: &str, message: &str) -> Result<(), String> {
        let mut inner = self.lock();
        let written =
            Self::connect_locked(&mut inner, self.addr, CONNECT_TIMEOUT).and_then(
                |()| match &inner.connection {
                    Some((_, sink)) => sink.try_send(message),
                    None => Err("not connected".into()),
                },
            );
        if written.is_ok() {
            inner.stats.sent += 1;
        } else {
            // A write that failed leaves the connection in an unknown state, possibly
            // with part of a line on it; the next send opens a fresh one.
            inner.connection = None;
            inner.stats.connected = false;
        }
        let failed = written.as_ref().err().cloned();
        let acknowledged = inner.tasks.get(task_id).is_some_and(|t| t.acknowledged);
        inner.tasks.insert(
            task_id.to_owned(),
            SentTask {
                at: Instant::now(),
                failed,
                acknowledged,
            },
        );
        written
    }

    /// What became of a task this link sent, or `None` while it waits.
    #[must_use]
    pub fn delivery(&self, task_id: &str) -> Option<TaskDelivery> {
        let inner = self.lock();
        let task = inner.tasks.get(task_id)?;
        if task.acknowledged {
            return Some(TaskDelivery::Acknowledged);
        }
        if let Some(reason) = &task.failed {
            return Some(TaskDelivery::Unreachable(format!(
                "{}: {reason}",
                self.name
            )));
        }
        (task.at.elapsed() >= self.ack_window).then(|| {
            TaskDelivery::Unreachable(format!(
                "{}: no TaskAck from the effector within {} s",
                self.name,
                self.ack_window.as_secs_f64()
            ))
        })
    }

    /// Read everything the connection has, and return the `TaskAck`s the effector sent
    /// for tasks this link sent. Everything else is counted and dropped.
    pub fn pump(&self) -> Vec<TaskAckReport> {
        let mut inner = self.lock();
        let lines = match inner
            .connection
            .as_mut()
            .map(|(source, _)| source.take_messages())
        {
            None => return Vec::new(),
            Some(Ok(lines)) => lines,
            Some(Err(err)) => {
                inner.connection = None;
                inner.stats.connected = false;
                inner.stats.last_error = Some(err.to_string());
                return Vec::new();
            }
        };
        let mut taken = Vec::new();
        for line in lines {
            if let Some(ack) = self.take_line(&mut inner, &line) {
                taken.push(ack);
            }
        }
        taken
    }

    fn take_line(&self, inner: &mut Inner, line: &str) -> Option<TaskAckReport> {
        let refuse = |inner: &mut Inner, why: &str| {
            *inner.stats.refused.entry(why.to_owned()).or_default() += 1;
            None
        };
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return refuse(inner, "not a JSON message");
        };
        let from = message.get("nodeId").and_then(Value::as_str);
        if let Some(registration) = message.get("registration") {
            if from == Some(self.destination_id.as_str()) {
                let types = registration
                    .get("nodeDefinition")
                    .and_then(Value::as_array)
                    .map(|defs| {
                        defs.iter()
                            .filter_map(|d| d.get("nodeType").and_then(Value::as_str))
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                inner.registered = Some(types);
            }
            return None;
        }
        let task_ack = message.get("taskAck")?;
        if from != Some(self.destination_id.as_str()) {
            return refuse(inner, "a TaskAck from a node that is not this effector");
        }
        if inner
            .registered
            .as_ref()
            .is_some_and(|types| !types.iter().any(|t| t == self.node_type))
        {
            return refuse(inner, "the effector registered as another node type");
        }
        let Some(ack) = parse_task_ack(task_ack) else {
            return refuse(inner, "a TaskAck missing its task id or status");
        };
        let Some(task) = inner.tasks.get_mut(&ack.task_id) else {
            return refuse(inner, "a TaskAck naming a task this link did not send");
        };
        task.acknowledged = true;
        inner.stats.acks += 1;
        Some(ack)
    }

    #[must_use]
    pub fn stats(&self) -> EffectorLinkStats {
        self.lock().stats.clone()
    }
}

/// What a `TaskAck` says about the handoff, as an [`EffectorReport`] (GAP-207, the owner's
/// choice of 2026-10-27). `at` is when this host read it: a `TaskAck` carries no time of
/// its own beyond its message's, which is the effector's clock.
///
/// | `TaskStatus` | Report |
/// |---|---|
/// | `ACCEPTED` | `Acknowledged` |
/// | `REJECTED` | `Refused`, with the effector's reasons |
/// | `COMPLETED` | `Finished`: over, effect not stated |
/// | `FAILED` | `Completed`, ineffective: it accepted and could not do it |
#[must_use]
pub fn effector_report(ack: &TaskAckReport, at: MissionTime) -> EffectorReport {
    let reasons = ack.reasons.join("; ");
    match ack.status {
        TaskAckStatus::Accepted => EffectorReport::Acknowledged { at },
        TaskAckStatus::Rejected => EffectorReport::Refused {
            at,
            reason: if reasons.is_empty() {
                "the effector rejected the task and gave no reason".into()
            } else {
                reasons
            },
        },
        TaskAckStatus::Completed => EffectorReport::Finished {
            at,
            detail: reasons,
        },
        TaskAckStatus::Failed => EffectorReport::Completed {
            at,
            effective: false,
            detail: if reasons.is_empty() {
                "the effector accepted the task and could not complete it".into()
            } else {
                format!("the effector could not complete the task: {reasons}")
            },
        },
    }
}

/// Every configured SAPIENT effector, by the endpoint name a resource's
/// `handoff_endpoint` uses, and the decision each task was sent for (GAP-207).
///
/// One per host. The correlation is a table rather than folded into the task id: a
/// decision is 128 bits and a ULID has room for 80.
#[derive(Debug, Default)]
pub struct SapientEffectors {
    links: BTreeMap<String, Arc<SapientEffectorLink>>,
    decisions: Mutex<HashMap<String, (String, DecisionId)>>,
}

impl SapientEffectors {
    pub fn insert(&mut self, endpoint: impl Into<String>, link: SapientEffectorLink) {
        self.links.insert(endpoint.into(), Arc::new(link));
    }

    #[must_use]
    pub fn get(&self, endpoint: &str) -> Option<&Arc<SapientEffectorLink>> {
        self.links.get(endpoint)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<SapientEffectorLink>)> {
        self.links.iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// The address a handoff transport posts to for `endpoint`, when a SAPIENT effector is
    /// behind it: the endpoint's name under [`ADDRESS_PREFIX`], so it can never be taken
    /// for an HTTP address.
    #[must_use]
    pub fn address_for(&self, endpoint: &str) -> Option<String> {
        self.links
            .contains_key(endpoint)
            .then(|| format!("{ADDRESS_PREFIX}{endpoint}"))
    }

    /// Send `message` to the effector `address` names, or `None` when the address is not
    /// a SAPIENT effector's. A send that fails is still a post: the link records the
    /// failure, and [`SapientPost::poll`] answers it unreachable so the desk retries.
    #[must_use]
    pub fn post(&self, address: &str, message: &Value) -> Option<SapientPost> {
        let link = self.links.get(address.strip_prefix(ADDRESS_PREFIX)?)?;
        let Some(task_id) = task_id_of(message) else {
            // Unreachable for a payload `engage_task` built; a host that posted anything
            // else is told so rather than sending a task no TaskAck could ever name.
            return Some(SapientPost {
                link: Arc::clone(link),
                task_id: String::new(),
                malformed: true,
            });
        };
        if let Err(reason) = link.send(task_id, &message.to_string()) {
            tracing::warn!(effector = %link.name(), %reason, "a handoff task was not sent");
        }
        Some(SapientPost {
            link: Arc::clone(link),
            task_id: task_id.to_owned(),
            malformed: false,
        })
    }

    /// Remember that `task_id`, sent to `endpoint`, carries `decision`.
    pub fn remember(&self, endpoint: &str, task_id: &str, decision: DecisionId) {
        if let Ok(mut decisions) = self.decisions.lock() {
            decisions.insert(task_id.to_owned(), (endpoint.to_owned(), decision));
        }
    }

    /// Read every effector, and return each report with the decision and endpoint it is
    /// about, in the order the effectors sent them.
    pub fn pump(&self, at: MissionTime) -> Vec<(DecisionId, String, EffectorReport)> {
        let decisions = self.decisions.lock().map(|d| d.clone()).unwrap_or_default();
        let mut reports = Vec::new();
        for link in self.links.values() {
            for ack in link.pump() {
                // A task the link sent always has a decision: `remember` is called with
                // the payload that is sent. A link that took an ack for a task with none
                // would be a host that sent without remembering, and it is said.
                if let Some((endpoint, decision)) = decisions.get(&ack.task_id) {
                    reports.push((*decision, endpoint.clone(), effector_report(&ack, at)));
                } else {
                    tracing::error!(
                        task = %ack.task_id,
                        effector = %link.name(),
                        "a TaskAck for a task sent with no decision remembered"
                    );
                }
            }
        }
        reports
    }
}

/// How a handoff address names a SAPIENT effector: `sapient:<endpoint>`.
pub const ADDRESS_PREFIX: &str = "sapient:";

/// One handoff task sent to an effector and not yet answered.
#[derive(Debug)]
pub struct SapientPost {
    link: Arc<SapientEffectorLink>,
    task_id: String,
    malformed: bool,
}

impl SapientPost {
    /// What became of the task, or `None` while the effector has not answered.
    #[must_use]
    pub fn poll(&self) -> Option<TaskDelivery> {
        if self.malformed {
            return Some(TaskDelivery::Unsendable(format!(
                "{}: the handoff was not a SAPIENT task with a taskId",
                self.link.name()
            )));
        }
        self.link.delivery(&self.task_id)
    }
}

/// The `taskId` of a SAPIENT `Task` message, as `engage_task` built it.
#[must_use]
pub fn task_id_of(message: &Value) -> Option<&str> {
    message.get("task")?.get("taskId")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    const EFFECTOR: &str = "7c0e6a2b-1d3f-4e5a-9b8c-0d1e2f3a4b5c";

    /// A middleware with one effector behind it: it reads each task line and answers with
    /// whatever `answer` makes of the task id.
    fn effector(answer: impl Fn(&str) -> Vec<String> + Send + 'static) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut writer = stream.try_clone().expect("clone");
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { return };
                let task: Value = serde_json::from_str(&line).expect("a task line");
                let id = task_id_of(&task).expect("a task id").to_owned();
                for reply in answer(&id) {
                    let _ = writeln!(writer, "{reply}");
                }
                let _ = writer.flush();
            }
        });
        addr
    }

    fn ack(from: &str, task: &str, status: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-10-27T10:00:00Z","nodeId":"{from}","taskAck":{{"taskId":"{task}","taskStatus":"{status}"}}}}"#
        )
    }

    fn task(id: &str) -> String {
        format!(r#"{{"nodeId":"us","destinationId":"{EFFECTOR}","task":{{"taskId":"{id}"}}}}"#)
    }

    fn until(link: &SapientEffectorLink, want: usize) -> Vec<TaskAckReport> {
        let mut got = Vec::new();
        for _ in 0..400 {
            got.extend(link.pump());
            if got.len() >= want {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    }

    /// The effector accepts and then completes: delivered on the first ack, and both
    /// statuses come back for the reports.
    #[test]
    fn an_acknowledged_task_is_delivered_and_its_acks_come_back() {
        let addr = effector(|id| {
            vec![
                ack(EFFECTOR, id, "TASK_STATUS_ACCEPTED"),
                ack(EFFECTOR, id, "TASK_STATUS_COMPLETED"),
            ]
        });
        let link = SapientEffectorLink::new("jammer", addr, EFFECTOR, "NODE_TYPE_JAMMER");
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        link.send(id, &task(id)).expect("sent");
        assert_eq!(link.delivery(id), None, "waiting for the effector");
        let acks = until(&link, 2);
        let statuses: Vec<_> = acks.iter().map(|a| a.status).collect();
        assert_eq!(
            statuses,
            [TaskAckStatus::Accepted, TaskAckStatus::Completed]
        );
        assert_eq!(link.delivery(id), Some(TaskDelivery::Acknowledged));
        assert_eq!(link.stats().acks, 2);
    }

    /// Only the effector speaks for the effector: an ack from another node, and one for a
    /// task this link never sent, are refused and counted.
    #[test]
    fn an_ack_from_another_node_or_for_another_task_is_refused() {
        let addr = effector(|id| {
            vec![
                ack("someone-else", id, "TASK_STATUS_COMPLETED"),
                ack(
                    EFFECTOR,
                    "01BX5ZZKBKACTAV9WEVGEMMVRZ",
                    "TASK_STATUS_COMPLETED",
                ),
                ack(EFFECTOR, id, "TASK_STATUS_REJECTED"),
            ]
        });
        let link = SapientEffectorLink::new("jammer", addr, EFFECTOR, "NODE_TYPE_JAMMER");
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        link.send(id, &task(id)).expect("sent");
        let acks = until(&link, 1);
        assert_eq!(acks.len(), 1, "{acks:?}");
        assert_eq!(acks[0].status, TaskAckStatus::Rejected);
        let refused = link.stats().refused;
        assert_eq!(
            refused.get("a TaskAck from a node that is not this effector"),
            Some(&1)
        );
        assert_eq!(
            refused.get("a TaskAck naming a task this link did not send"),
            Some(&1)
        );
    }

    /// A node that registered as something else is not the effector configured.
    #[test]
    fn an_effector_registered_as_another_type_is_refused() {
        let addr = effector(|id| {
            vec![
                format!(
                    r#"{{"nodeId":"{EFFECTOR}","registration":{{"nodeDefinition":[{{"nodeType":"NODE_TYPE_CAMERA"}}]}}}}"#
                ),
                ack(EFFECTOR, id, "TASK_STATUS_ACCEPTED"),
            ]
        });
        let link = SapientEffectorLink::new("jammer", addr, EFFECTOR, "NODE_TYPE_JAMMER");
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        link.send(id, &task(id)).expect("sent");
        for _ in 0..100 {
            link.pump();
            if !link.stats().refused.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            link.stats()
                .refused
                .get("the effector registered as another node type"),
            Some(&1)
        );
        assert_eq!(link.delivery(id), None);
    }

    /// Silence past the window is unreachable, and a link with nothing listening fails at
    /// the send and says so.
    #[test]
    fn silence_and_a_dead_middleware_are_unreachable() {
        let addr = effector(|_| Vec::new());
        let link = SapientEffectorLink::new("jammer", addr, EFFECTOR, "NODE_TYPE_JAMMER")
            .with_ack_window(Duration::from_millis(50));
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        link.send(id, &task(id)).expect("sent");
        std::thread::sleep(Duration::from_millis(80));
        assert!(matches!(
            link.delivery(id),
            Some(TaskDelivery::Unreachable(r)) if r.contains("no TaskAck")
        ));

        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            l.local_addr().expect("addr")
        };
        let dead = SapientEffectorLink::new("jammer", closed, EFFECTOR, "NODE_TYPE_JAMMER");
        assert!(dead.send(id, &task(id)).is_err());
        assert!(matches!(
            dead.delivery(id),
            Some(TaskDelivery::Unreachable(_))
        ));
        assert!(!dead.stats().connected);
    }

    #[test]
    fn each_status_becomes_the_report_the_owner_chose() {
        let at = MissionTime(5.0);
        let of = |status, reasons: &[&str]| {
            effector_report(
                &TaskAckReport {
                    task_id: "t".into(),
                    status,
                    reasons: reasons.iter().map(|r| (*r).to_owned()).collect(),
                },
                at,
            )
        };
        assert_eq!(
            of(TaskAckStatus::Accepted, &[]),
            EffectorReport::Acknowledged { at }
        );
        assert!(matches!(
            of(TaskAckStatus::Rejected, &["out of arc"]),
            EffectorReport::Refused { reason, .. } if reason == "out of arc"
        ));
        assert!(matches!(
            of(TaskAckStatus::Completed, &[]),
            EffectorReport::Finished { .. }
        ));
        assert!(matches!(
            of(TaskAckStatus::Failed, &["jammed"]),
            EffectorReport::Completed { effective: false, detail, .. } if detail.contains("jammed")
        ));
    }
}
