use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;
use zip::ZipArchive;

#[derive(Parser)]
#[command(name = "patchforge")]
#[command(about = "PatchForge OTA patch tooling")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Clap owns this enum only during command parsing, so the larger Publish
// variant does not remain in long-lived application state.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Commands {
    /// Save a publisher credential for a PatchForge server.
    Login {
        /// Public HTTPS URL of the PatchForge server.
        #[arg(long)]
        url: String,
        /// Publisher token override. Omit it to enter the token securely.
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
    /// Connect this Flutter project to a PatchForge server.
    Init {
        /// Public HTTPS URL of the PatchForge server.
        #[arg(long)]
        url: String,
        /// Publisher token override. Saved credentials are used by default.
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// App name to create or reuse when --app-id is omitted.
        #[arg(long, default_value = "PatchForge App")]
        app_name: String,
        /// Reuse a specific existing app instead of selecting by name.
        #[arg(long)]
        app_id: Option<String>,
        #[arg(long, default_value = "android")]
        platform: String,
        #[arg(long, default_value = "stable")]
        channel: String,
        /// Project configuration file to create.
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        /// Replace an existing project configuration.
        #[arg(long)]
        force: bool,
    },
    /// Check this Flutter project's local tools, configuration, and server connection.
    Doctor {
        /// Project configuration file to check.
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        /// Publisher token override. Saved credentials are used by default.
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
    /// Build and register the baseline release for this Flutter project.
    Release {
        #[command(subcommand)]
        command: Option<ReleaseCommands>,
        /// Override the Flutter version detected from pubspec.yaml.
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        /// Android ABI to build and retain as the local patch baseline.
        #[arg(long, default_value = "arm64-v8a")]
        architecture: String,
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
    /// Build and publish the next OTA patch using the project configuration.
    Patch {
        #[command(subcommand)]
        command: Option<PatchCommands>,
        /// Advanced mode: use this APK or libapp.so instead of the cached base.
        #[arg(long)]
        old: Option<PathBuf>,
        /// Advanced mode: use this APK or libapp.so instead of building Flutter.
        #[arg(long)]
        new: Option<PathBuf>,
        #[arg(long, default_value = "arm64-v8a")]
        architecture: String,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(i32).range(0..=100))]
        rollout_percentage: i32,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(i32).range(1..=2))]
        manifest_version: i32,
        #[arg(long)]
        patch_number: Option<i32>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
    /// Print the SHA-256 hash used in server token configuration.
    #[command(hide = true)]
    TokenHash {
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: String,
    },
    #[command(hide = true)]
    Diff {
        #[arg(long)]
        old: PathBuf,
        #[arg(long)]
        new: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    #[command(hide = true)]
    Verify {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        sha256: String,
    },
    #[command(hide = true)]
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
    #[command(hide = true)]
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
        /// Legacy server-local artifact storage root. Omit to upload over HTTPS.
        #[arg(long, conflicts_with = "ssh_target")]
        storage_dir: Option<PathBuf>,
        /// Legacy SSH destination. Omit to upload over the authenticated HTTPS API.
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

#[derive(Subcommand)]
enum ReleaseCommands {
    /// Create or select a release and save it as the project's active release.
    #[command(hide = true)]
    Create {
        #[arg(long)]
        version: String,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
enum PatchCommands {
    /// Create and locally verify a BSDIFF patch without publishing it.
    #[command(hide = true)]
    Create {
        #[arg(long)]
        old: PathBuf,
        #[arg(long)]
        new: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "arm64-v8a")]
        architecture: String,
    },
    /// Create, upload, sign, and verify the next patch for the active release.
    #[command(hide = true)]
    Release {
        #[arg(long)]
        old: PathBuf,
        #[arg(long)]
        new: PathBuf,
        #[arg(long, default_value = "arm64-v8a")]
        architecture: String,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(i32).range(0..=100))]
        rollout_percentage: i32,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(i32).range(1..=2))]
        manifest_version: i32,
        #[arg(long)]
        patch_number: Option<i32>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "patchforge.yaml")]
        config: PathBuf,
        #[arg(long, env = "PATCHFORGE_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProjectConfig {
    schema_version: u32,
    server_url: String,
    app_id: String,
    app_name: String,
    platform: String,
    channel: String,
    signing_key_id: String,
    signing_public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppRecord {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct CreateAppRequest<'a> {
    name: &'a str,
}

#[derive(Debug, Deserialize)]
struct ReleaseRecord {
    id: String,
    version: String,
    platform: String,
    channel: String,
}

#[derive(Serialize)]
struct CreateReleaseRequest<'a> {
    version: &'a str,
    platform: &'a str,
    channel: &'a str,
}

#[derive(Debug, Deserialize)]
struct SigningKeyRecord {
    key_id: String,
    public_key_base64: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct SystemInfo {
    server_version: String,
    storage: SystemStorageInfo,
    signing: SystemSigningInfo,
}

#[derive(Debug, Deserialize)]
struct SystemStorageInfo {
    provider: String,
}

#[derive(Debug, Deserialize)]
struct SystemSigningInfo {
    key_id: String,
    public_key_base64: String,
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
struct UploadedArtifact {
    artifact_path: String,
    sha256: String,
    file_size: u64,
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
    storage_dir: Option<&'a Path>,
    ssh_target: Option<&'a str>,
    ssh_port: u16,
    remote_storage_dir: Option<&'a Path>,
    artifact_prefix: &'a str,
    patch_number: Option<i32>,
    output: Option<&'a Path>,
    require_latest_base: bool,
}

struct InitOptions<'a> {
    server_url: &'a str,
    token: Option<&'a str>,
    app_name: &'a str,
    app_id: Option<&'a str>,
    platform: &'a str,
    channel: &'a str,
    config_path: &'a Path,
    force: bool,
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

fn normalize_server_url(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value).context("Server URL is invalid")?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !local_http {
        bail!("Server URL must use HTTPS (plain HTTP is allowed only for localhost)");
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!("Server URL must be an origin without credentials, path, query, or fragment");
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn patchforge_config_home() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("patchforge"));
    }
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.join(".config/patchforge"))
        .context("HOME or XDG_CONFIG_HOME must be set to store publisher credentials")
}

fn credential_path(server_url: &str, app_id: &str) -> Result<PathBuf> {
    let server_fingerprint = hex::encode(Sha256::digest(server_url.as_bytes()));
    Ok(patchforge_config_home()?
        .join("credentials")
        .join(format!("{}-{app_id}.token", &server_fingerprint[..16])))
}

fn server_credential_path(server_url: &str) -> Result<PathBuf> {
    let server_fingerprint = hex::encode(Sha256::digest(server_url.as_bytes()));
    Ok(patchforge_config_home()?
        .join("credentials")
        .join(format!("{}.token", &server_fingerprint[..16])))
}

fn write_private_token(path: &Path, token: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("Publisher token path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn read_private_token(path: &Path) -> Result<String> {
    #[cfg(unix)]
    {
        let mode = fs::metadata(path)
            .with_context(|| format!("Failed to inspect publisher credential {}", path.display()))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "Publisher token file must not be accessible by group or others: {}",
                path.display()
            );
        }
    }
    let token = fs::read_to_string(path)
        .with_context(|| format!("Failed to read publisher credential {}", path.display()))?
        .trim()
        .to_string();
    if token.is_empty() {
        bail!("Publisher token is empty: {}", path.display());
    }
    Ok(token)
}

fn prompt_publisher_token(server_url: &str) -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!(
            "No publisher credential is available for {server_url}. Run `patchforge login --url {server_url}` in a terminal or set PATCHFORGE_TOKEN for automation"
        );
    }
    let token = rpassword::prompt_password("Publisher token: ")?
        .trim()
        .to_string();
    if token.is_empty() {
        bail!("Publisher token cannot be empty");
    }
    Ok(token)
}

fn setup_token(server_url: &str, override_token: Option<&str>) -> Result<String> {
    if let Some(token) = override_token {
        let token = token.trim();
        if token.is_empty() {
            bail!("Publisher token cannot be empty");
        }
        return Ok(token.to_string());
    }

    let path = server_credential_path(server_url)?;
    if path.exists() {
        return read_private_token(&path);
    }
    prompt_publisher_token(server_url)
}

fn login_to_server(server_url: &str, token_override: Option<&str>) -> Result<()> {
    let server_url = normalize_server_url(server_url)?;
    let token = match token_override {
        Some(token) => setup_token(&server_url, Some(token))?,
        None => prompt_publisher_token(&server_url)?,
    };
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    checked_response(
        client.get(api_url(&server_url, "health")).send()?,
        "Checking PatchForge server health",
    )?;
    checked_response(
        client
            .get(api_url(&server_url, "api/v1/system"))
            .bearer_auth(&token)
            .send()?,
        "Authenticating publisher credential",
    )?;

    let path = server_credential_path(&server_url)?;
    write_private_token(&path, &token)?;
    println!("Publisher login saved.");
    println!("Server: {server_url}");
    println!("Credential: {}", path.display());
    println!("The raw token was not written to the project or printed.");
    Ok(())
}

fn save_project_config(path: &Path, config: &ProjectConfig, replace: bool) -> Result<()> {
    if path.exists() && !replace {
        bail!(
            "Configuration already exists at {}. Use --force to replace it",
            path.display()
        );
    }
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(config)?;
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(yaml.as_bytes())?;
    temporary.flush()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to save {}", path.display()))?;
    Ok(())
}

fn load_project_config(path: &Path) -> Result<ProjectConfig> {
    let source = fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read {}. Run `patchforge init` first",
            path.display()
        )
    })?;
    let config: ProjectConfig = serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid PatchForge configuration in {}", path.display()))?;
    if config.schema_version != 1 {
        bail!(
            "Unsupported PatchForge configuration schema {}",
            config.schema_version
        );
    }
    normalize_server_url(&config.server_url)?;
    if !is_supported_channel(&config.channel) {
        bail!("Unsupported channel '{}' in configuration", config.channel);
    }
    Ok(config)
}

fn flutter_project_version(path: &Path) -> Result<String> {
    let source = fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read {}. Run from the Flutter project root or pass --version",
            path.display()
        )
    })?;
    let pubspec: serde_yaml::Value = serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid Flutter pubspec at {}", path.display()))?;
    let declared = pubspec
        .get("version")
        .and_then(serde_yaml::Value::as_str)
        .context("Flutter pubspec has no string version; pass --version")?;
    let version = declared
        .split_once('+')
        .map_or(declared, |(value, _)| value)
        .trim();
    if version.is_empty() {
        bail!("Flutter pubspec version is empty; pass --version");
    }
    Ok(version.to_string())
}

fn validate_flutter_project_for_init(root: &Path) -> Result<()> {
    let pubspec_path = root.join("pubspec.yaml");
    let source = fs::read_to_string(&pubspec_path).with_context(|| {
        format!(
            "Flutter pubspec not found at {}. Run PatchForge from the Flutter project root",
            pubspec_path.display()
        )
    })?;
    let pubspec: serde_yaml::Value = serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid Flutter pubspec at {}", pubspec_path.display()))?;
    let has_sdk = pubspec
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("patchforge_sdk"))
        .is_some();
    if !has_sdk {
        bail!(
            "Flutter dependency 'patchforge_sdk' is missing. Run `flutter pub add patchforge_sdk` before `patchforge init`"
        );
    }
    Ok(())
}

fn locked_package_version(root: &Path, package: &str) -> Result<String> {
    let path = root.join("pubspec.lock");
    let source = fs::read_to_string(&path).with_context(|| {
        format!(
            "Failed to read {}. Run `flutter pub get` first",
            path.display()
        )
    })?;
    let lockfile: serde_yaml::Value = serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid Flutter lockfile at {}", path.display()))?;
    lockfile
        .get("packages")
        .and_then(|packages| packages.get(package))
        .and_then(|entry| entry.get("version"))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("{package} is missing from {}", path.display()))
}

fn parse_version_core(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value
        .split_once('-')
        .map_or(value, |(core, _)| core)
        .split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn version_at_least(value: &str, minimum: &str) -> bool {
    match (parse_version_core(value), parse_version_core(minimum)) {
        (Some(value), Some(minimum)) => value >= minimum,
        _ => false,
    }
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(name))
        .find(|candidate| candidate.is_file())
}

#[derive(Default)]
struct DoctorReport {
    passed: usize,
    warnings: usize,
    failures: Vec<String>,
}

impl DoctorReport {
    fn pass(&mut self, label: &str, detail: impl AsRef<str>) {
        self.passed += 1;
        println!("[PASS] {label}: {}", detail.as_ref());
    }

    fn warn(&mut self, label: &str, detail: impl AsRef<str>) {
        self.warnings += 1;
        println!("[WARN] {label}: {}", detail.as_ref());
    }

    fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        let detail = detail.as_ref();
        println!("[FAIL] {label}: {detail}");
        self.failures.push(format!("{label}: {detail}"));
    }

    fn finish(self) -> Result<()> {
        println!();
        if self.failures.is_empty() {
            println!(
                "PatchForge doctor: all required checks passed ({} passed, {} warnings)",
                self.passed, self.warnings
            );
            return Ok(());
        }
        bail!(
            "PatchForge doctor found {} required issue(s); fix the [FAIL] items above",
            self.failures.len()
        )
    }
}

fn ensure_gitignore_entry(root: &Path, entry: &str) -> Result<bool> {
    let path = root.join(".gitignore");
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()));
        }
    };
    if source.lines().any(|line| line.trim() == entry) {
        return Ok(false);
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    if !source.is_empty() && !source.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.write_all(entry.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(true)
}

fn project_root(config_path: &Path) -> &Path {
    config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn android_build_target(architecture: &str) -> Result<(&'static str, &'static str)> {
    match architecture {
        "arm64-v8a" => Ok(("android-arm64", "app-arm64-v8a-release.apk")),
        "armeabi-v7a" => Ok(("android-arm", "app-armeabi-v7a-release.apk")),
        "x86_64" => Ok(("android-x64", "app-x86_64-release.apk")),
        _ => bail!(
            "Unsupported architecture '{architecture}'. Expected arm64-v8a, armeabi-v7a, or x86_64"
        ),
    }
}

fn flutter_build_defines(config: &ProjectConfig) -> Vec<String> {
    vec![
        format!("PATCHFORGE_URL={}", config.server_url),
        format!("PATCHFORGE_APP_ID={}", config.app_id),
        format!("PATCHFORGE_CHANNEL={}", config.channel),
        format!("PATCHFORGE_KEY_ID={}", config.signing_key_id),
        format!("PATCHFORGE_PUBLIC_KEY={}", config.signing_public_key),
    ]
}

fn build_flutter_apk(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<PathBuf> {
    if config.platform != "android" {
        bail!("Automatic builds currently support Android projects only");
    }
    let root = project_root(config_path);
    let pubspec = root.join("pubspec.yaml");
    if !pubspec.is_file() {
        bail!(
            "Flutter pubspec not found at {}. Run PatchForge from the Flutter project root",
            pubspec.display()
        );
    }
    let (target_platform, apk_name) = android_build_target(architecture)?;
    println!("Building Flutter release for {architecture}...");
    let mut command = Command::new("flutter");
    command.current_dir(root).args([
        "build",
        "apk",
        "--release",
        "--target-platform",
        target_platform,
        "--split-per-abi",
    ]);
    for define in flutter_build_defines(config) {
        command.arg(format!("--dart-define={define}"));
    }
    let status = command
        .status()
        .context("Failed to run Flutter. Is the flutter command available in PATH?")?;
    if !status.success() {
        bail!("Flutter release build failed");
    }
    let apk = root.join("build/app/outputs/flutter-apk").join(apk_name);
    if !apk.is_file() {
        bail!(
            "Flutter completed but the expected APK was not found at {}",
            apk.display()
        );
    }
    println!("Built APK: {}", apk.display());
    Ok(apk)
}

fn release_state_dir(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<PathBuf> {
    let release_id = config
        .release_id
        .as_deref()
        .context("No active release. Run `patchforge release` first")?;
    Ok(project_root(config_path)
        .join(".patchforge/releases")
        .join(release_id)
        .join(architecture))
}

fn cached_runtime_path(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<PathBuf> {
    Ok(release_state_dir(config_path, config, architecture)?.join("current-libapp.so"))
}

fn release_runtime_path(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<PathBuf> {
    Ok(release_state_dir(config_path, config, architecture)?.join("release-libapp.so"))
}

fn release_apk_path(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<PathBuf> {
    Ok(release_state_dir(config_path, config, architecture)?.join("release.apk"))
}

fn extract_runtime_to_cache(
    apk: &Path,
    architecture: &str,
    destination: &Path,
    replace: bool,
) -> Result<()> {
    let parent = destination
        .parent()
        .context("PatchForge runtime cache path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    extract_libapp(apk, architecture, temporary.path())?;

    if destination.is_file() && !replace {
        let existing_hash = sha256_file(destination)?;
        let built_hash = sha256_file(temporary.path())?;
        if existing_hash != built_hash {
            bail!(
                "This release already has a different local baseline. Use `patchforge patch` for code changes or create a new app version"
            );
        }
        println!("Local release baseline is already current.");
        return Ok(());
    }

    temporary
        .persist(destination)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to save runtime cache at {}", destination.display()))?;
    println!("Saved PatchForge runtime state: {}", destination.display());
    Ok(())
}

fn build_release_baseline(
    config_path: &Path,
    config: &ProjectConfig,
    architecture: &str,
) -> Result<()> {
    let apk = build_flutter_apk(config_path, config, architecture)?;
    let token = publisher_token(config, None)?;
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let release_id = config
        .release_id
        .as_deref()
        .context("No active release. Run `patchforge release` first")?;
    let temporary = TempDir::new()?;
    let built_runtime = temporary.path().join("release-libapp.so");
    extract_libapp(&apk, architecture, &built_runtime)?;
    let built_hash = sha256_file(&built_runtime)?;
    let built_size = i64::try_from(fs::metadata(&built_runtime)?.len())?;

    let artifacts_url = api_url(
        &config.server_url,
        &format!("api/v1/releases/{release_id}/artifacts"),
    );
    let artifacts: Vec<ReleaseArtifact> = checked_response(
        client.get(&artifacts_url).bearer_auth(&token).send()?,
        "Reading release baseline artifacts",
    )?
    .json()?;
    let patches_url = api_url(
        &config.server_url,
        &format!("api/v1/releases/{release_id}/patches"),
    );
    let patches: Vec<PatchRecord> = checked_response(
        client.get(&patches_url).bearer_auth(&token).send()?,
        "Reading release patches",
    )?
    .json()?;
    let architecture_patches: Vec<&PatchRecord> = patches
        .iter()
        .filter(|patch| patch.architecture.as_deref() == Some(architecture))
        .collect();

    match artifacts
        .iter()
        .find(|artifact| artifact.architecture == architecture)
    {
        Some(baseline) => {
            if baseline.sha256 != built_hash || baseline.file_size != built_size {
                bail!(
                    "The Flutter build does not match the registered {architecture} release baseline. Check out the store-release source before rerunning `patchforge release`"
                );
            }
        }
        None => {
            if !architecture_patches.is_empty() {
                bail!("The release has {architecture} patches but no registered baseline artifact");
            }
            let request = CreateReleaseArtifactRequest {
                architecture,
                sha256: &built_hash,
                file_size: built_size,
            };
            let created: ReleaseArtifact = checked_response(
                client
                    .post(&artifacts_url)
                    .bearer_auth(&token)
                    .json(&request)
                    .send()?,
                "Registering release baseline artifact",
            )?
            .json()?;
            println!(
                "Registered {} release baseline: {}",
                created.architecture, created.sha256
            );
        }
    }

    let release_runtime = release_runtime_path(config_path, config, architecture)?;
    let current_runtime = cached_runtime_path(config_path, config, architecture)?;
    let release_apk = release_apk_path(config_path, config, architecture)?;
    extract_runtime_to_cache(&apk, architecture, &release_runtime, false)?;
    if architecture_patches.is_empty() && !current_runtime.exists() {
        fs::copy(&release_runtime, &current_runtime).with_context(|| {
            format!(
                "Failed to initialize patch state at {}",
                current_runtime.display()
            )
        })?;
    } else if architecture_patches.is_empty() {
        if sha256_file(&current_runtime)? != built_hash {
            bail!("Local runtime state differs from the unpatched release baseline");
        }
    } else if let Some(latest) = architecture_patches
        .iter()
        .max_by_key(|patch| patch.patch_number)
    {
        let current_hash = sha256_file(&current_runtime).with_context(|| {
            format!(
                "Local current runtime is missing. Restore the latest patched source and run advanced --old/--new mode once: {}",
                current_runtime.display()
            )
        })?;
        if latest.target_sha256.as_deref() != Some(current_hash.as_str()) {
            bail!(
                "Local runtime state does not match the latest server patch {}",
                latest.patch_number
            );
        }
    }
    if !release_apk.exists() {
        fs::copy(&apk, &release_apk).with_context(|| {
            format!(
                "Failed to preserve release APK at {}",
                release_apk.display()
            )
        })?;
    }
    println!();
    println!("Release baseline is ready.");
    println!("Install or distribute: {}", release_apk.display());
    println!("Next: edit Dart code, then run `patchforge patch`");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_and_publish_project_patch(
    config_path: &Path,
    token_override: Option<String>,
    architecture: &str,
    rollout_percentage: i32,
    manifest_version: i32,
    patch_number: Option<i32>,
    output: Option<&Path>,
) -> Result<()> {
    let config = load_project_config(config_path)?;
    let cached = cached_runtime_path(config_path, &config, architecture)?;
    if !cached.is_file() {
        bail!(
            "Local release baseline is missing at {}. Run `patchforge release` first",
            cached.display()
        );
    }

    let target_apk = build_flutter_apk(config_path, &config, architecture)?;
    let temporary = TempDir::new()?;
    let target_runtime = temporary.path().join("target-libapp.so");
    extract_libapp(&target_apk, architecture, &target_runtime)?;
    if sha256_file(&cached)? == sha256_file(&target_runtime)? {
        bail!("No Dart AOT changes were detected; there is no patch to publish");
    }

    publish_project_patch(
        config_path,
        token_override,
        &cached,
        &target_apk,
        architecture,
        rollout_percentage,
        manifest_version,
        patch_number,
        output,
        true,
    )?;
    extract_runtime_to_cache(&target_apk, architecture, &cached, true)?;
    println!("Local patch state advanced to the published target.");
    Ok(())
}

fn publisher_token(config: &ProjectConfig, override_token: Option<String>) -> Result<String> {
    match override_token {
        Some(token) if !token.trim().is_empty() => Ok(token.trim().to_string()),
        Some(_) => bail!("Publisher token is empty"),
        None => read_private_token(&credential_path(&config.server_url, &config.app_id)?),
    }
}

fn doctor(config_path: &Path, token_override: Option<String>) -> Result<()> {
    const MINIMUM_SDK_VERSION: &str = "0.6.1";

    println!("PatchForge doctor");
    println!("Project: {}", project_root(config_path).display());
    println!();

    let root = project_root(config_path);
    let mut report = DoctorReport::default();

    match validate_flutter_project_for_init(root) {
        Ok(()) => report.pass("Flutter project", root.display().to_string()),
        Err(error) => report.fail("Flutter project", format!("{error:#}")),
    }

    if root.join("android").is_dir() {
        report.pass(
            "Android project",
            root.join("android").display().to_string(),
        );
    } else {
        report.fail(
            "Android project",
            "android/ is missing; PatchForge currently supports Flutter Android apps",
        );
    }

    match locked_package_version(root, "patchforge_sdk") {
        Ok(version) if version_at_least(&version, MINIMUM_SDK_VERSION) => {
            report.pass("PatchForge SDK", version)
        }
        Ok(version) => report.fail(
            "PatchForge SDK",
            format!(
                "version {version} is too old; run `flutter pub add patchforge_sdk:^{MINIMUM_SDK_VERSION}`"
            ),
        ),
        Err(error) => report.fail("PatchForge SDK", format!("{error:#}")),
    }

    match Command::new("flutter").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("flutter is available")
                .trim()
                .to_string();
            report.pass("Flutter command", version);
        }
        Ok(output) => report.fail(
            "Flutter command",
            format!("`flutter --version` exited with {}", output.status),
        ),
        Err(error) => report.fail(
            "Flutter command",
            format!("{error}; install Flutter and add it to PATH"),
        ),
    }

    for tool in ["bsdiff", "bspatch"] {
        match executable_in_path(tool) {
            Some(path) => report.pass(tool, path.display().to_string()),
            None => report.fail(
                tool,
                "not found in PATH; install the bsdiff package before publishing patches",
            ),
        }
    }

    let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    if gitignore.lines().any(|line| line.trim() == ".patchforge/") {
        report.pass("Local state ignore", ".patchforge/ is excluded from Git");
    } else {
        report.warn(
            "Local state ignore",
            "add `.patchforge/` to .gitignore before creating a release",
        );
    }

    let config = match load_project_config(config_path) {
        Ok(config) => {
            report.pass("Project configuration", config_path.display().to_string());
            Some(config)
        }
        Err(error) => {
            report.fail("Project configuration", format!("{error:#}"));
            None
        }
    };

    if let Some(config) = config {
        let credential_description = if token_override.is_some() {
            "provided securely for this command".to_string()
        } else {
            credential_path(&config.server_url, &config.app_id)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "saved publisher credential".to_string())
        };
        let token = match publisher_token(&config, token_override) {
            Ok(token) => {
                report.pass("Publisher credential", credential_description);
                Some(token)
            }
            Err(error) => {
                report.fail("Publisher credential", format!("{error:#}"));
                None
            }
        };

        let client = match Client::builder().timeout(Duration::from_secs(15)).build() {
            Ok(client) => Some(client),
            Err(error) => {
                report.fail("HTTP client", error.to_string());
                None
            }
        };

        if let Some(client) = client.as_ref() {
            match client.get(api_url(&config.server_url, "health")).send() {
                Ok(response) => match checked_response(response, "Server health check") {
                    Ok(_) => report.pass("Server health", &config.server_url),
                    Err(error) => report.fail("Server health", format!("{error:#}")),
                },
                Err(error) => report.fail("Server health", error.to_string()),
            }
        }

        if let (Some(client), Some(token)) = (client.as_ref(), token.as_ref()) {
            let system = match client
                .get(api_url(&config.server_url, "api/v1/system"))
                .bearer_auth(token)
                .send()
                .and_then(Response::error_for_status)
            {
                Ok(response) => match response.json::<SystemInfo>() {
                    Ok(system) => {
                        report.pass(
                            "Publisher authentication",
                            format!(
                                "server {} using {} storage",
                                system.server_version, system.storage.provider
                            ),
                        );
                        Some(system)
                    }
                    Err(error) => {
                        report.fail("Publisher authentication", error.to_string());
                        None
                    }
                },
                Err(error) => {
                    report.fail("Publisher authentication", error.to_string());
                    None
                }
            };

            match client
                .get(api_url(&config.server_url, "api/v1/apps"))
                .bearer_auth(token)
                .send()
                .and_then(Response::error_for_status)
            {
                Ok(response) => match response.json::<Vec<AppRecord>>() {
                    Ok(apps) => match apps.into_iter().find(|app| app.id == config.app_id) {
                        Some(app) if app.name == config.app_name => {
                            report.pass("Application", format!("{} ({})", app.name, app.id))
                        }
                        Some(app) => report.warn(
                            "Application",
                            format!(
                                "ID exists but server name '{}' differs from local name '{}'",
                                app.name, config.app_name
                            ),
                        ),
                        None => report.fail(
                            "Application",
                            format!("{} does not exist on this server", config.app_id),
                        ),
                    },
                    Err(error) => report.fail("Application", error.to_string()),
                },
                Err(error) => report.fail("Application", error.to_string()),
            }

            if let Some(system) = system {
                if system.signing.key_id == config.signing_key_id
                    && system.signing.public_key_base64 == config.signing_public_key
                {
                    report.pass("Signing identity", config.signing_key_id.clone());
                } else {
                    report.fail(
                        "Signing identity",
                        format!(
                            "server active key '{}' does not match local key '{}'; rerun `patchforge init --force` after reviewing the rotation",
                            system.signing.key_id, config.signing_key_id
                        ),
                    );
                }
            }

            if let Some(release_id) = config.release_id.as_deref() {
                match client
                    .get(api_url(
                        &config.server_url,
                        &format!("api/v1/apps/{}/releases", config.app_id),
                    ))
                    .bearer_auth(token)
                    .send()
                    .and_then(Response::error_for_status)
                {
                    Ok(response) => match response.json::<Vec<ReleaseRecord>>() {
                        Ok(releases) => match releases
                            .into_iter()
                            .find(|release| release.id == release_id)
                        {
                            Some(release)
                                if Some(release.version.as_str())
                                    == config.release_version.as_deref()
                                    && release.platform == config.platform
                                    && release.channel == config.channel =>
                            {
                                report.pass(
                                    "Active release",
                                    format!(
                                        "{} · {} · {}",
                                        release.version, release.platform, release.channel
                                    ),
                                );
                            }
                            Some(_) => report.fail(
                                "Active release",
                                "server release metadata differs from patchforge.yaml",
                            ),
                            None => report.fail(
                                "Active release",
                                format!("{release_id} does not exist for this app"),
                            ),
                        },
                        Err(error) => report.fail("Active release", error.to_string()),
                    },
                    Err(error) => report.fail("Active release", error.to_string()),
                }
            } else {
                report.warn(
                    "Active release",
                    "none selected yet; run `patchforge release` when the store build is ready",
                );
            }
        }
    }

    report.finish()
}

fn initialize_project(options: InitOptions<'_>) -> Result<()> {
    if options.config_path.exists() && !options.force {
        bail!(
            "Configuration already exists at {}. Use --force to replace it",
            options.config_path.display()
        );
    }
    let server_url = normalize_server_url(options.server_url)?;
    if options.app_name.trim().is_empty() || options.app_name.chars().count() > 100 {
        bail!("App name must contain between 1 and 100 characters");
    }
    if options.platform != "android" {
        bail!("PatchForge currently supports the android platform only");
    }
    if !is_supported_channel(options.channel) {
        bail!("Unsupported channel '{}'", options.channel);
    }
    let root = project_root(options.config_path);
    validate_flutter_project_for_init(root)?;

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    checked_response(
        client.get(api_url(&server_url, "health")).send()?,
        "Checking PatchForge server health",
    )?;
    let token = setup_token(&server_url, options.token)?;
    let apps_url = api_url(&server_url, "api/v1/apps");
    let apps: Vec<AppRecord> = checked_response(
        client.get(&apps_url).bearer_auth(&token).send()?,
        "Reading server apps",
    )?
    .json()?;
    let app = if let Some(app_id) = options.app_id {
        apps.into_iter()
            .find(|app| app.id == app_id)
            .with_context(|| format!("App {app_id} does not exist on this server"))?
    } else {
        let matches: Vec<AppRecord> = apps
            .into_iter()
            .filter(|app| app.name == options.app_name.trim())
            .collect();
        match matches.len() {
            0 => checked_response(
                client
                    .post(&apps_url)
                    .bearer_auth(&token)
                    .json(&CreateAppRequest {
                        name: options.app_name.trim(),
                    })
                    .send()?,
                "Creating PatchForge app",
            )?
            .json()?,
            1 => matches.into_iter().next().expect("one app match"),
            _ => bail!(
                "More than one app is named '{}'; rerun with --app-id",
                options.app_name
            ),
        }
    };

    let signing_keys: Vec<SigningKeyRecord> = checked_response(
        client
            .get(api_url(&server_url, "api/v1/signing-keys"))
            .bearer_auth(&token)
            .send()?,
        "Reading signing keys",
    )?
    .json()?;
    let mut active_keys = signing_keys
        .into_iter()
        .filter(|key| key.status == "active");
    let signing_key = active_keys
        .next()
        .context("Server has no active signing key")?;
    if active_keys.next().is_some() {
        bail!("Server returned more than one active signing key");
    }

    let publisher_token_file = credential_path(&server_url, &app.id)?;
    write_private_token(&publisher_token_file, &token)?;
    write_private_token(&server_credential_path(&server_url)?, &token)?;
    let project = ProjectConfig {
        schema_version: 1,
        server_url,
        app_id: app.id,
        app_name: app.name,
        platform: options.platform.to_string(),
        channel: options.channel.to_string(),
        signing_key_id: signing_key.key_id,
        signing_public_key: signing_key.public_key_base64,
        release_id: None,
        release_version: None,
    };
    save_project_config(options.config_path, &project, options.force)?;
    let added_gitignore = ensure_gitignore_entry(root, ".patchforge/")?;

    println!("PatchForge project initialized.");
    println!("Configuration: {}", options.config_path.display());
    println!("Server: {}", project.server_url);
    println!("App: {} ({})", project.app_name, project.app_id);
    println!("Channel: {}", project.channel);
    println!("Signing key: {}", project.signing_key_id);
    println!("Publisher credential: {}", publisher_token_file.display());
    println!();
    println!("Flutter SDK values:");
    println!("  baseUrl: '{}'", project.server_url);
    println!("  appId: '{}'", project.app_id);
    println!(
        "  trustedPublicKeys: {{'{}': '{}'}}",
        project.signing_key_id, project.signing_public_key
    );
    println!("  channel: '{}'", project.channel);
    println!();
    if added_gitignore {
        println!(
            "Added .patchforge/ to {}",
            root.join(".gitignore").display()
        );
    }
    println!("Next:");
    println!("  1. Create PatchForgeConfig.fromEnvironment() at app startup and call update().");
    println!("  2. Run `patchforge release`.");
    Ok(())
}

fn create_or_select_release(
    config_path: &Path,
    token_override: Option<String>,
    version: &str,
    platform_override: Option<&str>,
    channel_override: Option<&str>,
) -> Result<ProjectConfig> {
    if version.trim().is_empty() {
        bail!("Release version cannot be empty");
    }
    let mut config = load_project_config(config_path)?;
    let token = publisher_token(&config, token_override)?;
    let platform = platform_override.unwrap_or(&config.platform);
    let channel = channel_override.unwrap_or(&config.channel);
    if platform != "android" {
        bail!("PatchForge currently supports the android platform only");
    }
    if !is_supported_channel(channel) {
        bail!("Unsupported channel '{channel}'");
    }

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let releases_url = api_url(
        &config.server_url,
        &format!("api/v1/apps/{}/releases", config.app_id),
    );
    let releases: Vec<ReleaseRecord> = checked_response(
        client.get(&releases_url).bearer_auth(&token).send()?,
        "Reading app releases",
    )?
    .json()?;
    let release = match releases.into_iter().find(|release| {
        release.version == version && release.platform == platform && release.channel == channel
    }) {
        Some(release) => {
            println!("Using existing release {}.", release.id);
            release
        }
        None => checked_response(
            client
                .post(&releases_url)
                .bearer_auth(&token)
                .json(&CreateReleaseRequest {
                    version,
                    platform,
                    channel,
                })
                .send()?,
            "Creating release",
        )?
        .json()?,
    };
    config.platform = release.platform;
    config.channel = release.channel;
    config.release_id = Some(release.id.clone());
    config.release_version = Some(release.version.clone());
    save_project_config(config_path, &config, true)?;
    println!("Active release: {}", release.id);
    println!("Version: {}", release.version);
    println!("Platform: {}", config.platform);
    println!("Channel: {}", config.channel);
    println!("Configuration updated: {}", config_path.display());
    println!();
    Ok(config)
}

fn create_local_patch(old: &Path, new: &Path, output: &Path, architecture: &str) -> Result<()> {
    if !is_supported_android_architecture(architecture) {
        bail!("Unsupported architecture '{architecture}'");
    }
    if output.exists() {
        bail!("Refusing to overwrite existing patch {}", output.display());
    }
    let temp = TempDir::new()?;
    let base = resolve_artifact(old, architecture, &temp.path().join("base-libapp.so"))?;
    let target = resolve_artifact(new, architecture, &temp.path().join("target-libapp.so"))?;
    let target_hash = sha256_file(&target)?;
    create_diff(&base, &target, output)?;
    apply_patch(
        &base,
        output,
        &temp.path().join("reconstructed-libapp.so"),
        Some(&target_hash),
    )?;
    println!("Local patch verified: {}", output.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn publish_project_patch(
    config_path: &Path,
    token_override: Option<String>,
    old: &Path,
    new: &Path,
    architecture: &str,
    rollout_percentage: i32,
    manifest_version: i32,
    patch_number: Option<i32>,
    output: Option<&Path>,
    require_latest_base: bool,
) -> Result<()> {
    let project = load_project_config(config_path)?;
    let token = publisher_token(&project, token_override)?;
    let release_id = project.release_id.as_deref().context(
        "No active release in patchforge.yaml. Run `patchforge release --version <installed-app-version>` first",
    )?;
    let version = project.release_version.as_deref().context(
        "No active release version in patchforge.yaml. Run `patchforge release --version <installed-app-version>` first",
    )?;
    publish(PublishOptions {
        server_url: &project.server_url,
        token: &token,
        app_id: &project.app_id,
        release_id,
        version,
        platform: &project.platform,
        channel: &project.channel,
        rollout_percentage,
        manifest_version,
        old,
        new,
        architecture,
        storage_dir: None,
        ssh_target: None,
        ssh_port: 22,
        remote_storage_dir: None,
        artifact_prefix: "patches",
        patch_number,
        output,
        require_latest_base,
    })
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

fn upload_api_artifact(
    client: &Client,
    server_url: &str,
    token: &str,
    source: &Path,
    public_path: &str,
    expected_sha256: &str,
    expected_size: i64,
) -> Result<()> {
    let file = File::open(source)
        .with_context(|| format!("Failed to read generated artifact {}", source.display()))?;
    let size = u64::try_from(expected_size).context("Artifact size cannot be negative")?;
    let upload_url = api_url(server_url, &format!("api/v1/artifacts/{public_path}"));
    let uploaded: UploadedArtifact = checked_response(
        client
            .put(upload_url)
            .bearer_auth(token)
            .header("content-type", "application/octet-stream")
            .header("x-patchforge-sha256", expected_sha256)
            .header("x-patchforge-file-size", size)
            .body(reqwest::blocking::Body::sized(file, size))
            .send()?,
        "Uploading patch artifact",
    )?
    .json()?;

    if uploaded.artifact_path != public_path
        || uploaded.sha256 != expected_sha256
        || uploaded.file_size != size
    {
        bail!("Server upload response does not match the generated patch: {uploaded:#?}");
    }
    println!("Uploaded artifact over HTTPS: {public_path}");
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
    if options.require_latest_base && base_patch_number != latest_number {
        bail!(
            "The local PatchForge runtime is at patch {base_patch_number}, but the server's latest {} patch is {latest_number}. Restore the latest project state or use advanced --old/--new mode deliberately",
            options.architecture
        );
    }
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
    match (
        options.storage_dir,
        options.ssh_target,
        options.remote_storage_dir,
    ) {
        (None, None, None) => upload_api_artifact(
            &client,
            options.server_url,
            options.token,
            &patch_path,
            &public_path,
            &patch_hash,
            patch_size,
        )?,
        (None, Some(ssh_target), Some(remote_storage_dir)) => upload_remote_artifact(
            &patch_path,
            ssh_target,
            options.ssh_port,
            remote_storage_dir,
            &filename,
        )?,
        (Some(storage_dir), None, None) => {
            let patch_storage_dir = storage_dir.join("patches");
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
        _ => bail!(
            "Use the HTTPS upload default, --storage-dir, or both --ssh-target and --remote-storage-dir"
        ),
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
        Commands::Login { url, token } => login_to_server(&url, token.as_deref())?,
        Commands::Init {
            url,
            token,
            app_name,
            app_id,
            platform,
            channel,
            config,
            force,
        } => initialize_project(InitOptions {
            server_url: &url,
            token: token.as_deref(),
            app_name: &app_name,
            app_id: app_id.as_deref(),
            platform: &platform,
            channel: &channel,
            config_path: &config,
            force,
        })?,
        Commands::Doctor { config, token } => doctor(&config, token)?,
        Commands::Release {
            command,
            version,
            platform,
            channel,
            architecture,
            config,
            token,
        } => match command {
            Some(ReleaseCommands::Create {
                version,
                platform,
                channel,
                config,
                token,
            }) => {
                create_or_select_release(
                    &config,
                    token,
                    &version,
                    platform.as_deref(),
                    channel.as_deref(),
                )?;
            }
            None => {
                let version = match version {
                    Some(version) => version,
                    None => {
                        let detected =
                            flutter_project_version(&project_root(&config).join("pubspec.yaml"))?;
                        println!("Detected Flutter version {detected} from pubspec.yaml.");
                        detected
                    }
                };
                let project = create_or_select_release(
                    &config,
                    token,
                    &version,
                    platform.as_deref(),
                    channel.as_deref(),
                )?;
                build_release_baseline(&config, &project, &architecture)?;
            }
        },
        Commands::Patch {
            command,
            old,
            new,
            architecture,
            rollout_percentage,
            manifest_version,
            patch_number,
            output,
            config,
            token,
        } => match command {
            Some(PatchCommands::Create {
                old,
                new,
                output,
                architecture,
            }) => create_local_patch(&old, &new, &output, &architecture)?,
            Some(PatchCommands::Release {
                old,
                new,
                architecture,
                rollout_percentage,
                manifest_version,
                patch_number,
                output,
                config,
                token,
            }) => publish_project_patch(
                &config,
                token,
                &old,
                &new,
                &architecture,
                rollout_percentage,
                manifest_version,
                patch_number,
                output.as_deref(),
                false,
            )?,
            None => match (old, new) {
                (None, None) => build_and_publish_project_patch(
                    &config,
                    token,
                    &architecture,
                    rollout_percentage,
                    manifest_version,
                    patch_number,
                    output.as_deref(),
                )?,
                (Some(old), Some(new)) => publish_project_patch(
                    &config,
                    token,
                    &old,
                    &new,
                    &architecture,
                    rollout_percentage,
                    manifest_version,
                    patch_number,
                    output.as_deref(),
                    false,
                )?,
                _ => bail!("Use both --old and --new, or omit both for an automatic Flutter build"),
            },
        },
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
            storage_dir: storage_dir.as_deref(),
            ssh_target: ssh_target.as_deref(),
            ssh_port,
            remote_storage_dir: remote_storage_dir.as_deref(),
            artifact_prefix: &artifact_prefix,
            patch_number,
            output: output.as_deref(),
            require_latest_base: false,
        })?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use std::path::Path;

    use super::{
        Cli, Commands, PatchCommands, ProjectConfig, ReleaseCommands, android_build_target,
        api_url, artifact_path, cached_runtime_path, ensure_gitignore_entry, flutter_build_defines,
        flutter_project_version, load_project_config, locked_package_version, normalize_server_url,
        release_apk_path, release_runtime_path, save_project_config, sha256_reader,
        validate_flutter_project_for_init, validate_remote_storage_dir, validate_ssh_target,
        version_at_least,
    };
    use tempfile::TempDir;

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

    #[test]
    fn requires_https_except_for_local_development() {
        assert_eq!(
            normalize_server_url("https://updates.example.com/").unwrap(),
            "https://updates.example.com"
        );
        assert_eq!(
            normalize_server_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert!(normalize_server_url("http://updates.example.com").is_err());
        assert!(normalize_server_url("https://updates.example.com/api").is_err());
        assert!(normalize_server_url("https://user:secret@updates.example.com").is_err());
    }

    #[test]
    fn round_trips_project_configuration_without_a_raw_token() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("patchforge.yaml");
        let expected = ProjectConfig {
            schema_version: 1,
            server_url: "https://updates.example.com".to_string(),
            app_id: "app-id".to_string(),
            app_name: "Example".to_string(),
            platform: "android".to_string(),
            channel: "stable".to_string(),
            signing_key_id: "main-2026".to_string(),
            signing_public_key: "public-key".to_string(),
            release_id: Some("release-id".to_string()),
            release_version: Some("1.0.0".to_string()),
        };

        save_project_config(&path, &expected, false).unwrap();
        let yaml = std::fs::read_to_string(&path).unwrap();
        assert!(!yaml.contains("raw-publisher-secret"));
        let actual = load_project_config(&path).unwrap();
        assert_eq!(actual.server_url, expected.server_url);
        assert_eq!(actual.app_id, expected.app_id);
        assert_eq!(actual.release_id, expected.release_id);
        assert!(save_project_config(&path, &expected, false).is_err());
    }

    #[test]
    fn reads_device_version_name_from_flutter_pubspec() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("pubspec.yaml");
        std::fs::write(&path, "name: example\nversion: 1.2.3+42\n").unwrap();
        assert_eq!(flutter_project_version(&path).unwrap(), "1.2.3");
    }

    #[test]
    fn validates_public_flutter_sdk_dependency() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("pubspec.yaml");
        std::fs::write(
            &path,
            "name: example\ndependencies:\n  flutter:\n    sdk: flutter\n  patchforge_sdk: ^0.6.0\n",
        )
        .unwrap();
        validate_flutter_project_for_init(temp.path()).unwrap();

        std::fs::write(
            &path,
            "name: example\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        let error = validate_flutter_project_for_init(temp.path()).unwrap_err();
        assert!(error.to_string().contains("flutter pub add patchforge_sdk"));
    }

    #[test]
    fn reads_and_validates_the_locked_sdk_version() {
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("pubspec.lock"),
            "packages:\n  patchforge_sdk:\n    dependency: direct main\n    source: hosted\n    version: \"0.6.1\"\n",
        )
        .unwrap();

        assert_eq!(
            locked_package_version(temp.path(), "patchforge_sdk").unwrap(),
            "0.6.1"
        );
        assert!(version_at_least("0.6.1", "0.6.1"));
        assert!(version_at_least("0.7.0", "0.6.1"));
        assert!(!version_at_least("0.6.0", "0.6.1"));
        assert!(!version_at_least("invalid", "0.6.1"));
    }

    #[test]
    fn adds_patchforge_state_to_gitignore_once() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".gitignore");
        std::fs::write(&path, ".dart_tool/\n").unwrap();

        assert!(ensure_gitignore_entry(temp.path(), ".patchforge/").unwrap());
        assert!(!ensure_gitignore_entry(temp.path(), ".patchforge/").unwrap());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            ".dart_tool/\n.patchforge/\n"
        );
    }

    #[test]
    fn parses_config_driven_workflow_commands() {
        let login = Cli::try_parse_from([
            "patchforge",
            "login",
            "--url",
            "https://updates.example.com",
        ])
        .unwrap();
        assert!(matches!(login.command, Commands::Login { token: None, .. }));

        let init =
            Cli::try_parse_from(["patchforge", "init", "--url", "https://updates.example.com"])
                .unwrap();
        assert!(matches!(init.command, Commands::Init { token: None, .. }));

        let release = Cli::try_parse_from(["patchforge", "release", "--version", "1.0.0"]).unwrap();
        assert!(matches!(
            release.command,
            Commands::Release {
                command: None,
                version: Some(_),
                ..
            }
        ));

        let inferred_release = Cli::try_parse_from(["patchforge", "release"]).unwrap();
        assert!(matches!(
            inferred_release.command,
            Commands::Release {
                command: None,
                version: None,
                ..
            }
        ));

        let doctor = Cli::try_parse_from(["patchforge", "doctor"]).unwrap();
        assert!(matches!(doctor.command, Commands::Doctor { .. }));

        let patch = Cli::try_parse_from([
            "patchforge",
            "patch",
            "--old",
            "base.apk",
            "--new",
            "target.apk",
        ])
        .unwrap();
        assert!(matches!(
            patch.command,
            Commands::Patch {
                command: None,
                old: Some(_),
                new: Some(_),
                ..
            }
        ));

        let automatic_patch = Cli::try_parse_from(["patchforge", "patch"]).unwrap();
        assert!(matches!(
            automatic_patch.command,
            Commands::Patch {
                command: None,
                old: None,
                new: None,
                ..
            }
        ));
    }

    #[test]
    fn prepares_automatic_flutter_build_inputs_and_cache_path() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("patchforge.yaml");
        let config = ProjectConfig {
            schema_version: 1,
            server_url: "https://updates.example.com".to_string(),
            app_id: "app-id".to_string(),
            app_name: "Example".to_string(),
            platform: "android".to_string(),
            channel: "stable".to_string(),
            signing_key_id: "main-2026".to_string(),
            signing_public_key: "public-key".to_string(),
            release_id: Some("release-id".to_string()),
            release_version: Some("1.0.0".to_string()),
        };

        assert_eq!(
            android_build_target("arm64-v8a").unwrap(),
            ("android-arm64", "app-arm64-v8a-release.apk")
        );
        assert!(android_build_target("unsupported").is_err());
        let defines = flutter_build_defines(&config);
        assert!(defines.contains(&"PATCHFORGE_APP_ID=app-id".to_string()));
        assert!(defines.contains(&"PATCHFORGE_CHANNEL=stable".to_string()));
        assert_eq!(
            cached_runtime_path(&config_path, &config, "arm64-v8a").unwrap(),
            temp.path()
                .join(".patchforge/releases/release-id/arm64-v8a/current-libapp.so")
        );
        assert_eq!(
            release_runtime_path(&config_path, &config, "arm64-v8a").unwrap(),
            temp.path()
                .join(".patchforge/releases/release-id/arm64-v8a/release-libapp.so")
        );
        assert_eq!(
            release_apk_path(&config_path, &config, "arm64-v8a").unwrap(),
            temp.path()
                .join(".patchforge/releases/release-id/arm64-v8a/release.apk")
        );
    }

    #[test]
    fn top_level_help_includes_the_primary_workflow_and_diagnostics() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("login"));
        assert!(help.contains("init"));
        assert!(help.contains("doctor"));
        assert!(help.contains("release"));
        assert!(help.contains("patch"));
        for hidden in ["token-hash", "publish", "verify"] {
            assert!(
                !help
                    .lines()
                    .any(|line| line.trim_start().starts_with(&format!("{hidden} ")))
            );
        }
    }

    #[test]
    fn keeps_legacy_nested_workflow_commands_compatible() {
        let release =
            Cli::try_parse_from(["patchforge", "release", "create", "--version", "1.0.0"]).unwrap();
        assert!(matches!(
            release.command,
            Commands::Release {
                command: Some(ReleaseCommands::Create { .. }),
                ..
            }
        ));

        let patch = Cli::try_parse_from([
            "patchforge",
            "patch",
            "release",
            "--old",
            "base.apk",
            "--new",
            "target.apk",
        ])
        .unwrap();
        assert!(matches!(
            patch.command,
            Commands::Patch {
                command: Some(PatchCommands::Release { .. }),
                ..
            }
        ));
    }
}
