use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;
use zip::ZipArchive;

#[derive(Parser)]
#[command(name = "patchforge")]
#[command(about = "PatchForge OTA patch tooling")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Clap owns this enum only during command parsing, so the larger Publish
// variant does not remain in long-lived application state.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Commands {
    /// Print the SHA-256 hash used in server token configuration.
    TokenHash {
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: String,
    },
    Diff {
        #[arg(long)]
        old: PathBuf,
        #[arg(long)]
        new: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Verify {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        sha256: String,
    },
    Apply {
        #[arg(long)]
        old: PathBuf,
        #[arg(long)]
        patch: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    /// Create, verify, publish, and manifest-check a delta patch.
    Publish {
        /// PatchForge server base URL.
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server_url: String,
        /// Publisher bearer token; PATCHFORGE_TOKEN is recommended.
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: String,
        #[arg(long)]
        app_id: String,
        #[arg(long)]
        release_id: String,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "android")]
        platform: String,
        /// Release channel: development, staging, beta, or stable.
        #[arg(long, default_value = "stable")]
        channel: String,
        /// Percentage of deterministic device buckets eligible for this patch.
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(i32).range(0..=100))]
        rollout_percentage: i32,
        /// Signed manifest protocol version. Use 1 only for compatibility bridges.
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(i32).range(1..=2))]
        manifest_version: i32,
        /// Base APK or libapp.so.
        #[arg(long)]
        old: PathBuf,
        /// Target APK or libapp.so.
        #[arg(long)]
        new: PathBuf,
        #[arg(long, default_value = "arm64-v8a")]
        architecture: String,
        /// Server-local artifact storage root served at /artifacts.
        #[arg(long, default_value = "storage")]
        storage_dir: PathBuf,
        /// SSH destination used to upload artifacts to a remote server host.
        #[arg(long, requires = "remote_storage_dir")]
        ssh_target: Option<String>,
        /// SSH port used with --ssh-target.
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,
        /// Absolute artifact storage root on the remote server host.
        #[arg(long, requires = "ssh_target")]
        remote_storage_dir: Option<PathBuf>,
        /// Manifest path relative to the server's /artifacts route.
        #[arg(long, default_value = "patches")]
        artifact_prefix: String,
        /// Override the automatically selected next patch number.
        #[arg(long)]
        patch_number: Option<i32>,
        /// Also retain a local copy of the generated patch.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Deserialize)]
struct PatchRecord {
    id: String,
    patch_number: i32,
    manifest_version: i32,
    rollout_percentage: i32,
    file_path: String,
    sha256: String,
    file_size: i64,
    patch_format: Option<String>,
    architecture: Option<String>,
    base_sha256: Option<String>,
    base_patch_number: Option<i32>,
    target_sha256: Option<String>,
    target_file_size: Option<i64>,
}

#[derive(Serialize)]
struct CreatePatchRequest<'a> {
    patch_number: i32,
    manifest_version: i32,
    rollout_percentage: i32,
    file_path: &'a str,
    sha256: &'a str,
    file_size: i64,
    patch_format: &'static str,
    architecture: &'a str,
    base_sha256: &'a str,
    base_patch_number: i32,
    target_sha256: &'a str,
    target_file_size: i64,
}

#[derive(Debug, Deserialize)]
struct ReleaseArtifact {
    architecture: String,
    sha256: String,
    file_size: i64,
}

#[derive(Serialize)]
struct CreateReleaseArtifactRequest<'a> {
    architecture: &'a str,
    sha256: &'a str,
    file_size: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateResponse {
    update_available: bool,
    manifest: Option<PatchManifest>,
}

#[derive(Debug, Deserialize)]
struct PatchManifest {
    manifest_version: i32,
    app_id: String,
    release_id: String,
    version: String,
    platform: String,
    channel: String,
    patch_number: i32,
    artifact_path: String,
    sha256: String,
    file_size: i64,
    patch_format: Option<String>,
    architecture: Option<String>,
    base_sha256: Option<String>,
    base_patch_number: Option<i32>,
    target_sha256: Option<String>,
    target_file_size: Option<i64>,
    rollout_percentage: i32,
    signature: String,
}

struct PublishOptions<'a> {
    server_url: &'a str,
    token: &'a str,
    app_id: &'a str,
    release_id: &'a str,
    version: &'a str,
    platform: &'a str,
    channel: &'a str,
    rollout_percentage: i32,
    manifest_version: i32,
    old: &'a Path,
    new: &'a Path,
    architecture: &'a str,
    storage_dir: &'a Path,
    ssh_target: Option<&'a str>,
    ssh_port: u16,
    remote_storage_dir: Option<&'a Path>,
    artifact_prefix: &'a str,
    patch_number: Option<i32>,
    output: Option<&'a Path>,
}

fn sha256_reader(mut reader: impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("Failed to read {}", path.display()))?;
    sha256_reader(BufReader::new(file))
}

fn create_diff(old: &Path, new: &Path, output: &Path) -> Result<()> {
    if !old.is_file() {
        bail!("Old file does not exist: {}", old.display());
    }
    if !new.is_file() {
        bail!("New file does not exist: {}", new.display());
    }
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    println!("Old file: {}", old.display());
    println!("New file: {}", new.display());
    let old_hash = sha256_file(old)?;
    let new_hash = sha256_file(new)?;
    println!("Old SHA-256: {old_hash}");
    println!("New SHA-256: {new_hash}");

    let status = Command::new("bsdiff")
        .arg(old)
        .arg(new)
        .arg(output)
        .status()
        .context("Failed to run bsdiff. Is it installed?")?;
    if !status.success() {
        bail!("bsdiff failed");
    }

    let patch_size = fs::metadata(output)?.len();
    let new_size = fs::metadata(new)?.len();
    println!();
    println!("Patch created successfully.");
    println!("Patch: {}", output.display());
    println!("Patch size: {patch_size} bytes");
    println!("Target size: {new_size} bytes");
    Ok(())
}

fn verify_file(file: &Path, expected: &str) -> Result<()> {
    let actual = sha256_file(file)?;
    println!("Expected: {expected}");
    println!("Actual:   {actual}");
    if actual != expected.to_ascii_lowercase() {
        bail!("SHA-256 verification failed");
    }
    println!("SHA-256 verification successful.");
    Ok(())
}

fn apply_patch(
    old: &Path,
    patch: &Path,
    output: &Path,
    expected_sha256: Option<&str>,
) -> Result<()> {
    if !old.is_file() {
        bail!("Old file does not exist: {}", old.display());
    }
    if !patch.is_file() {
        bail!("Patch file does not exist: {}", patch.display());
    }
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    println!("Base file: {}", old.display());
    println!("Patch: {}", patch.display());
    println!("Output: {}", output.display());
    let status = Command::new("bspatch")
        .arg(old)
        .arg(output)
        .arg(patch)
        .status()
        .context("Failed to run bspatch. Is bsdiff installed?")?;
    if !status.success() {
        bail!("bspatch failed");
    }

    let output_hash = sha256_file(output)?;
    println!();
    println!("Patch applied successfully.");
    println!("Output SHA-256: {output_hash}");
    if let Some(expected) = expected_sha256 {
        if output_hash != expected.to_ascii_lowercase() {
            bail!(
                "Target SHA-256 verification failed.\nExpected: {}\nActual:   {}",
                expected,
                output_hash
            );
        }
        println!("Target SHA-256 verification successful.");
    }
    Ok(())
}

fn is_apk(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
}

fn extract_libapp(apk: &Path, architecture: &str, output: &Path) -> Result<()> {
    let file = File::open(apk).with_context(|| format!("Failed to open APK {}", apk.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read APK archive {}", apk.display()))?;
    let entry_name = format!("lib/{architecture}/libapp.so");
    let mut entry = archive
        .by_name(&entry_name)
        .with_context(|| format!("APK {} does not contain {}", apk.display(), entry_name))?;
    let mut destination = File::create(output)?;
    io::copy(&mut entry, &mut destination)?;
    destination.flush()?;
    Ok(())
}

fn resolve_artifact(input: &Path, architecture: &str, extracted_path: &Path) -> Result<PathBuf> {
    if !input.is_file() {
        bail!("Artifact does not exist: {}", input.display());
    }
    if is_apk(input) {
        extract_libapp(input, architecture, extracted_path)?;
        println!(
            "Extracted lib/{architecture}/libapp.so from {}",
            input.display()
        );
        Ok(extracted_path.to_path_buf())
    } else {
        Ok(input.to_path_buf())
    }
}

fn checked_response(response: Response, action: &str) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().unwrap_or_default();
    bail!("{action} failed with HTTP {status}: {body}")
}

fn api_url(server_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        server_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn artifact_path(prefix: &str, filename: &str) -> String {
    format!(
        "{}/{}",
        prefix.trim_matches('/'),
        filename.trim_start_matches('/')
    )
}

fn is_supported_android_architecture(value: &str) -> bool {
    matches!(value, "arm64-v8a" | "armeabi-v7a" | "x86_64")
}

fn is_supported_channel(value: &str) -> bool {
    matches!(value, "development" | "staging" | "beta" | "stable")
}

fn validation_device_id(app_id: &str, patch_id: &str, rollout_percentage: i32) -> Option<String> {
    if rollout_percentage == 0 {
        return None;
    }
    if rollout_percentage == 100 {
        return Some("patchforge-cli-validation".to_string());
    }
    (0..100_000).find_map(|number| {
        let candidate = format!("patchforge-cli-validation-{number}");
        let digest = Sha256::digest(format!("{app_id}:{patch_id}:{candidate}"));
        let bucket = u16::from_be_bytes([digest[0], digest[1]]) as u32 * 100 / 65_536;
        (bucket < rollout_percentage as u32).then_some(candidate)
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_patch_record(
    patch: &PatchRecord,
    patch_number: i32,
    public_path: &str,
    patch_hash: &str,
    patch_size: i64,
    architecture: &str,
    base_hash: &str,
    base_patch_number: i32,
    target_hash: &str,
    target_size: i64,
    manifest_version: i32,
    rollout_percentage: i32,
) -> Result<()> {
    if patch.patch_number != patch_number
        || patch.manifest_version != manifest_version
        || patch.rollout_percentage != rollout_percentage
        || patch.file_path != public_path
        || patch.sha256 != patch_hash
        || patch.file_size != patch_size
        || patch.patch_format.as_deref() != Some("bsdiff")
        || patch.architecture.as_deref() != Some(architecture)
        || patch.base_sha256.as_deref() != Some(base_hash)
        || patch.base_patch_number != Some(base_patch_number)
        || patch.target_sha256.as_deref() != Some(target_hash)
        || patch.target_file_size != Some(target_size)
    {
        bail!("Server patch record does not match the generated delta metadata: {patch:#?}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_manifest(
    manifest: &PatchManifest,
    options: &PublishOptions<'_>,
    patch_number: i32,
    public_path: &str,
    patch_hash: &str,
    patch_size: i64,
    base_hash: &str,
    base_patch_number: i32,
    target_hash: &str,
    target_size: i64,
) -> Result<()> {
    if manifest.app_id != options.app_id
        || manifest.release_id != options.release_id
        || manifest.version != options.version
        || manifest.platform != options.platform
        || manifest.channel != options.channel
        || manifest.manifest_version != options.manifest_version
        || manifest.rollout_percentage != options.rollout_percentage
        || manifest.patch_number != patch_number
        || manifest.artifact_path != public_path
        || manifest.sha256 != patch_hash
        || manifest.file_size != patch_size
        || manifest.patch_format.as_deref() != Some("bsdiff")
        || manifest.architecture.as_deref() != Some(options.architecture)
        || manifest.base_sha256.as_deref() != Some(base_hash)
        || manifest.base_patch_number != Some(base_patch_number)
        || manifest.target_sha256.as_deref() != Some(target_hash)
        || manifest.target_file_size != Some(target_size)
        || manifest.signature.is_empty()
    {
        bail!("Update manifest does not match the published patch: {manifest:#?}");
    }
    Ok(())
}

fn copy_new(source: &Path, destination: &Path) -> Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .with_context(|| {
            format!(
                "Refusing to overwrite existing artifact {}",
                destination.display()
            )
        })?;
    io::copy(&mut input, &mut output)?;
    output.flush()?;
    Ok(())
}

fn validate_ssh_target(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._@:-".contains(&byte))
    {
        bail!("SSH target contains unsupported characters")
    }
    Ok(())
}

fn validate_remote_storage_dir(path: &Path) -> Result<&str> {
    let value = path
        .to_str()
        .context("Remote storage path must be valid UTF-8")?;
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
    {
        bail!("Remote storage path must be an absolute path without spaces or '..'")
    }
    Ok(value)
}

fn upload_remote_artifact(
    source: &Path,
    ssh_target: &str,
    ssh_port: u16,
    remote_storage_dir: &Path,
    filename: &str,
) -> Result<()> {
    validate_ssh_target(ssh_target)?;
    let storage = validate_remote_storage_dir(remote_storage_dir)?;
    let patch_directory = format!("{}/patches", storage.trim_end_matches('/'));
    let destination = format!("{ssh_target}:{patch_directory}/{filename}");
    let port = ssh_port.to_string();

    let mkdir_status = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-p", &port, ssh_target])
        .args(["mkdir", "-p", "--", &patch_directory])
        .status()
        .context("Failed to run ssh. Is OpenSSH installed?")?;
    if !mkdir_status.success() {
        bail!("Failed to create the remote patch storage directory")
    }

    let upload_status = Command::new("scp")
        .args(["-q", "-P", &port])
        .arg(source)
        .arg(&destination)
        .status()
        .context("Failed to run scp. Is OpenSSH installed?")?;
    if !upload_status.success() {
        bail!("Failed to upload the patch artifact")
    }

    println!("Uploaded artifact: {destination}");
    Ok(())
}

fn publish(options: PublishOptions<'_>) -> Result<()> {
    if !is_supported_android_architecture(options.architecture) {
        bail!(
            "Unsupported architecture '{}'. Expected arm64-v8a, armeabi-v7a, or x86_64",
            options.architecture
        );
    }
    if !is_supported_channel(options.channel) {
        bail!(
            "Unsupported channel '{}'. Expected development, staging, beta, or stable",
            options.channel
        );
    }

    let client = Client::builder().timeout(Duration::from_secs(60)).build()?;
    let patches_url = api_url(
        options.server_url,
        &format!("api/v1/releases/{}/patches", options.release_id),
    );
    println!("Reading current release patch list...");
    let existing: Vec<PatchRecord> = checked_response(
        client.get(&patches_url).bearer_auth(options.token).send()?,
        "Listing release patches",
    )?
    .json()?;
    let release_artifacts_url = api_url(
        options.server_url,
        &format!("api/v1/releases/{}/artifacts", options.release_id),
    );
    let release_artifacts: Vec<ReleaseArtifact> = checked_response(
        client
            .get(&release_artifacts_url)
            .bearer_auth(options.token)
            .send()?,
        "Listing release baseline artifacts",
    )?
    .json()?;

    let temp = TempDir::new()?;
    let base_path = resolve_artifact(
        options.old,
        options.architecture,
        &temp.path().join("base-libapp.so"),
    )?;
    let target_path = resolve_artifact(
        options.new,
        options.architecture,
        &temp.path().join("target-libapp.so"),
    )?;
    let patch_path = temp.path().join("delta.patch");
    let reconstructed_path = temp.path().join("reconstructed-libapp.so");
    let base_hash = sha256_file(&base_path)?;
    let target_hash = sha256_file(&target_path)?;
    let base_size = i64::try_from(fs::metadata(&base_path)?.len())?;
    let target_size = i64::try_from(fs::metadata(&target_path)?.len())?;

    let architecture_patches: Vec<&PatchRecord> = existing
        .iter()
        .filter(|patch| patch.architecture.as_deref() == Some(options.architecture))
        .collect();
    let baseline = release_artifacts
        .iter()
        .find(|artifact| artifact.architecture == options.architecture);

    let baseline_hash = if let Some(baseline) = baseline {
        if baseline.sha256 == base_hash && baseline.file_size != base_size {
            bail!(
                "Baseline size mismatch for {}: expected {}, received {}",
                options.architecture,
                baseline.file_size,
                base_size
            );
        }
        baseline.sha256.clone()
    } else {
        if !architecture_patches.is_empty() {
            bail!(
                "Release has {} patch records but no registered baseline artifact",
                options.architecture
            );
        }

        let baseline_request = CreateReleaseArtifactRequest {
            architecture: options.architecture,
            sha256: &base_hash,
            file_size: base_size,
        };
        let created: ReleaseArtifact = checked_response(
            client
                .post(&release_artifacts_url)
                .bearer_auth(options.token)
                .json(&baseline_request)
                .send()?,
            "Registering release baseline artifact",
        )?
        .json()?;
        println!(
            "Registered {} baseline: {}",
            created.architecture, created.sha256
        );
        created.sha256
    };

    let base_patch_number = if base_hash == baseline_hash {
        0
    } else {
        architecture_patches
            .iter()
            .filter(|patch| patch.target_sha256.as_deref() == Some(base_hash.as_str()))
            .map(|patch| patch.patch_number)
            .max()
            .with_context(|| {
                format!(
                    "The supplied {} base does not match the registered release baseline or any published patch target",
                    options.architecture
                )
            })?
    };

    let latest_number = architecture_patches
        .iter()
        .map(|patch| patch.patch_number)
        .max()
        .unwrap_or(0);
    let patch_number = match options.patch_number {
        Some(number) if number > base_patch_number => number,
        Some(number) => {
            bail!("Patch number {number} must be greater than base patch {base_patch_number}")
        }
        None => latest_number
            .max(base_patch_number)
            .checked_add(1)
            .context("Patch number overflow")?,
    };
    if architecture_patches
        .iter()
        .any(|patch| patch.patch_number == patch_number)
    {
        bail!(
            "Patch {patch_number} already exists for architecture {}",
            options.architecture
        );
    }
    println!(
        "Selected {} chain: base patch {} -> patch {}",
        options.architecture, base_patch_number, patch_number
    );

    create_diff(&base_path, &target_path, &patch_path)?;
    apply_patch(
        &base_path,
        &patch_path,
        &reconstructed_path,
        Some(&target_hash),
    )?;
    let reconstructed_size = i64::try_from(fs::metadata(&reconstructed_path)?.len())?;
    if reconstructed_size != target_size {
        bail!(
            "Reconstructed target size mismatch: expected {target_size}, got {reconstructed_size}"
        );
    }

    let patch_hash = sha256_file(&patch_path)?;
    let patch_size = i64::try_from(fs::metadata(&patch_path)?.len())?;
    if let Some(output) = options.output {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        copy_new(&patch_path, output)
            .with_context(|| format!("Failed to retain patch at {}", output.display()))?;
        println!("Retained patch: {}", output.display());
    }

    let filename = format!(
        "{}-{}-patch-{}-{}.patch",
        options.release_id,
        options.architecture,
        patch_number,
        &patch_hash[..12]
    );
    let public_path = artifact_path(options.artifact_prefix, &filename);
    match (options.ssh_target, options.remote_storage_dir) {
        (Some(ssh_target), Some(remote_storage_dir)) => upload_remote_artifact(
            &patch_path,
            ssh_target,
            options.ssh_port,
            remote_storage_dir,
            &filename,
        )?,
        (None, None) => {
            let patch_storage_dir = options.storage_dir.join("patches");
            fs::create_dir_all(&patch_storage_dir).with_context(|| {
                format!(
                    "Failed to create artifact directory {}",
                    patch_storage_dir.display()
                )
            })?;
            let stored_path = patch_storage_dir.join(&filename);
            copy_new(&patch_path, &stored_path)?;
            println!("Stored artifact: {}", stored_path.display());
        }
        _ => bail!("--ssh-target and --remote-storage-dir must be used together"),
    }
    println!("Public artifact path: {public_path}");

    let request = CreatePatchRequest {
        patch_number,
        manifest_version: options.manifest_version,
        rollout_percentage: options.rollout_percentage,
        file_path: &public_path,
        sha256: &patch_hash,
        file_size: patch_size,
        patch_format: "bsdiff",
        architecture: options.architecture,
        base_sha256: &base_hash,
        base_patch_number,
        target_sha256: &target_hash,
        target_file_size: target_size,
    };
    let created: PatchRecord = checked_response(
        client
            .post(&patches_url)
            .bearer_auth(options.token)
            .json(&request)
            .send()?,
        "Creating patch record",
    )?
    .json()?;
    validate_patch_record(
        &created,
        patch_number,
        &public_path,
        &patch_hash,
        patch_size,
        options.architecture,
        &base_hash,
        base_patch_number,
        &target_hash,
        target_size,
        options.manifest_version,
        options.rollout_percentage,
    )?;

    let updates_url = api_url(
        options.server_url,
        &format!("api/v1/apps/{}/updates", options.app_id),
    );
    let current_patch_value = base_patch_number.to_string();
    let validation_device =
        validation_device_id(options.app_id, &created.id, options.rollout_percentage);
    let mut update_request = client.get(&updates_url).query(&[
        ("version", options.version),
        ("platform", options.platform),
        ("architecture", options.architecture),
        ("channel", options.channel),
        ("patch", current_patch_value.as_str()),
    ]);
    if let Some(device_id) = validation_device.as_deref() {
        update_request = update_request.query(&[("device_id", device_id)]);
    }
    let update: UpdateResponse =
        checked_response(update_request.send()?, "Checking update manifest")?.json()?;
    if options.rollout_percentage == 0 {
        if update.update_available {
            bail!("A 0% rollout unexpectedly returned an update");
        }
        println!("Verified 0% rollout suppression.");
    } else if !update.update_available {
        bail!("Manifest reported no update after publishing patch {patch_number}");
    } else {
        let manifest = update.manifest.context("Manifest body is missing")?;
        validate_manifest(
            &manifest,
            &options,
            patch_number,
            &public_path,
            &patch_hash,
            patch_size,
            &base_hash,
            base_patch_number,
            &target_hash,
            target_size,
        )?;
    }

    let artifact_download_path = format!("artifacts/{public_path}");
    let downloaded = checked_response(
        client
            .get(api_url(options.server_url, &artifact_download_path))
            .send()?,
        "Downloading the published artifact",
    )?
    .bytes()?;
    let downloaded_size = i64::try_from(downloaded.len())?;
    let downloaded_hash = sha256_reader(downloaded.as_ref())?;
    if downloaded_size != patch_size || downloaded_hash != patch_hash {
        bail!(
            "Published artifact verification failed: expected {patch_size} bytes/{patch_hash}, got {downloaded_size} bytes/{downloaded_hash}"
        );
    }

    println!();
    println!("Patch {patch_number} published and verified end-to-end.");
    println!("Patch format: bsdiff");
    println!("Architecture: {}", options.architecture);
    println!("Channel: {}", options.channel);
    println!("Rollout: {}%", options.rollout_percentage);
    println!("Manifest version: {}", options.manifest_version);
    println!("Base SHA-256: {base_hash}");
    println!("Base patch: {base_patch_number}");
    println!("Target SHA-256: {target_hash}");
    println!("Target size: {target_size} bytes");
    println!("Patch SHA-256: {patch_hash}");
    println!("Patch size: {patch_size} bytes");
    println!("Manifest signature: present");
    Ok(())
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Commands::TokenHash { token } => {
            println!("{}", hex::encode(Sha256::digest(token.as_bytes())));
        }
        Commands::Diff { old, new, output } => create_diff(&old, &new, &output)?,
        Commands::Verify { file, sha256 } => verify_file(&file, &sha256)?,
        Commands::Apply {
            old,
            patch,
            output,
            expected_sha256,
        } => {
            apply_patch(&old, &patch, &output, expected_sha256.as_deref())?;
        }
        Commands::Publish {
            server_url,
            token,
            app_id,
            release_id,
            version,
            platform,
            channel,
            rollout_percentage,
            manifest_version,
            old,
            new,
            architecture,
            storage_dir,
            ssh_target,
            ssh_port,
            remote_storage_dir,
            artifact_prefix,
            patch_number,
            output,
        } => publish(PublishOptions {
            server_url: &server_url,
            token: &token,
            app_id: &app_id,
            release_id: &release_id,
            version: &version,
            platform: &platform,
            channel: &channel,
            rollout_percentage,
            manifest_version,
            old: &old,
            new: &new,
            architecture: &architecture,
            storage_dir: &storage_dir,
            ssh_target: ssh_target.as_deref(),
            ssh_port,
            remote_storage_dir: remote_storage_dir.as_deref(),
            artifact_prefix: &artifact_prefix,
            patch_number,
            output: output.as_deref(),
        })?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        api_url, artifact_path, sha256_reader, validate_remote_storage_dir, validate_ssh_target,
    };

    #[test]
    fn joins_api_urls_without_duplicate_slashes() {
        assert_eq!(
            api_url("http://localhost:8080/", "/api/v1/apps"),
            "http://localhost:8080/api/v1/apps"
        );
    }

    #[test]
    fn creates_normalized_public_artifact_path() {
        assert_eq!(
            artifact_path("/patches/", "/delta.patch"),
            "patches/delta.patch"
        );
    }

    #[test]
    fn hashes_streamed_content() {
        assert_eq!(
            sha256_reader("patchforge".as_bytes()).unwrap(),
            "0615616fee1d7698f2e26edb0e44a485957de0d982713b211e2564f7c05d931f"
        );
    }

    #[test]
    fn accepts_safe_remote_publish_destinations() {
        validate_ssh_target("kmt@104.207.92.53").unwrap();
        assert_eq!(
            validate_remote_storage_dir(Path::new("/srv/patchforge/storage")).unwrap(),
            "/srv/patchforge/storage"
        );
    }

    #[test]
    fn rejects_remote_shell_metacharacters_and_parent_paths() {
        assert!(validate_ssh_target("host;touch/tmp/pwned").is_err());
        assert!(validate_remote_storage_dir(Path::new("/srv/../tmp")).is_err());
        assert!(validate_remote_storage_dir(Path::new("relative/storage")).is_err());
    }
}
