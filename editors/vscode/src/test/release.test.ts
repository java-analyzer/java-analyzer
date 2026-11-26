import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { describe, it } from "node:test";
import {
  buildAssetName,
  buildExtractedBinaryRelativePath,
  isUpdateNeeded,
  parseBuildInfo,
  parseSha256Sums,
  releaseApiUrl,
  resolvePlatformTarget,
  shouldCheckForUpdates,
  verifyFileSha256,
} from "../release";

describe("resolvePlatformTarget", () => {
  it("maps all 5 supported platform/arch combinations", () => {
    assert.deepEqual(resolvePlatformTarget("darwin", "arm64"), {
      triple: "aarch64-apple-darwin",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    });
    assert.deepEqual(resolvePlatformTarget("darwin", "x64"), {
      triple: "x86_64-apple-darwin",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    });
    assert.deepEqual(resolvePlatformTarget("linux", "x64"), {
      triple: "x86_64-unknown-linux-musl",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    });
    assert.deepEqual(resolvePlatformTarget("linux", "arm64"), {
      triple: "aarch64-unknown-linux-musl",
      archiveExt: "tar.gz",
      binName: "java-analyzer",
    });
    assert.deepEqual(resolvePlatformTarget("win32", "x64"), {
      triple: "x86_64-pc-windows-msvc",
      archiveExt: "zip",
      binName: "java-analyzer.exe",
    });
  });

  it("returns undefined for unsupported platforms", () => {
    assert.equal(resolvePlatformTarget("win32", "ia32"), undefined);
    assert.equal(resolvePlatformTarget("freebsd", "x64"), undefined);
  });
});

describe("buildAssetName and extracted paths", () => {
  it("formats stable and nightly archive names", () => {
    assert.equal(
      buildAssetName("v0.1.0", "aarch64-apple-darwin", "tar.gz"),
      "java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz",
    );
    assert.equal(
      buildAssetName("nightly", "x86_64-unknown-linux-musl", "tar.gz"),
      "java-analyzer-nightly-x86_64-unknown-linux-musl.tar.gz",
    );
    assert.equal(
      buildAssetName("v0.1.0", "x86_64-pc-windows-msvc", "zip"),
      "java-analyzer-v0.1.0-x86_64-pc-windows-msvc.zip",
    );
  });

  it("computes relative binary path inside archive", () => {
    assert.equal(
      buildExtractedBinaryRelativePath(
        "v0.1.0",
        "aarch64-apple-darwin",
        "java-analyzer",
      ),
      path.join("java-analyzer-v0.1.0-aarch64-apple-darwin", "java-analyzer"),
    );
  });
});

describe("SHA256SUMS parsing and verification", () => {
  it("parses standard sha256sum lines with two spaces or binary marker", () => {
    const hashA = "a".repeat(64);
    const hashB = "B".repeat(64);
    const content = [
      `${hashA}  java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz`,
      `${hashB} *java-analyzer-v0.1.0-x86_64-pc-windows-msvc.zip`,
      "",
      "not-a-valid-line",
    ].join("\n");

    assert.equal(
      parseSha256Sums(
        content,
        "java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz",
      ),
      hashA,
    );
    assert.equal(
      parseSha256Sums(content, "java-analyzer-v0.1.0-x86_64-pc-windows-msvc.zip"),
      "b".repeat(64),
    );
    assert.equal(parseSha256Sums(content, "missing.tar.gz"), undefined);
  });

  it("verifies real file SHA-256 digests", async () => {
    const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "ja-vscode-test-"));
    try {
      const samplePath = path.join(tmpDir, "archive.tar.gz");
      const payload = Buffer.from("java-analyzer-release-payload");
      await fs.writeFile(samplePath, payload);
      const expected = createHash("sha256").update(payload).digest("hex");

      assert.equal(await verifyFileSha256(samplePath, expected), true);
      assert.equal(
        await verifyFileSha256(samplePath, expected.toUpperCase()),
        true,
      );
      assert.equal(await verifyFileSha256(samplePath, "0".repeat(64)), false);
      assert.equal(await verifyFileSha256(samplePath, "short"), false);
    } finally {
      await fs.rm(tmpDir, { recursive: true, force: true });
    }
  });
});

describe("isUpdateNeeded and release metadata", () => {
  it("detects missing installs, stable tag upgrades, and nightly commit changes", () => {
    assert.equal(isUpdateNeeded(undefined, "v0.1.0"), true);
    assert.equal(
      isUpdateNeeded({ tag: "v0.1.0" }, "v0.1.0"),
      false,
    );
    assert.equal(
      isUpdateNeeded({ tag: "v0.1.0" }, "v0.2.0"),
      true,
    );
    assert.equal(
      isUpdateNeeded({ tag: "nightly", commit: "abc123" }, "nightly", "abc123"),
      false,
    );
    assert.equal(
      isUpdateNeeded({ tag: "nightly", commit: "abc123" }, "nightly", "def456"),
      true,
    );
    assert.equal(
      isUpdateNeeded({ tag: "nightly" }, "nightly", "def456"),
      true,
    );
    assert.equal(
      isUpdateNeeded({ tag: "nightly", commit: "abc123" }, "nightly", undefined),
      false,
    );
  });

  it("builds channel URLs, parses BUILD_INFO.json, and checks throttle intervals", () => {
    assert.equal(
      releaseApiUrl("stable"),
      "https://api.github.com/repos/java-analyzer/java-analyzer/releases/latest",
    );
    assert.equal(
      releaseApiUrl("nightly"),
      "https://api.github.com/repos/java-analyzer/java-analyzer/releases/tags/nightly",
    );
    assert.deepEqual(
      parseBuildInfo(
        '{"tag":"nightly","commit":"deadbeef","built_at":"2026-11-26T03:00:00Z"}',
      ),
      {
        tag: "nightly",
        commit: "deadbeef",
        built_at: "2026-11-26T03:00:00Z",
      },
    );
    assert.equal(parseBuildInfo("not json"), undefined);
    assert.equal(shouldCheckForUpdates(undefined, 1000, 600), true);
    assert.equal(shouldCheckForUpdates(500, 1000, 600), false);
    assert.equal(shouldCheckForUpdates(400, 1000, 600), true);
  });
});
