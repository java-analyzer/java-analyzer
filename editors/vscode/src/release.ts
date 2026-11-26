import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import * as path from "node:path";

export const GITHUB_REPO = "java-analyzer/java-analyzer";
export const GITHUB_API_BASE = `https://api.github.com/repos/${GITHUB_REPO}`;

export type UpdateChannel = "stable" | "nightly" | "off";
export type ArchiveExt = "tar.gz" | "zip";

export interface PlatformTarget {
  triple: string;
  archiveExt: ArchiveExt;
  binName: string;
}

export interface InstalledServerMeta {
  tag: string;
  commit?: string;
  builtAt?: string;
  binaryPath: string;
}

export interface BuildInfo {
  tag: string;
  commit: string;
  built_at: string;
}

export function resolvePlatformTarget(
  platform: NodeJS.Platform,
  arch: string,
): PlatformTarget | undefined {
  if (platform === "darwin" && arch === "arm64") {
    return {
      triple: "aarch64-apple-darwin",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    };
  }
  if (platform === "darwin" && arch === "x64") {
    return {
      triple: "x86_64-apple-darwin",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    };
  }
  if (platform === "linux" && arch === "x64") {
    return {
      triple: "x86_64-unknown-linux-musl",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    };
  }
  if (platform === "linux" && arch === "arm64") {
    return {
      triple: "aarch64-unknown-linux-musl",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    };
  }
  if (platform === "win32" && arch === "x64") {
    return {
      triple: "x86_64-pc-windows-msvc",
      archiveExt: "zip",
      binName: "java-analyzer.exe",
    };
  }
  return undefined;
}

export function buildAssetName(
  tag: string,
  triple: string,
  ext: ArchiveExt,
): string {
  return `java-analyzer-${tag}-${triple}.${ext}`;
}

export function buildExtractedBinaryRelativePath(
  tag: string,
  triple: string,
  binName: string,
): string {
  return path.join(`java-analyzer-${tag}-${triple}`, binName);
}

export function releaseApiUrl(channel: Exclude<UpdateChannel, "off">): string {
  if (channel === "nightly") {
    return `${GITHUB_API_BASE}/releases/tags/nightly`;
  }
  return `${GITHUB_API_BASE}/releases/latest`;
}

export function parseSha256Sums(
  content: string,
  assetName: string,
): string | undefined {
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.length === 0) {
      continue;
    }
    const match = /^([a-fA-F0-9]{64})\s+\*?(.+)$/.exec(line);
    if (!match) {
      continue;
    }
    if (match[2].trim() === assetName) {
      return match[1].toLowerCase();
    }
  }
  return undefined;
}

export async function verifyFileSha256(
  filePath: string,
  expectedHex: string,
): Promise<boolean> {
  const normalizedExpected = expectedHex.trim().toLowerCase();
  if (!/^[a-f0-9]{64}$/.test(normalizedExpected)) {
    return false;
  }

  const hash = createHash("sha256");
  const stream = createReadStream(filePath);
  for await (const chunk of stream) {
    hash.update(chunk);
  }
  return hash.digest("hex").toLowerCase() === normalizedExpected;
}

export function parseBuildInfo(rawJson: string): BuildInfo | undefined {
  try {
    const parsed: unknown = JSON.parse(rawJson);
    if (typeof parsed !== "object" || parsed === null) {
      return undefined;
    }
    const record = parsed as Record<string, unknown>;
    if (
      typeof record.tag === "string" &&
      typeof record.commit === "string" &&
      typeof record.built_at === "string"
    ) {
      return {
        tag: record.tag,
        commit: record.commit,
        built_at: record.built_at,
      };
    }
    return undefined;
  } catch {
    return undefined;
  }
}

export function isUpdateNeeded(
  installedMeta: { tag: string; commit?: string } | undefined,
  remoteTag: string,
  remoteCommit?: string,
): boolean {
  if (!installedMeta) {
    return true;
  }
  if (installedMeta.tag !== remoteTag) {
    return true;
  }
  if (remoteTag === "nightly") {
    return Boolean(remoteCommit && installedMeta.commit !== remoteCommit);
  }
  return false;
}

export function shouldCheckForUpdates(
  lastCheckedMs: number | undefined,
  nowMs: number,
  intervalMs: number,
): boolean {
  if (lastCheckedMs === undefined) {
    return true;
  }
  return nowMs - lastCheckedMs >= intervalMs;
}
