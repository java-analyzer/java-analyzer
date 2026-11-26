//! Release archive, checksum, and build-info packager (`cargo xtask dist`).
//! See `docs/specs/0022-release-process-and-publishing.md`.

use std::{
    fs,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::{Compression, write::GzEncoder};
use serde::Serialize;
use zip::write::SimpleFileOptions;

use crate::{
    release_prep::{read_workspace_cargo_version, workspace_root},
    report::civil_from_days,
};

pub const HOST_TARGET: &str = env!("XTASK_HOST_TARGET");

#[derive(Debug, Clone, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct DistArgs {
    /// Path to the compiled java-analyzer binary [target[/<target>]/release/java-analyzer[.exe]]
    #[usage(long, value_name = "PATH")]
    pub bin_path: Option<PathBuf>,
    /// Target triple [host target]
    #[usage(long, value_name = "TRIPLE")]
    pub target: Option<String>,
    /// Release tag (e.g. `v0.1.0` or `nightly`) [v<workspace_version>]
    #[usage(long, value_name = "TAG")]
    pub tag: Option<String>,
    /// Host-runnable java-analyzer binary used to generate shell completions when cross-compiling
    #[usage(long, value_name = "PATH")]
    pub completions_bin: Option<PathBuf>,
    /// Output directory for release archive, SHA256SUMS, and BUILD_INFO.json
    #[usage(long, value_name = "DIR", default = "target/dist")]
    pub out_dir: PathBuf,
}

impl Default for DistArgs {
    fn default() -> Self {
        Self {
            bin_path: None,
            target: None,
            tag: None,
            completions_bin: None,
            out_dir: PathBuf::from("target/dist"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildInfo {
    pub tag: String,
    pub commit: String,
    pub built_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistOutput {
    pub archive_path: PathBuf,
    pub sha256sums_path: PathBuf,
    pub build_info_path: PathBuf,
    pub sha256: String,
}

pub fn run(args: &DistArgs) -> i32 {
    let root = match workspace_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    };
    match run_in(&root, args) {
        Ok(out) => {
            println!("dist: wrote {}", out.archive_path.display());
            println!("dist: sha256 = {}", out.sha256);
            println!("dist: wrote {}", out.sha256sums_path.display());
            println!("dist: wrote {}", out.build_info_path.display());
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

pub fn run_in(root: &Path, args: &DistArgs) -> Result<DistOutput, String> {
    let target = args
        .target
        .clone()
        .unwrap_or_else(|| HOST_TARGET.to_string());
    if target.trim().is_empty() {
        return Err("target triple must not be empty".to_string());
    }

    let tag = match &args.tag {
        Some(tag) if !tag.trim().is_empty() => tag.clone(),
        Some(_) => return Err("tag must not be empty".to_string()),
        None => {
            let cargo_path = root.join("Cargo.toml");
            let cargo_text = fs::read_to_string(&cargo_path)
                .map_err(|e| format!("failed to read {}: {e}", cargo_path.display()))?;
            let version = read_workspace_cargo_version(&cargo_text, &cargo_path)?;
            format!("v{version}")
        }
    };

    let is_windows = target.contains("windows");
    let exe_name = if is_windows {
        "java-analyzer.exe"
    } else {
        "java-analyzer"
    };

    let bin_path = resolve_bin_path(root, args.bin_path.as_deref(), &target, exe_name)?;
    let completions_bin = match &args.completions_bin {
        Some(path) => resolve_existing_file(root, path, "--completions-bin")?,
        None => bin_path.clone(),
    };

    let readme_bytes = read_required_file(&root.join("README.md"))?;
    let license_apache_bytes = read_required_file(&root.join("LICENSE-APACHE"))?;
    let license_mit_bytes = read_required_file(&root.join("LICENSE-MIT"))?;
    let bin_bytes = read_required_file(&bin_path)?;

    let bash_comp = generate_completion(&completions_bin, "bash")?;
    let zsh_comp = generate_completion(&completions_bin, "zsh")?;
    let fish_comp = generate_completion(&completions_bin, "fish")?;

    let stem = format!("java-analyzer-{tag}-{target}");
    let ext = if is_windows { "zip" } else { "tar.gz" };
    let archive_filename = format!("{stem}.{ext}");

    let out_dir = absolutize_against(root, &args.out_dir);
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;

    let archive_path = out_dir.join(&archive_filename);
    let entries = [
        ArchiveEntry::file(exe_name, &bin_bytes, 0o755),
        ArchiveEntry::file("README.md", &readme_bytes, 0o644),
        ArchiveEntry::file("LICENSE-APACHE", &license_apache_bytes, 0o644),
        ArchiveEntry::file("LICENSE-MIT", &license_mit_bytes, 0o644),
        ArchiveEntry::file("completions/java-analyzer.bash", &bash_comp, 0o644),
        ArchiveEntry::file("completions/_java-analyzer", &zsh_comp, 0o644),
        ArchiveEntry::file("completions/java-analyzer.fish", &fish_comp, 0o644),
    ];

    if is_windows {
        write_zip_archive(&archive_path, &stem, &entries)?;
    } else {
        write_tar_gz_archive(&archive_path, &stem, &entries)?;
    }

    let sha256 = sha256_file(&archive_path)?;
    let sha256sums_path = out_dir.join("SHA256SUMS");
    update_sha256sums(&sha256sums_path, &archive_filename, &sha256)?;

    let build_info = BuildInfo {
        tag,
        commit: detect_commit(root),
        built_at: current_utc_iso8601(),
    };
    let build_info_path = out_dir.join("BUILD_INFO.json");
    let build_info_json = serde_json::to_string_pretty(&build_info)
        .map_err(|e| format!("failed to serialize BUILD_INFO.json: {e}"))?;
    fs::write(&build_info_path, format!("{build_info_json}\n"))
        .map_err(|e| format!("failed to write {}: {e}", build_info_path.display()))?;

    Ok(DistOutput {
        archive_path,
        sha256sums_path,
        build_info_path,
        sha256,
    })
}

struct ArchiveEntry<'a> {
    rel_path: &'a str,
    bytes: &'a [u8],
    mode: u32,
}

impl<'a> ArchiveEntry<'a> {
    const fn file(rel_path: &'a str, bytes: &'a [u8], mode: u32) -> Self {
        Self {
            rel_path,
            bytes,
            mode,
        }
    }
}

fn absolutize_against(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn resolve_existing_file(root: &Path, path: &Path, flag: &str) -> Result<PathBuf, String> {
    let candidate = absolutize_against(root, path);
    if !candidate.is_file() {
        return Err(format!(
            "{flag} path does not exist or is not a file: {}",
            candidate.display()
        ));
    }
    Ok(candidate)
}

fn resolve_bin_path(
    root: &Path,
    explicit: Option<&Path>,
    target: &str,
    exe_name: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return resolve_existing_file(root, path, "--bin-path");
    }

    let target_release = root
        .join("target")
        .join(target)
        .join("release")
        .join(exe_name);
    if target_release.is_file() {
        return Ok(target_release);
    }

    let default_release = root.join("target").join("release").join(exe_name);
    if default_release.is_file() {
        return Ok(default_release);
    }

    Err(format!(
        "could not locate compiled binary at {} or {}; build with `cargo build --release -p java-analyzer` or pass `--bin-path`",
        target_release.display(),
        default_release.display()
    ))
}

fn read_required_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn generate_completion(bin: &Path, shell: &str) -> Result<Vec<u8>, String> {
    let output = Command::new(bin)
        .args(["completion", shell])
        .output()
        .map_err(|e| {
            format!(
                "failed to run `{} completion {shell}`: {e}",
                bin.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{} completion {shell}` exited with {}: {}",
            bin.display(),
            output.status,
            stderr.trim()
        ));
    }
    if output.stdout.is_empty() {
        return Err(format!(
            "`{} completion {shell}` produced empty output",
            bin.display()
        ));
    }
    Ok(output.stdout)
}

fn write_zip_archive(
    archive_path: &Path,
    stem: &str,
    entries: &[ArchiveEntry<'_>],
) -> Result<(), String> {
    let file = fs::File::create(archive_path)
        .map_err(|e| format!("failed to create {}: {e}", archive_path.display()))?;
    let mut writer = zip::ZipWriter::new(BufWriter::new(file));

    let dir_opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    writer
        .add_directory(format!("{stem}/"), dir_opts)
        .map_err(|e| format!("failed to add zip directory {stem}/: {e}"))?;
    writer
        .add_directory(format!("{stem}/completions/"), dir_opts)
        .map_err(|e| format!("failed to add zip directory {stem}/completions/: {e}"))?;

    for entry in entries {
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(entry.mode);
        let name = format!("{stem}/{}", entry.rel_path);
        writer
            .start_file(&name, opts)
            .map_err(|e| format!("failed to start zip entry {name}: {e}"))?;
        writer
            .write_all(entry.bytes)
            .map_err(|e| format!("failed to write zip entry {name}: {e}"))?;
    }

    writer
        .finish()
        .map_err(|e| format!("failed to finalize {}: {e}", archive_path.display()))?;
    Ok(())
}

fn write_tar_gz_archive(
    archive_path: &Path,
    stem: &str,
    entries: &[ArchiveEntry<'_>],
) -> Result<(), String> {
    let file = fs::File::create(archive_path)
        .map_err(|e| format!("failed to create {}: {e}", archive_path.display()))?;
    let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::default());

    write_tar_dir(&mut encoder, &format!("{stem}/"), 0o755)?;
    write_tar_dir(&mut encoder, &format!("{stem}/completions/"), 0o755)?;

    for entry in entries {
        let path = format!("{stem}/{}", entry.rel_path);
        write_tar_file(&mut encoder, &path, entry.bytes, entry.mode)?;
    }

    // Standard tar end-of-archive marker: two 512-byte zero blocks.
    encoder
        .write_all(&[0u8; 1024])
        .map_err(|e| format!("failed to write tar trailer: {e}"))?;
    encoder
        .finish()
        .map_err(|e| format!("failed to finalize {}: {e}", archive_path.display()))?;
    Ok(())
}

fn write_tar_dir(w: &mut impl Write, path: &str, mode: u32) -> Result<(), String> {
    let header = build_ustar_header(path, mode, 0, b'5')?;
    w.write_all(&header)
        .map_err(|e| format!("failed to write tar dir header {path}: {e}"))
}

fn write_tar_file(w: &mut impl Write, path: &str, bytes: &[u8], mode: u32) -> Result<(), String> {
    let header = build_ustar_header(path, mode, bytes.len() as u64, b'0')?;
    w.write_all(&header)
        .map_err(|e| format!("failed to write tar header {path}: {e}"))?;
    w.write_all(bytes)
        .map_err(|e| format!("failed to write tar payload {path}: {e}"))?;

    let rem = bytes.len() % 512;
    if rem != 0 {
        let pad = 512 - rem;
        w.write_all(&[0u8; 512][..pad])
            .map_err(|e| format!("failed to write tar padding {path}: {e}"))?;
    }
    Ok(())
}

fn build_ustar_header(
    path: &str,
    mode: u32,
    size: u64,
    typeflag: u8,
) -> Result<[u8; 512], String> {
    let (prefix, name) = split_ustar_path(path)?;
    let mut header = [0u8; 512];

    header[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut header[100..108], u64::from(mode), 7)?;
    write_octal(&mut header[108..116], 0, 7)?;
    write_octal(&mut header[116..124], 0, 7)?;
    write_octal(&mut header[124..136], size, 11)?;
    write_octal(&mut header[136..148], 0, 11)?;
    header[148..156].fill(b' ');
    header[156] = typeflag;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    if !prefix.is_empty() {
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    }

    let checksum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    let chksum_str = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(chksum_str.as_bytes());
    Ok(header)
}

fn split_ustar_path(path: &str) -> Result<(&str, &str), String> {
    if path.len() <= 100 {
        return Ok(("", path));
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    for (idx, _) in trimmed.match_indices('/') {
        let prefix = &path[..idx];
        let name = &path[idx + 1..];
        if !prefix.is_empty() && prefix.len() <= 155 && !name.is_empty() && name.len() <= 100 {
            return Ok((prefix, name));
        }
    }
    Err(format!("tar path exceeds ustar limits: '{path}'"))
}

fn write_octal(dst: &mut [u8], value: u64, digits: usize) -> Result<(), String> {
    let encoded = format!("{value:0width$o}", width = digits);
    if encoded.len() > digits || dst.len() < digits + 1 {
        return Err(format!("octal field overflow for value {value}"));
    }
    dst[..digits].copy_from_slice(encoded.as_bytes());
    dst[digits] = 0;
    Ok(())
}

fn update_sha256sums(path: &Path, archive_filename: &str, digest: &str) -> Result<(), String> {
    let mut lines: Vec<String> = if path.is_file() {
        let existing = fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        existing
            .lines()
            .filter(|line| {
                !line.trim().is_empty()
                    && !line
                        .split_once("  ")
                        .is_some_and(|(_, name)| name == archive_filename)
            })
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };

    lines.push(format!("{digest}  {archive_filename}"));
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(path, content).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn detect_commit(root: &Path) -> String {
    if let Ok(sha) = std::env::var("GITHUB_SHA")
        && !sha.trim().is_empty()
    {
        return sha.trim().to_string();
    }
    if let Ok(out) = Command::new("jj")
        .args(["log", "-r", "@-", "--no-graph", "-T", "commit_id"])
        .current_dir(root)
        .output()
        && out.status.success()
    {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !sha.is_empty() {
            return sha;
        }
    }
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        && out.status.success()
    {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !sha.is_empty() {
            return sha;
        }
    }
    "unknown".to_string()
}

fn current_utc_iso8601() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        });
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Computes the lowercase hex SHA-256 digest of a file on disk.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file =
        fs::File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

/// Computes the lowercase hex SHA-256 digest of an in-memory byte slice.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize_hex()
}

/// Minimal FIPS 180-4 SHA-256 implementation (zero external dependencies).
struct Sha256 {
    state: [u32; 8],
    bit_len: u64,
    buf: [u8; 64],
    buf_len: usize,
}

impl Sha256 {
    const H0: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];

    const fn new() -> Self {
        Self {
            state: Self::H0,
            bit_len: 0,
            buf: [0u8; 64],
            buf_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.bit_len = self.bit_len.wrapping_add((data.len() as u64) * 8);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            Self::compress(&mut self.state, block);
            data = rest;
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize_hex(mut self) -> String {
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            let block = self.buf;
            Self::compress(&mut self.state, &block);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&self.bit_len.to_be_bytes());
        let block = self.buf;
        Self::compress(&mut self.state, &block);

        let mut out = String::with_capacity(64);
        for word in self.state {
            use std::fmt::Write as _;
            let _ = write!(out, "{word:08x}");
        }
        out
    }

    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut w = [0u32; 64];
        let (chunks, _) = block.as_chunks::<4>();
        for (dst, &chunk) in w[..16].iter_mut().zip(chunks) {
            *dst = u32::from_be_bytes(chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for (&k, &wi) in Self::K.iter().zip(&w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k)
                .wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
}
