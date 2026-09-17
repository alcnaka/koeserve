use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsConfig {
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<ModelSource>,
    #[serde(default)]
    pub voices: Vec<VoiceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSource {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    pub format: SourceFormat,
    pub license: String,
    #[serde(default)]
    pub license_url: Option<String>,
    /// License/notice files stored inside an archive. They are copied under
    /// models/licenses/<source-id>/ while preserving their archive path.
    #[serde(default)]
    pub license_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat { Raw, Zip, TarGz }

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceConfig {
    pub id: String,
    pub speaker: String,
    pub style: String,
    pub display_name: String,
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_path: Option<PathBuf>,
    #[serde(default)]
    pub file_sha256: Option<String>,
}

fn default_true() -> bool { true }

pub fn load_config(path: &Path) -> anyhow::Result<ModelsConfig> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read model configuration {}", path.display()))?;
    let config: ModelsConfig = toml::from_str(&source)
        .with_context(|| format!("failed to parse model configuration {}", path.display()))?;
    if config.version != 1 { bail!("unsupported model configuration version {}; expected version 1", config.version); }
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &ModelsConfig) -> anyhow::Result<()> {
    let mut source_ids = HashSet::new();
    for source in &config.sources {
        if source.id.trim().is_empty() || source.url.trim().is_empty() || source.license.trim().is_empty() {
            bail!("model source id, url and license must not be empty");
        }
        if !source_ids.insert(source.id.as_str()) { bail!("duplicate model source id: {}", source.id); }
        if let Some(hash) = &source.sha256 { validate_sha256(hash, "source sha256")?; }
        for path in &source.license_paths { validate_relative_member(path)?; }
    }
    let mut voice_ids = HashSet::new();
    for voice in &config.voices {
        for (field, value) in [("id", voice.id.as_str()), ("speaker", voice.speaker.as_str()), ("style", voice.style.as_str()), ("display_name", voice.display_name.as_str())] {
            if value.trim().is_empty() { bail!("voice configuration field {field} must not be empty"); }
        }
        if !voice_ids.insert(voice.id.as_str()) { bail!("duplicate voice id: {}", voice.id); }
        if voice.path.as_os_str().is_empty() { bail!("voice path must not be empty: {}", voice.id); }
        if let Some(source) = &voice.source {
            if !source_ids.contains(source.as_str()) { bail!("voice {} references unknown source {}", voice.id, source); }
        }
        if let Some(hash) = &voice.file_sha256 { validate_sha256(hash, "voice file_sha256")?; }
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str) -> anyhow::Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("{field} must be a 64-character hexadecimal SHA-256 value");
    }
    Ok(())
}

pub fn config_base_dir(config_path: &Path) -> &Path { config_path.parent().unwrap_or_else(|| Path::new(".")) }
pub fn voice_path(config_path: &Path, voice: &VoiceConfig) -> PathBuf {
    if voice.path.is_absolute() { voice.path.clone() } else { config_base_dir(config_path).join(&voice.path) }
}
fn license_root(config_path: &Path, source: &ModelSource) -> PathBuf {
    config_base_dir(config_path).join("licenses").join(&source.id)
}
fn archive_license_path(config_path: &Path, source: &ModelSource, member: &Path) -> PathBuf {
    license_root(config_path, source).join(member)
}
fn remote_license_path(config_path: &Path, source: &ModelSource) -> PathBuf {
    license_root(config_path, source).join("REMOTE_LICENSE.txt")
}

pub fn list(config_path: &Path) -> anyhow::Result<()> {
    let config = load_config(config_path)?;
    let sources: HashMap<_, _> = config.sources.iter().map(|s| (s.id.as_str(), s)).collect();
    println!("{:<24} {:<10} {:<12} {:<10} SOURCE", "VOICE", "SPEAKER", "STYLE", "STATUS");
    for voice in &config.voices {
        let status = if voice_path(config_path, voice).is_file() { "installed" } else { "missing" };
        let source = voice.source.as_deref().unwrap_or("manual");
        println!("{:<24} {:<10} {:<12} {:<10} {}", voice.id, voice.speaker, voice.style, status, source);
        if let Some(src) = voice.source.as_deref().and_then(|id| sources.get(id)) {
            println!("  license: {}{}", src.license, src.license_url.as_deref().map(|u| format!(" ({u})")).unwrap_or_default());
            println!("  license files: {}", license_root(config_path, src).display());
        }
    }
    Ok(())
}

pub async fn fetch(config_path: &Path, selectors: &[String], all: bool, allow_unverified: bool) -> anyhow::Result<()> {
    let config = load_config(config_path)?;
    let source_map: HashMap<_, _> = config.sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut wanted = HashSet::<String>::new();
    if all { wanted.extend(config.voices.iter().filter_map(|v| v.source.clone())); }
    for selector in selectors {
        if source_map.contains_key(selector.as_str()) { wanted.insert(selector.clone()); continue; }
        let voice = config.voices.iter().find(|v| v.id == *selector).with_context(|| format!("unknown voice/source: {selector}"))?;
        let source = voice.source.as_ref().with_context(|| format!("voice {} has no download source", voice.id))?;
        wanted.insert(source.clone());
    }
    if wanted.is_empty() { bail!("specify one or more voice/source IDs, or use --all"); }
    for source_id in wanted {
        let source = source_map.get(source_id.as_str()).expect("validated source");
        let voices: Vec<_> = config.voices.iter().filter(|v| v.source.as_deref() == Some(source_id.as_str())).collect();
        fetch_source(config_path, source, &voices, allow_unverified).await?;
    }
    Ok(())
}

async fn fetch_source(config_path: &Path, source: &ModelSource, voices: &[&VoiceConfig], allow_unverified: bool) -> anyhow::Result<()> {
    if voices.is_empty() { return Ok(()); }
    if source.sha256.is_none() && !allow_unverified {
        bail!("source {} has no sha256; refusing unverified download (use --allow-unverified to override)", source.id);
    }
    let models_ok = voices.iter().all(|v| verify_voice_file(config_path, v).unwrap_or(false));
    let licenses_ok = verify_source_licenses(config_path, source);
    if models_ok && licenses_ok {
        println!("{}: models and license files already installed; skipping", source.id);
        return Ok(());
    }

    println!("{}: downloading {}", source.id, source.url);
    println!("{}: license: {}{}", source.id, source.license,
        source.license_url.as_deref().map(|u| format!(" ({u})")).unwrap_or_default());
    let client = reqwest::Client::builder().user_agent(concat!("KoeServe/", env!("CARGO_PKG_VERSION"))).build()?;
    let response = client.get(&source.url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;
    if let Some(expected) = &source.sha256 {
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) { bail!("SHA-256 mismatch for {}: expected {}, got {}", source.id, expected, actual); }
    }

    match source.format {
        SourceFormat::Raw => install_raw(config_path, source, voices, &bytes)?,
        SourceFormat::Zip => { install_zip(config_path, voices, &bytes)?; install_zip_licenses(config_path, source, &bytes)?; }
        SourceFormat::TarGz => { install_targz(config_path, voices, &bytes)?; install_targz_licenses(config_path, source, &bytes)?; }
    }
    if source.license_paths.is_empty() {
        if let Some(url) = &source.license_url { install_remote_license(config_path, source, &client, url).await?; }
    }

    for voice in voices {
        if !verify_voice_file(config_path, voice)? { bail!("installed file verification failed for {}", voice.id); }
    }
    if !verify_source_licenses(config_path, source) { bail!("license file installation failed for {}", source.id); }
    println!("{}: installed {} voice file(s); license files: {}", source.id, voices.len(), license_root(config_path, source).display());
    Ok(())
}

async fn install_remote_license(config_path: &Path, source: &ModelSource, client: &reqwest::Client, url: &str) -> anyhow::Result<()> {
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    atomic_write_if_changed(&remote_license_path(config_path, source), &bytes)
}

fn install_raw(config_path: &Path, source: &ModelSource, voices: &[&VoiceConfig], bytes: &[u8]) -> anyhow::Result<()> {
    if voices.len() != 1 { bail!("raw source {} must map to exactly one voice", source.id); }
    atomic_write_if_changed(&voice_path(config_path, voices[0]), bytes)
}

fn install_zip(config_path: &Path, voices: &[&VoiceConfig], bytes: &[u8]) -> anyhow::Result<()> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)?;
    for voice in voices {
        let member = voice.source_path.as_ref().with_context(|| format!("voice {} needs source_path for zip source", voice.id))?;
        validate_relative_member(member)?;
        let member_str = member.to_string_lossy().replace('\\', "/");
        let mut file = archive.by_name(&member_str).with_context(|| format!("archive member not found for {}: {}", voice.id, member.display()))?;
        if file.enclosed_name().is_none() { bail!("unsafe zip member path: {}", member.display()); }
        let mut data = Vec::with_capacity(file.size() as usize); file.read_to_end(&mut data)?;
        atomic_write_if_changed(&voice_path(config_path, voice), &data)?;
    }
    Ok(())
}

fn install_zip_licenses(config_path: &Path, source: &ModelSource, bytes: &[u8]) -> anyhow::Result<()> {
    if source.license_paths.is_empty() { return Ok(()); }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for member in &source.license_paths {
        validate_relative_member(member)?;
        let key = member.to_string_lossy().replace('\\', "/");
        let mut file = archive.by_name(&key).with_context(|| format!("license archive member not found for {}: {}", source.id, member.display()))?;
        if file.enclosed_name().is_none() { bail!("unsafe zip license member path: {}", member.display()); }
        let mut data = Vec::with_capacity(file.size() as usize); file.read_to_end(&mut data)?;
        atomic_write_if_changed(&archive_license_path(config_path, source, member), &data)?;
    }
    Ok(())
}

fn install_targz(config_path: &Path, voices: &[&VoiceConfig], bytes: &[u8]) -> anyhow::Result<()> {
    for voice in voices { validate_relative_member(voice.source_path.as_ref().with_context(|| format!("voice {} needs source_path for tar_gz source", voice.id))?)?; }
    let wanted: HashMap<String, &VoiceConfig> = voices.iter().map(|v| {
        let p = v.source_path.as_ref().expect("source_path validated above"); (p.to_string_lossy().replace('\\', "/"), *v)
    }).collect();
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut installed = HashSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned(); validate_relative_member(&path)?;
        let key = path.to_string_lossy().replace('\\', "/");
        if let Some(voice) = wanted.get(&key) {
            if !entry.header().entry_type().is_file() { bail!("archive member for {} is not a regular file", voice.id); }
            let mut data = Vec::new(); entry.read_to_end(&mut data)?;
            atomic_write_if_changed(&voice_path(config_path, voice), &data)?; installed.insert(voice.id.clone());
        }
    }
    for voice in voices { if !installed.contains(&voice.id) { bail!("archive member not found for {}: {:?}", voice.id, voice.source_path); } }
    Ok(())
}

fn install_targz_licenses(config_path: &Path, source: &ModelSource, bytes: &[u8]) -> anyhow::Result<()> {
    if source.license_paths.is_empty() { return Ok(()); }
    let wanted: HashSet<String> = source.license_paths.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
    let mut found = HashSet::new();
    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(bytes)));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned(); validate_relative_member(&path)?;
        let key = path.to_string_lossy().replace('\\', "/");
        if wanted.contains(&key) {
            if !entry.header().entry_type().is_file() { bail!("license member is not a regular file: {}", path.display()); }
            let mut data = Vec::new(); entry.read_to_end(&mut data)?;
            atomic_write_if_changed(&archive_license_path(config_path, source, &path), &data)?; found.insert(key);
        }
    }
    for member in &source.license_paths {
        let key = member.to_string_lossy().replace('\\', "/");
        if !found.contains(&key) { bail!("license archive member not found for {}: {}", source.id, member.display()); }
    }
    Ok(())
}

fn validate_relative_member(path: &Path) -> anyhow::Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() { bail!("archive member path must be relative: {}", path.display()); }
    for c in path.components() {
        if matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)) { bail!("unsafe archive member path: {}", path.display()); }
    }
    Ok(())
}

fn atomic_write_if_changed(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if path.is_file() && fs::read(path)? == data { return Ok(()); }
    let parent = path.parent().unwrap_or_else(|| Path::new(".")); fs::create_dir_all(parent)?;
    let name = path.file_name().context("destination has no file name")?.to_string_lossy();
    let temp = parent.join(format!(".{name}.koeserve.tmp"));
    { let mut f = fs::File::create(&temp)?; f.write_all(data)?; f.sync_all()?; }
    if path.exists() { fs::remove_file(path)?; }
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn verify(config_path: &Path) -> anyhow::Result<()> {
    let config = load_config(config_path)?;
    let mut failed = 0usize;
    for voice in &config.voices {
        let path = voice_path(config_path, voice); let ok = verify_voice_file(config_path, voice)?;
        println!("{:<24} {} {}", voice.id, if ok { "OK" } else { "FAIL" }, path.display());
        if !ok { failed += 1; }
    }
    for source in &config.sources {
        let ok = verify_source_licenses(config_path, source);
        println!("license/{:<16} {} {}", source.id, if ok { "OK" } else { "FAIL" }, license_root(config_path, source).display());
        if !ok { failed += 1; }
    }
    if failed > 0 { bail!("{failed} model/license verification item(s) failed"); }
    Ok(())
}

fn verify_source_licenses(config_path: &Path, source: &ModelSource) -> bool {
    if !source.license_paths.is_empty() {
        return source.license_paths.iter().all(|p| archive_license_path(config_path, source, p).is_file());
    }
    if source.license_url.is_some() { return remote_license_path(config_path, source).is_file(); }
    true
}

fn verify_voice_file(config_path: &Path, voice: &VoiceConfig) -> anyhow::Result<bool> {
    let path = voice_path(config_path, voice); if !path.is_file() { return Ok(false); }
    if let Some(expected) = &voice.file_sha256 { let bytes = fs::read(&path)?; return Ok(sha256_hex(&bytes).eq_ignore_ascii_case(expected)); }
    Ok(true)
}
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes); digest.iter().map(|b| format!("{b:02x}")).collect()
}
