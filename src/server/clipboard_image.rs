use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STAGED_CLIPBOARD_IMAGE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const API_STAGED_CLIPBOARD_IMAGE_LEASE: Duration = Duration::from_secs(15 * 60);
pub(crate) const API_STAGING_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const API_STAGE_PREFIX: &str = "api-clipboard-";
const API_STAGE_MAX_FILES: usize = 64;
const API_STAGE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
#[cfg(unix)]
const DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

#[derive(Debug)]
pub(crate) struct StagedClipboardImage {
    pub(crate) path: PathBuf,
    pub(crate) paste_text: String,
}

#[derive(Clone, Copy)]
struct ApiQuota {
    max_files: usize,
    max_bytes: u64,
}

impl Default for ApiQuota {
    fn default() -> Self {
        Self {
            max_files: API_STAGE_MAX_FILES,
            max_bytes: API_STAGE_MAX_BYTES,
        }
    }
}

pub(crate) fn stage(
    client_id: u64,
    extension: &str,
    data: &[u8],
) -> io::Result<StagedClipboardImage> {
    validate_image(extension, data)?;
    let _guard = staging_lock();
    let dir = ensure_staging_dir()?;
    cleanup_legacy_stale(&dir, SystemTime::now());
    stage_in_dir(
        &dir,
        &format!("client-{client_id}-clipboard-"),
        extension,
        data,
        SystemTime::now(),
    )
}

pub(crate) fn stage_api(extension: &str, data: &[u8]) -> io::Result<StagedClipboardImage> {
    validate_image(extension, data)?;
    let _guard = staging_lock();
    let dir = ensure_staging_dir()?;
    stage_api_in(
        &dir,
        extension,
        data,
        SystemTime::now(),
        ApiQuota::default(),
    )
}

pub(crate) fn prepare_api_staging() -> io::Result<()> {
    let _guard = staging_lock();
    let dir = ensure_staging_dir()?;
    cleanup_expired_api_files_in(&dir, SystemTime::now())
}

pub(crate) fn cleanup_expired_api_files() -> io::Result<()> {
    let _guard = staging_lock();
    let dir = ensure_staging_dir()?;
    cleanup_expired_api_files_in(&dir, SystemTime::now())
}

pub(crate) fn cleanup_all_api_files() -> io::Result<()> {
    let _guard = staging_lock();
    let dir = ensure_staging_dir()?;
    remove_api_files_in(&dir, |_| true)
}

pub(crate) fn remove_files(paths: Vec<PathBuf>) {
    let _guard = staging_lock();
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

/// Clipboard image ingestion intentionally accepts only PNG. PNG is the format
/// emitted by Herdr's clipboard bridge and is the only format for which this
/// binary already has a bounded decoder. Expanding this set requires a decoder
/// and mismatch tests for each added format.
pub(crate) fn normalized_extension(extension: &str) -> Option<&'static str> {
    extension.eq_ignore_ascii_case("png").then_some("png")
}

fn validate_image(extension: &str, data: &[u8]) -> io::Result<&'static str> {
    let extension = normalized_extension(extension).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported clipboard image extension; expected png",
        )
    })?;
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard image bytes do not match the declared png extension",
        ));
    }

    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_limits(png::Limits {
        bytes: MAX_DECODED_IMAGE_BYTES,
    });
    let mut reader = decoder.read_info().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid png clipboard image: {err}"),
        )
    })?;
    let output_size = reader.output_buffer_size();
    if output_size == 0 || output_size > MAX_DECODED_IMAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "png clipboard image has invalid decoded dimensions",
        ));
    }
    let mut decoded = vec![0; output_size];
    reader.next_frame(&mut decoded).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid png clipboard image data: {err}"),
        )
    })?;
    reader.finish().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid png clipboard image trailer: {err}"),
        )
    })?;
    Ok(extension)
}

fn staging_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn staging_dir() -> PathBuf {
    #[cfg(unix)]
    {
        let root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/var/tmp"));
        root.join(format!("herdr-clipboard-images-{}", effective_uid()))
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join(format!("herdr-clipboard-images-{}", std::process::id()))
    }
}

fn ensure_staging_dir() -> io::Result<PathBuf> {
    let dir = staging_dir();
    ensure_staging_dir_at(&dir)?;
    Ok(dir)
}

fn ensure_staging_dir_at(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(_) => validate_staging_dir(dir),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            create_private_dir(dir)?;
            validate_staging_dir(dir)
        }
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    let mut builder = fs::DirBuilder::new();
    builder.mode(DIRECTORY_MODE);
    builder.create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir(dir)
}

fn validate_staging_dir(dir: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "clipboard image staging path is not a private directory: {}",
                dir.display()
            ),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        if metadata.uid() != effective_uid()
            || metadata.permissions().mode() & 0o777 != DIRECTORY_MODE
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "clipboard image staging directory is not owned and private: {}",
                    dir.display()
                ),
            ));
        }
    }
    Ok(())
}

fn stage_api_in(
    dir: &Path,
    extension: &str,
    data: &[u8],
    now: SystemTime,
    quota: ApiQuota,
) -> io::Result<StagedClipboardImage> {
    cleanup_expired_api_files_in(dir, now)?;
    let (count, bytes) = api_usage_in(dir)?;
    if count >= quota.max_files || bytes.saturating_add(data.len() as u64) > quota.max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "clipboard image staging quota exceeded",
        ));
    }
    let expires = now
        .checked_add(API_STAGED_CLIPBOARD_IMAGE_LEASE)
        .unwrap_or(now)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    stage_in_dir(
        dir,
        &format!("{API_STAGE_PREFIX}{expires}-"),
        extension,
        data,
        now,
    )
}

fn stage_in_dir(
    dir: &Path,
    prefix: &str,
    extension: &str,
    data: &[u8],
    now: SystemTime,
) -> io::Result<StagedClipboardImage> {
    let extension = normalized_extension(extension).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported clipboard image extension",
        )
    })?;
    let unique = now
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for attempt in 0..100 {
        let path = dir.join(format!("{prefix}{unique}-{attempt}.{extension}"));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        restrict_file_options(&mut options);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };
        if let Err(err) = file.write_all(data).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&path);
            return Err(err);
        }
        validate_staged_file(&file, data.len() as u64)?;
        return Ok(StagedClipboardImage {
            paste_text: path.to_string_lossy().into_owned(),
            path,
        });
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to allocate unique clipboard image staging path",
    ))
}

#[cfg(unix)]
fn restrict_file_options(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;

    options.mode(FILE_MODE).custom_flags(libc::O_NOFOLLOW);
}

#[cfg(not(unix))]
fn restrict_file_options(_options: &mut fs::OpenOptions) {}

fn validate_staged_file(file: &fs::File, expected_len: u64) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clipboard image staging file failed validation",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        if metadata.uid() != effective_uid()
            || metadata.permissions().mode() & 0o777 != FILE_MODE
            || metadata.nlink() != 1
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "clipboard image staging file is not private",
            ));
        }
    }
    Ok(())
}

fn cleanup_expired_api_files_in(dir: &Path, now: SystemTime) -> io::Result<()> {
    let now_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    remove_api_files_in(dir, |name| {
        api_expiry_from_name(name).is_some_and(|expires| expires <= now_secs)
    })
}

fn cleanup_legacy_stale(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(API_STAGE_PREFIX) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = safe_owned_regular_metadata(&path) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now.duration_since(modified).unwrap_or_default() > STAGED_CLIPBOARD_IMAGE_MAX_AGE {
            let _ = fs::remove_file(path);
        }
    }
}

fn api_usage_in(dir: &Path) -> io::Result<(usize, u64)> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if api_expiry_from_name(&name).is_none() {
            continue;
        }
        if let Ok(metadata) = safe_owned_regular_metadata(&entry.path()) {
            count = count.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok((count, bytes))
}

fn remove_api_files_in(dir: &Path, predicate: impl Fn(&str) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if api_expiry_from_name(&name).is_none() || !predicate(&name) {
            continue;
        }
        let path = entry.path();
        if safe_owned_regular_metadata(&path).is_ok() {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn api_expiry_from_name(name: &str) -> Option<u64> {
    name.strip_prefix(API_STAGE_PREFIX)?
        .split('-')
        .next()?
        .parse()
        .ok()
}

fn safe_owned_regular_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "staging entry is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;

        if metadata.uid() != effective_uid() || metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "staging entry is not safely owned",
            ));
        }
    }
    Ok(metadata)
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_png() -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3, 255]).unwrap();
        }
        encoded
    }

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "herdr-clipboard-image-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn accepts_only_fully_decodable_png_images() {
        let png = valid_png();
        assert_eq!(validate_image("PNG", &png).unwrap(), "png");
        assert!(validate_image("jpeg", &png).is_err());
        assert!(validate_image("png", b"arbitrary bytes").is_err());
        assert!(validate_image("png", b"\x89PNG\r\n\x1a\ntruncated").is_err());
    }

    #[test]
    fn api_staging_enforces_count_and_byte_quotas() {
        let dir = test_dir("quota");
        ensure_staging_dir_at(&dir).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(10_000);
        let png = valid_png();
        let quota = ApiQuota {
            max_files: 1,
            max_bytes: png.len() as u64,
        };
        stage_api_in(&dir, "png", &png, now, quota).unwrap();
        assert_eq!(
            stage_api_in(&dir, "png", &png, now, quota)
                .unwrap_err()
                .kind(),
            io::ErrorKind::StorageFull
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn api_cleanup_honors_lease_then_reclaims_expired_files() {
        let dir = test_dir("lease");
        ensure_staging_dir_at(&dir).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(20_000);
        let staged = stage_api_in(&dir, "png", &valid_png(), now, ApiQuota::default()).unwrap();
        cleanup_expired_api_files_in(
            &dir,
            now + API_STAGED_CLIPBOARD_IMAGE_LEASE - Duration::from_secs(1),
        )
        .unwrap();
        assert!(staged.path.exists());
        cleanup_expired_api_files_in(&dir, now + API_STAGED_CLIPBOARD_IMAGE_LEASE).unwrap();
        assert!(!staged.path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn staging_directory_rejects_prepositioned_symlink_without_chmodding_target() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};

        let root = test_dir("symlink");
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        let staged = root.join("staged");
        symlink(&target, &staged).unwrap();

        assert_eq!(
            ensure_staging_dir_at(&staged).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            fs::symlink_metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::remove_dir_all(root).unwrap();
    }
}
