// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The cloud node's custody profile: a managed key service, off-host.
//!
//! Design: docs/design/DN-22-key-management.md §14 (amendment 5), which is §5's third
//! row finally given a shape of its own; the row itself is docs/design/DN-22-key-management.md §5.
//! Decision D-42 admitted the crates. Register entry GAP-084.
//!
//! **Human-owned** (docs/agentic-workflow.md: `gungnir-security` decides who can read
//! what); signed by the owner 2026-09-10, over both halves together -- the design
//! (DN-22 amendment 5, §14) and this code -- per the register's own rule that this
//! profile needed both signed at once.
//!
//! **One real defect found and fixed before signing.** Key Vault's `sign` returns an
//! ES256 signature as the raw 64-byte `r || s` concatenation (RFC 7518 §3.4), never the
//! ASN.1 DER encoding this crate's `KeyProvider::sign` contract uses everywhere else --
//! `P256KeyProvider::sign` converts to DER before returning, AWS KMS's own algorithm
//! returns DER natively, and every real caller (`gungnir_remote::identity`'s TLS bridge
//! foremost) parses a signature with `Signature::from_der`. Handed straight through, a
//! signature from an `AzureKeyVaultKeyService`-backed identity would have failed every
//! TLS handshake it signed -- invisible to every test here, since the in-crate fake
//! never modelled either service's real wire format and the two tests that could reach
//! a real vault are `#[ignore]`d for want of credentials. `azure_call`'s `Job::Sign` arm
//! now converts through `azure_signature_to_provider_format`, and a new test constructs
//! a real P-256 signature, takes its raw form exactly as Key Vault's own documentation
//! describes it, and checks the conversion parses with `Signature::from_der` and still
//! verifies.
//!
//! # The one decision this module exists to implement
//!
//! Amendment 5 (a): **the key service wraps, and this process encrypts.** The master key
//! stays in the provider's hardware and this process never sees it; what it does hold is
//! an AES-256-GCM data key that the service wrapped, so `seal` and `unseal` are local and
//! the service is called **once at start and once per rotation and at no other time**.
//!
//! The alternative -- a call to the service per `seal` -- misses
//! docs/performance-budgets.md's journal budget by about three orders of magnitude: that
//! budget is 50 envelopes under 1 ms, which is 20 microseconds an envelope, against a
//! regional round trip measured in tens of milliseconds, and the node profile fsyncs
//! every envelope. It would also stop the system of record from writing whenever the
//! link was slow, which is the moment a record matters most.
//!
//! [`the_service_is_never_called_on_a_seal`] is that claim as a test rather than as
//! prose: it counts calls across a thousand seals and asserts the count did not move.
//!
//! # What reuses what
//!
//! `keystore.rs`'s [`PersistentKeyProvider`] already seals a key snapshot into one file
//! under a wrapping key argon2 derives from a string; amendment 3 got that string from a
//! typed passphrase and amendment 4 from the operating system's keystore. This is the
//! third source and it changes nothing else: 32 random bytes, hex-encoded, that the key
//! service wraps into [`WRAPPED_SECRET_FILE`] beside the keystore and unwraps at every
//! later start. One file format, one derivation, one set of tests.
//!
//! **A 32-byte secret is wrapped rather than the snapshot itself** because AWS KMS caps
//! `Encrypt` at 4096 bytes of plaintext and a snapshot grows with every rotation -- a
//! deployment would have hit that ceiling silently, years in, holding a keystore it could
//! no longer open.
//!
//! # Signing is the exception
//!
//! Amendment 5 (b): `sign` is not on a per-frame path -- once per TLS handshake, once per
//! baseline -- so the private half stays in the service and every call is a round trip,
//! which is the cost amendment 1 (a) already priced and accepted. [`ManagedServiceKeyProvider`]
//! therefore delegates `seal`/`unseal`/`rotate` to the local keystore and `sign` to the
//! service.
//!
//! # Why there is a trait in the middle
//!
//! [`CloudKeyService`] is the seam. No test in this workspace can reach a real AWS KMS or
//! Azure Key Vault, and pretending otherwise is the one thing this crate must not do, so
//! everything that can be tested without a cloud account is tested against a fake that
//! implements this trait -- the envelope logic, the unreachable path, the health status,
//! rotation, and the call counting above. The two SDK-backed implementations below are
//! compile-verified and carry `#[ignore]`d integration tests. **No cloud round trip has
//! been made and none is claimed.**

use std::path::Path;
use std::sync::Arc;

use crate::asymmetric::{EscrowPublicKey, EscrowedKey};
use crate::keys::{KeyId, KeyProvider, KeyPurpose, KeyState, SignatureScheme};
use crate::keystore::PersistentKeyProvider;
use crate::SecurityError;

/// The wrapped secret, beside `keystore.sealed` in the data directory. Fixed, for the
/// same reason [`crate::KEYSTORE_FILE`] is fixed: the baseline names a mechanism and
/// never a path (DN-22 §6).
pub const WRAPPED_SECRET_FILE: &str = "keystore.kms-wrapped";

/// What this profile needs from a cloud key service, and nothing more.
///
/// Three operations, because three are all the design uses. Deliberately **not** an
/// abstraction over "a KMS": a wider trait would invite a caller to reach for a
/// capability one cloud has and the other does not, and the point of the seam is that
/// the provider above it cannot tell which cloud is underneath.
///
/// **No method returns key material.** `wrap` and `unwrap_key` move a data key the
/// caller already generated, and `sign` does the work; the master key never crosses this
/// boundary in either direction, which is DN-22 §3's rule applied one level down.
pub trait CloudKeyService: Send + Sync + std::fmt::Debug {
    /// Wrap a locally generated data key under the service's master key.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the service cannot be reached or refuses.
    fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError>;

    /// Open what [`CloudKeyService::wrap`] produced.
    ///
    /// # Errors
    ///
    /// As above. A wrapped secret that does not open is **not** a first run: the caller
    /// must refuse rather than generate a replacement, because the keystore it protects
    /// may be the only way to read a year of journals.
    fn unwrap_key(&self, wrapped: &[u8]) -> Result<Vec<u8>, SecurityError>;

    /// Sign with a private half that never leaves the service (amendment 5 b).
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the service is unreachable, or
    /// `AuthenticationUnavailable` when it cannot produce this scheme.
    fn sign(&self, message: &[u8], scheme: SignatureScheme) -> Result<Vec<u8>, SecurityError>;

    /// What the health line calls this, without naming anything secret.
    fn describe(&self) -> String;
}

/// 32 random bytes, hex-encoded: the same shape of string
/// [`PersistentKeyProvider::open_or_create`] already takes, so it needs no second code
/// path. As in amendment 4, argon2's cost against a generated high-entropy secret buys
/// nothing beyond what it already buys against a human passphrase; it is fed through
/// unchanged for that reason and not because the cost is thought to help.
fn generate_secret() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut aes_gcm::aead::OsRng, &mut bytes);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The wrapping secret for `dir`'s keystore: unwrapped by the key service on every start,
/// or generated and wrapped on the first one.
///
/// Mirrors [`crate::os_keystore::wrapping_secret`] exactly, with the cloud service where
/// the operating system's keystore was.
///
/// # Errors
///
/// `KeyProviderUnavailable` when the service is unreachable, when the wrapped file cannot
/// be read or written, or when it does not open -- **which is never treated as a first
/// run**. Generating a replacement then would orphan the keystore the existing one
/// protects, the same failure `open_or_create` refuses for a file that will not decrypt.
fn wrapping_secret(dir: &Path, cloud: &dyn CloudKeyService) -> Result<String, SecurityError> {
    let path = dir.join(WRAPPED_SECRET_FILE);
    let unavailable = SecurityError::KeyProviderUnavailable;
    if path.is_file() {
        let wrapped = std::fs::read(&path)
            .map_err(|e| unavailable(format!("reading {}: {e}", path.display())))?;
        let secret = cloud.unwrap_key(&wrapped)?;
        return Ok(hex(&secret));
    }
    // **A keystore with no wrapped secret beside it is not a first run**, and generating
    // one here would be the same mistake as treating a file that will not open as an
    // empty one. The keystore is already unopenable at this point -- its secret existed
    // only in the file that is missing -- so writing a fresh secret cannot make the data
    // recoverable; what it does do is turn a diagnosable "the wrapped secret is gone"
    // into a misleading "did not open under this passphrase", sending whoever is holding
    // the incident at the wrong question. Refuse and name it instead.
    let keystore = dir.join(crate::KEYSTORE_FILE);
    if keystore.is_file() {
        return Err(unavailable(format!(
            "{} exists but {} does not: the wrapped secret that unlocks this keystore is \
             missing, and a new one would not open it. Restore the file or recover the \
             journal through the escrow record (DN-22 §11)",
            keystore.display(),
            path.display()
        )));
    }
    let secret = generate_secret();
    let wrapped = cloud.wrap(&secret)?;
    std::fs::create_dir_all(dir)
        .map_err(|e| unavailable(format!("creating {}: {e}", dir.display())))?;
    // Write beside, then rename: a crash mid-write must not leave half a wrapped secret,
    // which on the next start would read as a file that does not open and stop the node
    // from ever opening its own keystore. Same rule as `keystore.rs::persist`.
    let tmp = path.with_extension("kms-wrapped.tmp");
    std::fs::write(&tmp, &wrapped)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| unavailable(format!("writing {}: {e}", path.display())))?;
    Ok(hex(&secret))
}

/// A provider whose keystore is unlocked by a cloud key service and whose signing happens
/// there (DN-22 amendment 5).
///
/// Constructed by the binary, because custody belongs to the host (DN-22 §4).
pub struct ManagedServiceKeyProvider {
    inner: PersistentKeyProvider,
    cloud: Arc<dyn CloudKeyService>,
}

impl std::fmt::Debug for ManagedServiceKeyProvider {
    /// Never prints a key, and the service's own `Debug` must not either -- both
    /// implementations below print an endpoint and a key name, which are baseline
    /// values, and nothing else.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedServiceKeyProvider")
            .field("cloud", &self.cloud)
            .finish_non_exhaustive()
    }
}

impl ManagedServiceKeyProvider {
    /// Open the keystore in `dir` under a secret the key service holds, creating both on
    /// a first run.
    ///
    /// **This is the only network call on the start path**, and a failure here is DN-22
    /// §7's stated-unencrypted condition rather than a reason to refuse to start: the
    /// caller reports `EncryptionStatus::UnavailableWritingPlaintext` with what the
    /// service actually said and journals in the clear.
    ///
    /// # Errors
    ///
    /// `KeyProviderUnavailable` when the service is unreachable or refuses, or when the
    /// keystore or the wrapped secret cannot be read or written.
    pub fn open_or_create(
        dir: &Path,
        cloud: Arc<dyn CloudKeyService>,
        escrow: Option<EscrowPublicKey>,
    ) -> Result<Self, SecurityError> {
        let secret = wrapping_secret(dir, cloud.as_ref())?;
        let inner = PersistentKeyProvider::open_or_create(dir, &secret, escrow)?;
        Ok(Self { inner, cloud })
    }

    /// The active key for `purpose`, generating one on first use.
    ///
    /// # Errors
    ///
    /// As [`PersistentKeyProvider::active_or_generate`]. **No network call**: the key is
    /// minted locally and sealed into the keystore the service already unlocked.
    pub fn active_or_generate(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        self.inner.active_or_generate(purpose)
    }

    /// The journal key wrapped to the security officer (DN-22 §11).
    ///
    /// Works unchanged for this profile, and amendment 5 (f) turns on that: the data key
    /// is in process, so there is something to wrap. A design that called the service per
    /// envelope would have had nothing to escrow at all.
    ///
    /// # Errors
    ///
    /// As [`PersistentKeyProvider::escrow_wrap`].
    pub fn escrow_wrap(&self, id: &KeyId) -> Result<EscrowedKey, SecurityError> {
        self.inner.escrow_wrap(id)
    }

    #[must_use]
    pub fn escrow_configured(&self) -> bool {
        self.inner.escrow_configured()
    }

    /// What the health line calls this deployment's key service.
    #[must_use]
    pub fn describe(&self) -> String {
        self.cloud.describe()
    }
}

impl KeyProvider for ManagedServiceKeyProvider {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        self.inner.active(purpose)
    }

    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError> {
        self.inner.state(id)
    }

    /// Local, and deliberately so. See this module's header and amendment 5 (a).
    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.inner.seal(id, plaintext)
    }

    /// Local, which is also why an outage that begins after start does not stop the
    /// journal sealing (amendment 5 d).
    fn unseal(&self, id: &KeyId, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.inner.unseal(id, ciphertext)
    }

    /// Mints a new data key and retires the previous one, rewriting nothing.
    ///
    /// **The wrapped secret is not re-wrapped here**: it protects the keystore file, not
    /// any one key inside it, so a rotation of a data key does not need the service. The
    /// service is called on a rotation only when the deployment rotates the *master* key,
    /// which is the cloud account's own act and not this one (amendment 5 e).
    fn rotate(&mut self, purpose: KeyPurpose) -> Result<KeyId, SecurityError> {
        self.inner.rotate(purpose)
    }

    /// A round trip to the key service, every time (amendment 5 b).
    fn sign(
        &self,
        id: &KeyId,
        message: &[u8],
        scheme: SignatureScheme,
    ) -> Result<Vec<u8>, SecurityError> {
        // `id` names the purpose and version this deployment asked to sign with; the key
        // itself is the one the baseline's `key_id` points at in the service, so there is
        // nothing here to look up. Named rather than ignored so a reader does not think
        // it was forgotten.
        let _ = id;
        self.cloud.sign(message, scheme)
    }
}

// ---------------------------------------------------------------------------
// Bridging a synchronous trait to two asynchronous SDKs
// ---------------------------------------------------------------------------

/// One call to a cloud key service, sent to the worker thread.
enum Job {
    Wrap(Vec<u8>),
    Unwrap(Vec<u8>),
    Sign(Vec<u8>, SignatureScheme),
}

type Reply = std::sync::mpsc::Sender<Result<Vec<u8>, SecurityError>>;

/// A cloud client running on its own thread, with its own runtime.
///
/// **Why a thread and not `Runtime::block_on` in place.** [`KeyProvider`] is synchronous
/// -- `rustls` calls `sign` from inside a handshake, and `gungnir-store` calls `seal`
/// from the journal -- while both SDKs are asynchronous. Calling `Runtime::block_on`
/// from inside another Tokio runtime panics, and `block_in_place` panics on a
/// current-thread one, so neither is safe from a library that cannot know what its
/// caller is running on. A dedicated thread owning the runtime and a channel to it is
/// safe from anywhere: the caller blocks on a `recv`, which is just a blocking read.
struct Worker {
    calls: std::sync::mpsc::Sender<(Job, Reply)>,
    describe: String,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("service", &self.describe)
            .finish_non_exhaustive()
    }
}

impl Worker {
    /// Start the thread. `run` owns the runtime and the client and answers every call.
    fn spawn<F>(describe: String, run: F) -> Self
    where
        F: FnOnce(std::sync::mpsc::Receiver<(Job, Reply)>) + Send + 'static,
    {
        let (calls, jobs) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("gungnir-cloud-kms".into())
            .spawn(move || run(jobs))
            // A thread that will not start is a service that cannot be reached, which is
            // exactly what `dispatch` reports when the channel is dead. Not a panic:
            // DN-22 §7 says an unavailable keystore is a stated condition, not a crash.
            .map_or_else(
                |e| tracing::error!("the cloud key service thread did not start: {e}"),
                |_| (),
            );
        Self { calls, describe }
    }

    fn dispatch(&self, job: Job) -> Result<Vec<u8>, SecurityError> {
        let (reply, answers) = std::sync::mpsc::channel();
        self.calls.send((job, reply)).map_err(|_| {
            SecurityError::KeyProviderUnavailable(format!(
                "the {} client stopped answering",
                self.describe
            ))
        })?;
        answers.recv().map_err(|_| {
            SecurityError::KeyProviderUnavailable(format!(
                "the {} client dropped a call without answering",
                self.describe
            ))
        })?
    }
}

/// A runtime for one cloud client's thread. Current-thread: this worker serves one call
/// at a time by construction, and a multi-threaded pool for a start-time key unwrap and
/// an occasional signature would be pure overhead.
fn worker_runtime(service: &str) -> Result<tokio::runtime::Runtime, SecurityError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            SecurityError::KeyProviderUnavailable(format!(
                "no runtime for the {service} client: {e}"
            ))
        })
}

/// Answer every queued call with the reason the client could not be built, rather than
/// leaving callers blocked on a channel that will never reply.
fn refuse_every_call(jobs: &std::sync::mpsc::Receiver<(Job, Reply)>, why: &str) {
    // The reason travels as a string rather than as a `SecurityError` because that type
    // is deliberately not `Clone` -- widening a human-owned public error type to satisfy
    // one worker loop would be the wrong way round -- and every call gets its own.
    while let Ok((_, reply)) = jobs.recv() {
        let _ = reply.send(Err(SecurityError::KeyProviderUnavailable(why.to_string())));
    }
}

// ---------------------------------------------------------------------------
// AWS KMS
// ---------------------------------------------------------------------------

/// AWS KMS behind [`CloudKeyService`] (D-42; DN-22 amendment 5 §14g).
///
/// **No credential comes from the baseline.** `aws-config`'s default chain resolves the
/// deployment's own identity -- environment, web identity token (an EKS service account),
/// container credentials, or the EC2 instance metadata service -- which is what lets
/// DN-22 §6's "no secret in a baseline" rule actually hold for a cloud client.
#[derive(Debug)]
pub struct AwsKmsKeyService {
    worker: Worker,
}

impl AwsKmsKeyService {
    /// Connect to KMS in `region` for the key `key_id` names (an ARN or an alias).
    ///
    /// Returns immediately: the client is built on the worker thread and the first
    /// failure surfaces on the first call, which on the start path is the unwrap.
    #[must_use]
    pub fn new(region: &str, key_id: &str) -> Self {
        let (region, key_id) = (region.to_string(), key_id.to_string());
        let describe = format!("AWS KMS key {key_id} in {region}");
        let label = describe.clone();
        Self {
            worker: Worker::spawn(describe, move |jobs| {
                let runtime = match worker_runtime("AWS KMS") {
                    Ok(runtime) => runtime,
                    Err(why) => return refuse_every_call(&jobs, &why.to_string()),
                };
                let config = runtime.block_on(
                    aws_config::defaults(aws_config::BehaviorVersion::latest())
                        .region(aws_config::Region::new(region))
                        .load(),
                );
                let client = aws_sdk_kms::Client::new(&config);
                while let Ok((job, reply)) = jobs.recv() {
                    let answer = runtime.block_on(aws_call(&client, &key_id, job));
                    let _ = reply.send(answer.map_err(|e| {
                        SecurityError::KeyProviderUnavailable(format!("{label}: {e}"))
                    }));
                }
            }),
        }
    }
}

/// One KMS call. Split out so the worker loop stays readable and so the error mapping
/// happens in exactly one place.
async fn aws_call(client: &aws_sdk_kms::Client, key_id: &str, job: Job) -> Result<Vec<u8>, String> {
    use aws_sdk_kms::primitives::Blob;
    match job {
        Job::Wrap(plaintext) => client
            .encrypt()
            .key_id(key_id)
            .plaintext(Blob::new(plaintext))
            .send()
            .await
            .map_err(|e| format!("wrapping the keystore secret: {e}"))?
            .ciphertext_blob()
            .map(|b| b.as_ref().to_vec())
            .ok_or_else(|| "KMS returned no ciphertext for the keystore secret".to_string()),
        Job::Unwrap(wrapped) => client
            .decrypt()
            .key_id(key_id)
            .ciphertext_blob(Blob::new(wrapped))
            .send()
            .await
            .map_err(|e| format!("unwrapping the keystore secret: {e}"))?
            .plaintext()
            .map(|b| b.as_ref().to_vec())
            .ok_or_else(|| "KMS returned no plaintext for the keystore secret".to_string()),
        Job::Sign(message, scheme) => {
            let algorithm = aws_signing_algorithm(scheme)?;
            // **Hashed here and sent as a digest, not sent raw.** KMS caps a `Raw`
            // message at 4096 bytes; a TLS transcript or a configuration baseline can
            // exceed that, and a signature that works until a baseline grows is worse
            // than one that never worked. The digest is 32 bytes whatever the input.
            let digest = sha256(&message);
            client
                .sign()
                .key_id(key_id)
                .message(Blob::new(digest))
                .message_type(aws_sdk_kms::types::MessageType::Digest)
                .signing_algorithm(algorithm)
                .send()
                .await
                .map_err(|e| format!("signing: {e}"))?
                .signature()
                .map(|b| b.as_ref().to_vec())
                .ok_or_else(|| "KMS returned no signature".to_string())
        }
    }
}

fn aws_signing_algorithm(
    scheme: SignatureScheme,
) -> Result<aws_sdk_kms::types::SigningAlgorithmSpec, String> {
    match scheme {
        SignatureScheme::EcdsaP256Sha256 => {
            Ok(aws_sdk_kms::types::SigningAlgorithmSpec::EcdsaSha256)
        }
        SignatureScheme::RsaPssSha256 => {
            Ok(aws_sdk_kms::types::SigningAlgorithmSpec::RsassaPssSha256)
        }
        // Named rather than mapped to something close: KMS has no Ed25519 signing
        // algorithm at all, and returning a P-256 signature for an Ed25519 request would
        // be a wrong answer rather than a missing one.
        SignatureScheme::Ed25519 => Err(
            "AWS KMS does not sign Ed25519; the baseline asks for a scheme this \
                 service cannot produce"
                .to_string(),
        ),
    }
}

fn sha256(message: &[u8]) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(message).to_vec()
}

impl CloudKeyService for AwsKmsKeyService {
    fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Wrap(plaintext.to_vec()))
    }

    fn unwrap_key(&self, wrapped: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Unwrap(wrapped.to_vec()))
    }

    fn sign(&self, message: &[u8], scheme: SignatureScheme) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Sign(message.to_vec(), scheme))
    }

    fn describe(&self) -> String {
        self.worker.describe.clone()
    }
}

// ---------------------------------------------------------------------------
// Azure Key Vault
// ---------------------------------------------------------------------------

/// Azure Key Vault behind [`CloudKeyService`] (D-42; DN-22 amendment 5 §14g).
///
/// **`ManagedIdentityCredential`, not the `DefaultAzureCredential` that no longer
/// exists**: at 1.0 the Azure SDK split that chain in two, and a deployment wants the
/// managed-identity half. The developer half would authenticate from an engineer's own
/// signed-in Azure CLI session, which works on a workstation and fails on a node --
/// worse than not compiling.
#[derive(Debug)]
pub struct AzureKeyVaultKeyService {
    worker: Worker,
}

impl AzureKeyVaultKeyService {
    /// Connect to the vault at `vault_url` for the key `key_name` names.
    ///
    /// Returns immediately, as [`AwsKmsKeyService::new`] does and for the same reason.
    #[must_use]
    pub fn new(vault_url: &str, key_name: &str) -> Self {
        let (vault_url, key_name) = (vault_url.to_string(), key_name.to_string());
        let describe = format!("Azure Key Vault key {key_name} at {vault_url}");
        let label = describe.clone();
        Self {
            worker: Worker::spawn(describe, move |jobs| {
                let runtime = match worker_runtime("Azure Key Vault") {
                    Ok(runtime) => runtime,
                    Err(why) => return refuse_every_call(&jobs, &why.to_string()),
                };
                let client = match azure_identity::ManagedIdentityCredential::new(None)
                    .map_err(|e| format!("no managed identity for this host: {e}"))
                    .and_then(|credential| {
                        azure_security_keyvault_keys::KeyClient::new(&vault_url, credential, None)
                            .map_err(|e| format!("addressing the vault: {e}"))
                    }) {
                    Ok(client) => client,
                    Err(why) => return refuse_every_call(&jobs, &format!("{label}: {why}")),
                };
                while let Ok((job, reply)) = jobs.recv() {
                    let answer = runtime.block_on(azure_call(&client, &key_name, job));
                    let _ = reply.send(answer.map_err(|e| {
                        SecurityError::KeyProviderUnavailable(format!("{label}: {e}"))
                    }));
                }
            }),
        }
    }
}

/// One Key Vault call, mirroring [`aws_call`].
async fn azure_call(
    client: &azure_security_keyvault_keys::KeyClient,
    key_name: &str,
    job: Job,
) -> Result<Vec<u8>, String> {
    use azure_security_keyvault_keys::models::{
        EncryptionAlgorithm, KeyOperationParameters, SignParameters,
    };
    match job {
        Job::Wrap(plaintext) => {
            let parameters = KeyOperationParameters {
                algorithm: Some(EncryptionAlgorithm::RsaOaep256),
                value: Some(plaintext),
                ..Default::default()
            };
            client
                .wrap_key(
                    key_name,
                    parameters
                        .try_into()
                        .map_err(|e| format!("encoding the wrap request: {e}"))?,
                    None,
                )
                .await
                .map_err(|e| format!("wrapping the keystore secret: {e}"))?
                .into_model()
                .map_err(|e| format!("reading the wrap response: {e}"))?
                .result
                .ok_or_else(|| "the vault returned no wrapped secret".to_string())
        }
        Job::Unwrap(wrapped) => {
            let parameters = KeyOperationParameters {
                algorithm: Some(EncryptionAlgorithm::RsaOaep256),
                value: Some(wrapped),
                ..Default::default()
            };
            // The empty version selects the latest, which is what a deployment that has
            // never rotated its master key has. A deployment that rotates one must record
            // the version the secret was wrapped under (amendment 5 e); carrying it in
            // the wrapped file is the open item that leaves.
            client
                .unwrap_key(
                    key_name,
                    "",
                    parameters
                        .try_into()
                        .map_err(|e| format!("encoding the unwrap request: {e}"))?,
                    None,
                )
                .await
                .map_err(|e| format!("unwrapping the keystore secret: {e}"))?
                .into_model()
                .map_err(|e| format!("reading the unwrap response: {e}"))?
                .result
                .ok_or_else(|| "the vault returned no unwrapped secret".to_string())
        }
        Job::Sign(message, scheme) => {
            let parameters = SignParameters {
                algorithm: Some(azure_signing_algorithm(scheme)?),
                // Hashed here for the same reason as the AWS path: the vault's sign
                // operation takes a digest, not a message.
                value: Some(sha256(&message)),
            };
            let raw = client
                .sign(
                    key_name,
                    parameters
                        .try_into()
                        .map_err(|e| format!("encoding the sign request: {e}"))?,
                    None,
                )
                .await
                .map_err(|e| format!("signing: {e}"))?
                .into_model()
                .map_err(|e| format!("reading the sign response: {e}"))?
                .result
                .ok_or_else(|| "the vault returned no signature".to_string())?;
            azure_signature_to_provider_format(scheme, raw)
        }
    }
}

/// Key Vault's `sign` operation returns ES256 as the raw 64-byte `r || s` concatenation
/// (RFC 7518 §3.4), never the ASN.1 DER encoding this crate's `KeyProvider::sign`
/// contract uses everywhere else: `P256KeyProvider::sign` (`asymmetric.rs`) calls
/// `Signature::to_der()` before returning, AWS KMS's own `EcdsaSha256` algorithm returns
/// DER natively, and every consumer -- `gungnir_remote::identity`'s `rustls`/`rcgen`
/// bridge chief among them -- parses the result with `Signature::from_der`. Handed
/// straight through, a raw signature from this path would fail to parse or, worse,
/// parse as a different (wrong) signature under a decoder lenient enough to try, and
/// every TLS handshake signed by an `AzureKeyVaultKeyService`-backed identity would be
/// unusable -- invisible to every test here, since none of them reaches a real vault.
/// **Found and fixed 2026-09-10, reviewing this file for the owner's signature.**
/// PS256 (RSA) has no such split -- an RSA signature is one raw integer under either
/// convention -- so this only converts the one scheme that needs it.
fn azure_signature_to_provider_format(
    scheme: SignatureScheme,
    raw: Vec<u8>,
) -> Result<Vec<u8>, String> {
    match scheme {
        SignatureScheme::EcdsaP256Sha256 => {
            let signature = p256::ecdsa::Signature::from_slice(&raw).map_err(|e| {
                format!(
                    "Key Vault's ES256 signature ({} bytes) did not parse as the \
                     documented raw r||s format: {e}",
                    raw.len()
                )
            })?;
            Ok(signature.to_der().as_bytes().to_vec())
        }
        SignatureScheme::RsaPssSha256 | SignatureScheme::Ed25519 => Ok(raw),
    }
}

fn azure_signing_algorithm(
    scheme: SignatureScheme,
) -> Result<azure_security_keyvault_keys::models::SignatureAlgorithm, String> {
    use azure_security_keyvault_keys::models::SignatureAlgorithm;
    match scheme {
        SignatureScheme::EcdsaP256Sha256 => Ok(SignatureAlgorithm::Es256),
        SignatureScheme::RsaPssSha256 => Ok(SignatureAlgorithm::Ps256),
        // As on the AWS side, and for the same reason: named, not approximated.
        SignatureScheme::Ed25519 => Err("Azure Key Vault does not sign Ed25519; the \
                                         baseline asks for a scheme this service cannot \
                                         produce"
            .to_string()),
    }
}

impl CloudKeyService for AzureKeyVaultKeyService {
    fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Wrap(plaintext.to_vec()))
    }

    fn unwrap_key(&self, wrapped: &[u8]) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Unwrap(wrapped.to_vec()))
    }

    fn sign(&self, message: &[u8], scheme: SignatureScheme) -> Result<Vec<u8>, SecurityError> {
        self.worker.dispatch(Job::Sign(message.to_vec(), scheme))
    }

    fn describe(&self) -> String {
        self.worker.describe.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// A key service that wraps locally, counts what it was asked to do, and can be made
    /// unreachable.
    ///
    /// **Wrapping is real AES-256-GCM under a key held here**, not an identity function:
    /// a fake that returned its input would let a bug that never wrapped at all pass
    /// every test below.
    struct FakeCloud {
        key: [u8; 32],
        calls: AtomicUsize,
        reachable: AtomicBool,
    }

    // Written out rather than derived, and the reason is a real one this test caught:
    // `#[derive(Debug)]` on a service that holds a key prints the key. Both real
    // services avoid it by holding only a `Worker`, whose own `Debug` prints the service
    // description and nothing else; the fake has to earn the same property or
    // `neither_the_provider_nor_the_service_prints_anything_secret` tests nothing.
    impl std::fmt::Debug for FakeCloud {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FakeCloud")
                .field("service", &self.describe())
                .finish_non_exhaustive()
        }
    }

    impl FakeCloud {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                key: [7u8; 32],
                calls: AtomicUsize::new(0),
                reachable: AtomicBool::new(true),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn go_offline(&self) {
            self.reachable.store(false, Ordering::SeqCst);
        }

        fn check(&self) -> Result<Aes, SecurityError> {
            use aes_gcm::KeyInit;
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.reachable.load(Ordering::SeqCst) {
                return Err(SecurityError::KeyProviderUnavailable(
                    "the key service is unreachable: no route to host".into(),
                ));
            }
            Ok(Aes::new(aes_gcm::Key::<Aes>::from_slice(&self.key)))
        }
    }

    type Aes = aes_gcm::Aes256Gcm;

    impl CloudKeyService for FakeCloud {
        fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
            use aes_gcm::aead::Aead;
            use aes_gcm::AeadCore;
            let cipher = self.check()?;
            let nonce = Aes::generate_nonce(&mut aes_gcm::aead::OsRng);
            let mut out = nonce.to_vec();
            out.extend_from_slice(
                &cipher
                    .encrypt(&nonce, plaintext)
                    .map_err(|_| SecurityError::KeyProviderUnavailable("wrap failed".into()))?,
            );
            Ok(out)
        }

        fn unwrap_key(&self, wrapped: &[u8]) -> Result<Vec<u8>, SecurityError> {
            use aes_gcm::aead::Aead;
            let cipher = self.check()?;
            if wrapped.len() < 12 {
                return Err(SecurityError::KeyProviderUnavailable("too short".into()));
            }
            let (nonce, body) = wrapped.split_at(12);
            cipher
                .decrypt(aes_gcm::Nonce::from_slice(nonce), body)
                .map_err(|_| {
                    SecurityError::KeyProviderUnavailable("the wrapped secret did not open".into())
                })
        }

        fn sign(&self, message: &[u8], _scheme: SignatureScheme) -> Result<Vec<u8>, SecurityError> {
            self.check()?;
            Ok(sha256(message))
        }

        fn describe(&self) -> String {
            "a fake key service".into()
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("gungnir-managed-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test directory");
        path
    }

    /// **The claim amendment 5 (a) is built on, counted rather than asserted in prose.**
    /// If `seal` ever calls the service, the journal budget of 20 microseconds an
    /// envelope becomes a network round trip and this test is what fails.
    #[test]
    fn the_service_is_never_called_on_a_seal() {
        let path = dir("no-call-on-seal");
        let cloud = FakeCloud::new();
        let provider =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect("opened");
        let key = provider
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");

        let after_open = cloud.calls();
        assert_eq!(after_open, 1, "opening should cost exactly one wrap");

        for _ in 0..1000 {
            let sealed = provider.seal(&key, b"an envelope").expect("sealed");
            assert_eq!(
                provider.unseal(&key, &sealed).expect("opened"),
                b"an envelope"
            );
        }
        assert_eq!(
            cloud.calls(),
            after_open,
            "the key service was called during sealing; the journal budget cannot be met \
             with a network round trip per envelope (DN-22 amendment 5 a)"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// A restart opens the same keystore, which is the whole point of a persistent
    /// custody profile: the second run unwraps the secret the first one wrapped.
    #[test]
    fn a_restart_opens_the_same_keystore() {
        let path = dir("restart");
        let cloud = FakeCloud::new();
        let sealed = {
            let provider = ManagedServiceKeyProvider::open_or_create(
                &path,
                Arc::clone(&cloud) as Arc<_>,
                None,
            )
            .expect("first run");
            let key = provider
                .active_or_generate(KeyPurpose::JournalAtRest)
                .expect("key");
            provider
                .seal(&key, b"written before the restart")
                .expect("sealed")
        };

        let provider =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect("second run");
        let key = provider
            .active(KeyPurpose::JournalAtRest)
            .expect("same key");
        assert_eq!(
            provider.unseal(&key, &sealed).expect("still opens"),
            b"written before the restart"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// DN-22 §7: an unavailable keystore yields a **stated** unencrypted condition, never
    /// a claimed-but-absent encryption. Here that means the constructor fails and carries
    /// what the service actually said, so the caller can put it in health verbatim.
    #[test]
    fn a_service_unreachable_at_start_refuses_and_says_why() {
        let path = dir("unreachable-at-start");
        let cloud = FakeCloud::new();
        cloud.go_offline();
        let err =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect_err("must not open");
        assert!(
            err.to_string().contains("unreachable"),
            "the reason the service gave must reach the operator: {err}"
        );
        assert!(
            !path.join(WRAPPED_SECRET_FILE).exists(),
            "a wrapped secret was written for a wrap that never happened"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// Amendment 5 (d), and the availability property (a)'s choice buys: an outage that
    /// begins **after** start does not stop the journal sealing, because the data key is
    /// already here. A design calling the service per envelope would have stopped
    /// journalling at exactly the moment a record matters most.
    #[test]
    fn sealing_survives_an_outage_that_begins_after_start_and_signing_does_not() {
        let path = dir("outage-after-start");
        let cloud = FakeCloud::new();
        let provider =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect("opened");
        let key = provider
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");

        cloud.go_offline();

        let sealed = provider
            .seal(&key, b"written during the outage")
            .expect("sealing must survive an outage");
        assert_eq!(
            provider.unseal(&key, &sealed).expect("opened"),
            b"written during the outage"
        );

        let err = provider
            .sign(
                &key,
                b"a handshake transcript",
                SignatureScheme::EcdsaP256Sha256,
            )
            .expect_err("signing must not survive it");
        assert!(err.to_string().contains("unreachable"), "{err}");
        let _ = std::fs::remove_dir_all(path);
    }

    /// A wrapped secret that does not open is **not** a first run. Generating a
    /// replacement would orphan the keystore it protects -- the same failure
    /// `open_or_create` refuses for a keystore file that will not decrypt.
    #[test]
    fn a_wrapped_secret_that_does_not_open_is_refused_and_never_replaced() {
        let path = dir("corrupt-wrapped-secret");
        let cloud = FakeCloud::new();
        ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
            .expect("first run");
        let wrapped_path = path.join(WRAPPED_SECRET_FILE);
        let before = std::fs::read(&wrapped_path).expect("wrapped secret");

        let mut corrupted = before.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0x01;
        std::fs::write(&wrapped_path, &corrupted).expect("corrupt it");

        let err =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect_err("must refuse");
        assert!(err.to_string().contains("did not open"), "{err}");
        assert_eq!(
            std::fs::read(&wrapped_path).expect("still there"),
            corrupted,
            "the wrapped secret was overwritten with a fresh one"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// A keystore with no wrapped secret beside it is **not** a first run either, and the
    /// distinction is about diagnosis rather than recovery: the data is already
    /// unrecoverable once that file is gone, but generating a fresh secret would turn a
    /// nameable "the wrapped secret is missing" into a misleading "did not open under
    /// this passphrase" and send whoever is holding the incident at the wrong question.
    #[test]
    fn a_keystore_with_no_wrapped_secret_beside_it_is_named_rather_than_re_created() {
        let path = dir("missing-wrapped-secret");
        let cloud = FakeCloud::new();
        ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
            .expect("first run");
        std::fs::remove_file(path.join(WRAPPED_SECRET_FILE)).expect("remove the wrapped secret");
        let before = cloud.calls();

        let err =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect_err("must refuse");
        let reason = err.to_string();
        assert!(reason.contains("wrapped secret"), "{reason}");
        assert!(
            reason.contains("escrow"),
            "the operator must be pointed at the one remaining route: {reason}"
        );
        assert_eq!(
            cloud.calls(),
            before,
            "the service was asked to wrap a replacement secret that could not have helped"
        );
        assert!(
            !path.join(WRAPPED_SECRET_FILE).exists(),
            "a replacement wrapped secret was written"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// Rotation is local and leaves earlier material readable, exactly as it does for
    /// every other profile: the sealed bytes carry the version that protected them
    /// (amendment 1 b), so nothing is rewritten and the service is not involved.
    #[test]
    fn rotation_is_local_and_leaves_earlier_material_readable() {
        let path = dir("rotation");
        let cloud = FakeCloud::new();
        let mut provider =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect("opened");
        let first = provider
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");
        let sealed = provider
            .seal(&first, b"before the rotation")
            .expect("sealed");
        let before_rotation = cloud.calls();

        let second = provider.rotate(KeyPurpose::JournalAtRest).expect("rotated");
        assert_ne!(second.version, first.version);
        assert_eq!(provider.state(&first).expect("known"), KeyState::Retired);
        assert_eq!(
            cloud.calls(),
            before_rotation,
            "rotating a data key must not call the key service"
        );
        assert_eq!(
            provider.unseal(&second, &sealed).expect("still opens"),
            b"before the rotation"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// Amendment 5 (f): escrow works unchanged for the journal key, and that is a direct
    /// consequence of (a) -- the data key is in process, so there is something to wrap.
    #[test]
    fn the_journal_key_can_still_be_escrowed_to_the_officer() {
        use p256::ecdsa::SigningKey;
        let path = dir("escrow");
        let cloud = FakeCloud::new();
        let officer = SigningKey::random(&mut aes_gcm::aead::OsRng);
        let pem = {
            use p256::pkcs8::EncodePublicKey;
            officer
                .verifying_key()
                .to_public_key_pem(p256::pkcs8::LineEnding::LF)
                .expect("pem")
        };
        let escrow = EscrowPublicKey::from_pem(&pem).expect("officer key");

        let provider = ManagedServiceKeyProvider::open_or_create(
            &path,
            Arc::clone(&cloud) as Arc<_>,
            Some(escrow),
        )
        .expect("opened");
        assert!(provider.escrow_configured());
        let key = provider
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");
        assert!(
            provider.escrow_wrap(&key).is_ok(),
            "the journal key must still be recoverable without this node"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    /// A provider must never print a key, and the service beneath it must never print a
    /// credential. Both `Debug` implementations are checked here because a `Debug` that
    /// leaked one would undo the module.
    #[test]
    fn neither_the_provider_nor_the_service_prints_anything_secret() {
        let path = dir("debug");
        let cloud = FakeCloud::new();
        let provider =
            ManagedServiceKeyProvider::open_or_create(&path, Arc::clone(&cloud) as Arc<_>, None)
                .expect("opened");
        let printed = format!("{provider:?}");
        assert!(printed.contains("fake key service"), "{printed}");
        // The fake's key is 32 bytes of 0x07; a derived `Debug` would render it as
        // `[7, 7, 7, ...]`, which is exactly the leak this test exists to catch and
        // exactly what it caught the first time it ran.
        assert!(
            !printed.contains("7, 7, 7"),
            "the key reached a Debug: {printed}"
        );
        assert!(!printed.contains("07070707"), "{printed}");
        let _ = std::fs::remove_dir_all(path);
    }

    /// Neither service signs Ed25519, and both say so by name rather than returning a
    /// P-256 signature a caller might take for one. Checked without a network because it
    /// is pure mapping.
    #[test]
    fn a_scheme_neither_service_can_produce_is_named_rather_than_approximated() {
        let aws = aws_signing_algorithm(SignatureScheme::Ed25519).expect_err("refused");
        assert!(aws.contains("Ed25519"), "{aws}");
        let azure = azure_signing_algorithm(SignatureScheme::Ed25519).expect_err("refused");
        assert!(azure.contains("Ed25519"), "{azure}");
        assert!(aws_signing_algorithm(SignatureScheme::EcdsaP256Sha256).is_ok());
        assert!(azure_signing_algorithm(SignatureScheme::EcdsaP256Sha256).is_ok());
    }

    /// **Found and fixed reviewing this file for the owner's 2026-09-10 signature.** Key
    /// Vault's `sign` returns ES256 as the raw 64-byte `r || s` concatenation (RFC 7518
    /// §3.4); every consumer of this crate's `KeyProvider::sign` -- `P256KeyProvider`
    /// itself, and `gungnir_remote::identity`'s TLS bridge -- expects the ASN.1 DER
    /// encoding `AwsKmsKeyService`'s path already gets natively from KMS. Nothing here
    /// could reach a real vault to catch the mismatch, so this constructs a real P-256
    /// signature exactly the length and shape Key Vault's own documentation describes
    /// and checks the conversion round-trips to something `Signature::from_der` (the
    /// form every real caller uses) accepts and that verifies under the signing key.
    #[test]
    fn azure_s_raw_ecdsa_signature_is_converted_to_the_der_every_caller_expects() {
        use p256::ecdsa::signature::{Signer, Verifier};
        use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

        let signing_key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
        let verifying_key = VerifyingKey::from(&signing_key);
        let signature: Signature = signing_key.sign(b"a TLS transcript");
        // Key Vault's own wire format: the fixed-size raw `r || s` bytes, not DER.
        let raw = signature.to_bytes().to_vec();
        assert_eq!(
            raw.len(),
            64,
            "P-256's raw ECDSA signature is exactly 64 bytes"
        );

        let der = azure_signature_to_provider_format(SignatureScheme::EcdsaP256Sha256, raw)
            .expect("a well-formed raw signature converts");
        assert_ne!(
            der,
            signature.to_bytes().to_vec(),
            "the output must not still be the raw form"
        );
        let parsed = Signature::from_der(&der).expect(
            "every real caller (P256KeyProvider, gungnir_remote::identity) parses with \
             from_der; a raw passthrough would fail exactly here",
        );
        verifying_key
            .verify(b"a TLS transcript", &parsed)
            .expect("the converted signature must still verify under the signing key");
    }
}
